//! PDF text with pdf-extract (pure Rust, over lopdf). Port of `readPdf` in
//! `apps/desktop/src/platform/extract.ts`: pages joined by a blank line, the
//! document's Title as the title.

use pdf_extract::{Document, Object, PlainTextOutput};

use crate::Error;
use crate::backend::Result;

pub struct PdfText {
    pub title: String,
    pub text: String,
}

pub fn read_pdf(data: &[u8]) -> Result<PdfText> {
    // pdf-extract panics on some malformed files; a bad PDF must not take the worker thread down.
    std::panic::catch_unwind(|| read(data)).unwrap_or_else(|_| Err(Error::other("This PDF could not be read.")))
}

fn read(data: &[u8]) -> Result<PdfText> {
    let mut doc = Document::load_mem(data).map_err(|e| Error::other(format!("This PDF could not be opened: {e}")))?;
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return Err(Error::other("This PDF is password protected."));
    }
    let title = info_title(&doc).unwrap_or_default();
    let mut pages = Vec::new();
    for (number, _) in doc.get_pages() {
        let mut text = String::new();
        let mut out = PlainTextOutput::new(&mut text);
        // A page that can't be read is left out rather than failing the file.
        if pdf_extract::output_doc_page(&doc, &mut out, number).is_ok() {
            pages.push(tidy_page(&text));
        }
    }
    Ok(PdfText { title, text: pages.join("\n\n").trim().to_string() })
}

/// Lines trimmed at the end, runs of blank lines kept to one, ligatures spelled out (as pdf.js does).
fn tidy_page(text: &str) -> String {
    let text = expand_ligatures(text);
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    super::office::collapse_blank_lines(&lines.join("\n")).trim_matches('\n').to_string()
}

fn expand_ligatures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\u{fb00}' => out.push_str("ff"),
            '\u{fb01}' => out.push_str("fi"),
            '\u{fb02}' => out.push_str("fl"),
            '\u{fb03}' => out.push_str("ffi"),
            '\u{fb04}' => out.push_str("ffl"),
            '\u{fb05}' | '\u{fb06}' => out.push_str("st"),
            _ => out.push(c),
        }
    }
    out
}

/// The Info dictionary's Title, unless it is a placeholder like "untitled" or "()".
fn info_title(doc: &Document) -> Option<String> {
    let info = doc.trailer.get(b"Info").ok()?;
    let info = match info {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    let title = info.as_dict().ok()?.get(b"Title").ok()?;
    let title = match title {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    let t = pdf_extract::decode_text_string(title).ok()?;
    let t = t.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let placeholder = !t.chars().any(char::is_alphanumeric) || t.eq_ignore_ascii_case("untitled");
    (!placeholder).then(|| t.to_string())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small PDF with a Title and two pages of Helvetica text, built by hand
    /// with a correct xref table.
    pub fn sample_pdf() -> Vec<u8> {
        let page = |text: &[&str]| {
            let mut s = String::from("BT /F1 12 Tf 72 720 Td 14 TL\n");
            for line in text {
                s.push_str(&format!("({line}) Tj T*\n"));
            }
            s.push_str("ET");
            s
        };
        let c1 = page(&["Hello from page one.", "Second line here."]);
        let c2 = page(&["Page two talks about Rust."]);
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R /Resources << /Font << /F1 7 0 R >> >> >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 6 0 R /Resources << /Font << /F1 7 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", c1.len(), c1),
            format!("<< /Length {} >>\nstream\n{}\nendstream", c2.len(), c2),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
            "<< /Title (A Test Paper) >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, body).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 8 0 R >>\nstartxref\n{}\n%%EOF\n", objects.len() + 1, xref).as_bytes());
        out
    }

    #[test]
    fn reads_title_and_pages() {
        let pdf = read_pdf(&sample_pdf()).unwrap();
        assert_eq!(pdf.title, "A Test Paper");
        assert!(pdf.text.contains("Hello from page one."), "{:?}", pdf.text);
        assert!(pdf.text.contains("Second line here."), "{:?}", pdf.text);
        let one = pdf.text.find("Hello").unwrap();
        let two = pdf.text.find("Page two talks about Rust.").unwrap();
        assert!(one < two);
        assert!(pdf.text[one..two].contains("\n\n"), "pages are separated by a blank line: {:?}", pdf.text);
    }

    #[test]
    fn ligatures_are_spelled_out() {
        assert_eq!(tidy_page("\u{fb01}le pro\u{fb01}le  \n\n\n\noﬄine"), "file profile\n\noffline");
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(read_pdf(b"%PDF-1.4 not really").is_err());
        assert!(read_pdf(b"").is_err());
    }
}

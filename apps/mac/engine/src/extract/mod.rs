//! Turns a picked file or a URL into readable text on device. Port of
//! `apps/desktop/src/platform/extract.ts` and `office.ts`.
//!
//! Audio and video are not read here: [`Extracted::media`] tells the caller to
//! transcribe them (`crate::transcribe`).

mod html;
mod office;
mod pdf;
mod url;
mod xml;

pub use html::{read_html, tidy};
pub use office::{is_office_file, read_office};
pub use pdf::read_pdf;
pub use url::{USER_AGENT, extract_url, read_youtube};

use crate::backend::Result;
use crate::transcribe::is_media;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extracted {
    /// A title guess: the document's own title, else the file name without its extension.
    pub title: String,
    pub text: String,
    /// Audio or video: `text` is empty and the recording needs transcribing.
    pub media: bool,
}

/// Reads a file the user picked: PDF, Word, PowerPoint, Excel, EPUB, HTML, or text and Markdown.
pub fn extract(name: &str, mime: Option<&str>, bytes: &[u8]) -> Result<Extracted> {
    let mime = mime.unwrap_or_default().to_ascii_lowercase();
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).to_string();
    let lower = name.to_ascii_lowercase();
    let or_stem = |title: String| if title.is_empty() { stem.clone() } else { title };

    if is_media(&mime, name) {
        return Ok(Extracted { title: stem, text: String::new(), media: true });
    }
    if mime == "application/pdf" || lower.ends_with(".pdf") {
        let pdf = read_pdf(bytes)?;
        return Ok(Extracted { title: or_stem(pdf.title), text: pdf.text, media: false });
    }
    if is_office_file(name) {
        let office = read_office(name, bytes)?;
        return Ok(Extracted { title: or_stem(office.title), text: office.text, media: false });
    }
    let text = decode_text(bytes);
    if lower.ends_with(".html") || lower.ends_with(".htm") || mime == "text/html" {
        let url = format!("file:///{}", crate::net::encode_component(name));
        let page = read_html(&text, Some(&url));
        return Ok(Extracted { title: or_stem(page.title), text: page.text, media: false });
    }
    Ok(Extracted { title: stem, text: text.trim().to_string(), media: false })
}

/// The media type for a file name, for the files the add dialog takes; None when unknown.
pub fn mime_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "epub" => "application/epub+zip",
        "html" | "htm" => "text/html",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "mp3" | "mpga" | "mpeg" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "webm" => "video/webm",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        _ => return None,
    })
}

/// UTF-8 text without a byte order mark, replacing invalid bytes like `File.text()` does.
fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_types_by_extension() {
        assert_eq!(mime_for("Talk.M4A"), Some("audio/mp4"));
        assert_eq!(mime_for("paper.pdf"), Some("application/pdf"));
        assert_eq!(mime_for("README"), None);
        assert_eq!(mime_for("archive.zip"), None);
        // Every media type it names is one the engine transcribes.
        assert!(is_media(mime_for("clip.mov").unwrap(), "clip"));
    }

    #[test]
    fn media_is_flagged_for_transcription() {
        let out = extract("Interview.m4a", None, b"\0\0").unwrap();
        assert_eq!(out, Extracted { title: "Interview".into(), text: String::new(), media: true });
        assert!(extract("clip", Some("video/mp4"), b"").unwrap().media);
    }

    #[test]
    fn text_and_markdown_are_read_as_they_are() {
        let out = extract("notes.md", Some("text/markdown"), b"\xEF\xBB\xBF# Notes\n\nBody\n\n").unwrap();
        assert_eq!(out, Extracted { title: "notes".into(), text: "# Notes\n\nBody".into(), media: false });
        assert_eq!(extract("README", None, b"plain").unwrap().title, "README");
    }

    #[test]
    fn pdfs_use_their_title() {
        let out = extract("paper.PDF", None, &pdf::tests::sample_pdf()).unwrap();
        assert_eq!(out.title, "A Test Paper");
        assert!(out.text.contains("Page two talks about Rust."));
    }

    #[test]
    fn office_files_fall_back_to_the_file_name() {
        let doc = r#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Hi</w:t></w:r></w:p></w:body></w:document>"#;
        let out = extract("Letter.docx", None, &office::tests::zip_of(&[("word/document.xml", doc)])).unwrap();
        assert_eq!((out.title.as_str(), out.text.as_str()), ("Letter", "Hi"));
    }

    #[test]
    fn html_files_are_cleaned() {
        let out =
            extract("page.html", None, b"<html><head><title>Saved page</title></head><body><p>Kept text</p><script>bad()</script></body></html>").unwrap();
        assert_eq!(out.title, "Saved page");
        assert!(out.text.contains("Kept text") && !out.text.contains("bad()"), "{}", out.text);
    }
}

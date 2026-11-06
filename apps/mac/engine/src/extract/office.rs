//! Text out of Word, PowerPoint, Excel and EPUB files. All four are zip archives
//! of XML. Port of `apps/desktop/src/platform/office.ts`.

use std::cell::RefCell;
use std::io::{Cursor, Read};

use super::xml::{Element, parse};
use crate::Error;
use crate::backend::Result;

pub struct OfficeText {
    pub title: String,
    pub text: String,
}

struct Archive<'a> {
    zip: RefCell<zip::ZipArchive<Cursor<&'a [u8]>>>,
    names: Vec<String>,
}

impl Archive<'_> {
    /// A member as text, or None when it isn't there.
    fn read(&self, path: &str) -> Option<String> {
        let mut zip = self.zip.borrow_mut();
        let mut file = zip.by_name(path).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn xml(&self, path: &str) -> Option<Element> {
        self.read(path).map(|s| parse(&s))
    }
}

/// The extensions this reader handles.
pub fn is_office_file(name: &str) -> bool {
    matches!(extension(name).as_str(), "docx" | "pptx" | "xlsx" | "epub")
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub fn read_office(name: &str, data: &[u8]) -> Result<OfficeText> {
    let zip = zip::ZipArchive::new(Cursor::new(data))
        .map_err(|_| Error::other("This file could not be opened. Older .doc, .ppt and .xls files are not supported; save it in the newer format."))?;
    let names = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let archive = Archive { zip: RefCell::new(zip), names };
    let text = match extension(name).as_str() {
        "docx" => read_docx(&archive)?,
        "pptx" => read_pptx(&archive),
        "xlsx" => read_xlsx(&archive),
        _ => read_epub(&archive)?,
    };
    let title = core_title(&archive).or_else(|| epub_title(&archive)).unwrap_or_default();
    Ok(OfficeText { title, text: collapse_blank_lines(&text).trim().to_string() })
}

/// `\n{3,}` becomes a blank line.
pub(super) fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut newlines = 0;
    for c in text.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push(c);
            }
        } else {
            newlines = 0;
            out.push(c);
        }
    }
    out
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn core_title(a: &Archive) -> Option<String> {
    a.xml("docProps/core.xml")?.first("title").and_then(|t| non_empty(t.text()))
}

/// The number at the end of `slide12.xml`, for sorting.
fn number(path: &str) -> u32 {
    let stem = path.strip_suffix(".xml").unwrap_or(path);
    let digits: String = stem.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().unwrap_or(0)
}

/// Members matching `^{prefix}\d+\.xml$`, in numeric order.
fn numbered(a: &Archive, prefix: &str) -> Vec<String> {
    let mut found: Vec<String> = a
        .names
        .iter()
        .filter(|n| n.strip_prefix(prefix).and_then(|r| r.strip_suffix(".xml")).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
        .cloned()
        .collect();
    found.sort_by_key(|n| number(n));
    found
}

/// Text of a Word paragraph, keeping tabs and line breaks.
fn run_text(p: &Element) -> String {
    let mut out = String::new();
    for el in p.descendants() {
        match el.local.as_str() {
            "t" => out.push_str(&el.text()),
            "tab" => out.push('\t'),
            "br" | "cr" => out.push('\n'),
            _ => {}
        }
    }
    out
}

fn read_docx(a: &Archive) -> Result<String> {
    let doc = a.xml("word/document.xml").ok_or_else(|| Error::other("This Word file has no document body."))?;
    let mut out: Vec<String> = Vec::new();
    // Walk the body in order so tables stay where they are in the document.
    if let Some(body) = doc.first("body") {
        for block in body.child_elements() {
            match block.local.as_str() {
                "p" => {
                    let text = run_text(block).trim().to_string();
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
                "tbl" => {
                    let rows: Vec<String> = block
                        .all("tr")
                        .into_iter()
                        .map(|row| row.all("tc").into_iter().map(|c| c.all("p").into_iter().map(run_text).collect::<Vec<_>>().join(" ").trim().to_string()).collect::<Vec<_>>())
                        .filter(|cells| cells.iter().any(|c| !c.is_empty()))
                        .map(|cells| cells.join(" | "))
                        .collect();
                    if !rows.is_empty() {
                        out.push(rows.join("\n"));
                    }
                }
                "sdt" => {
                    let text = block.all("p").into_iter().map(run_text).collect::<Vec<_>>().join("\n").trim().to_string();
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(out.join("\n\n"))
}

fn read_pptx(a: &Archive) -> String {
    numbered(a, "ppt/slides/slide")
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let mut parts = vec![format!("Slide {}", i + 1)];
            if let Some(slide) = a.xml(path) {
                parts.extend(slide.all("p").into_iter().map(|p| p.texts("").trim().to_string()).filter(|l| !l.is_empty()));
            }
            let notes = a.xml(&path.replace("slides/slide", "notesSlides/notesSlide")).map(|n| n.texts(" ").trim().to_string()).unwrap_or_default();
            if !notes.is_empty() {
                parts.push(format!("Notes: {notes}"));
            }
            parts.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn read_xlsx(a: &Archive) -> String {
    let strings: Vec<String> = a.xml("xl/sharedStrings.xml").map(|s| s.all("si").into_iter().map(|si| si.texts("")).collect()).unwrap_or_default();
    let names: Vec<String> =
        a.xml("xl/workbook.xml").map(|w| w.all("sheet").into_iter().map(|s| s.attr("name").unwrap_or_default().to_string()).collect()).unwrap_or_default();
    numbered(a, "xl/worksheets/sheet")
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let name = names.get(i).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("Sheet {}", i + 1));
            let mut lines = vec![name];
            if let Some(sheet) = a.xml(path) {
                for row in sheet.all("row") {
                    let cells: Vec<String> = row
                        .all("c")
                        .into_iter()
                        .map(|c| {
                            let kind = c.attr("t");
                            if kind == Some("inlineStr") {
                                return c.texts("");
                            }
                            let v = c.first("v").map(|v| v.text()).unwrap_or_default();
                            if kind == Some("s") { v.trim().parse::<usize>().ok().and_then(|n| strings.get(n).cloned()).unwrap_or_default() } else { v }
                        })
                        .collect();
                    let line = cells.join("\t").trim_end().to_string();
                    if !line.is_empty() {
                        lines.push(line);
                    }
                }
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn opf_path(a: &Archive) -> Option<String> {
    a.xml("META-INF/container.xml")?.first("rootfile")?.attr("full-path").map(str::to_string)
}

fn epub_title(a: &Archive) -> Option<String> {
    let opf = a.xml(&opf_path(a)?)?;
    opf.first("title").and_then(|t| non_empty(t.text()))
}

fn read_epub(a: &Archive) -> Result<String> {
    let none = || Error::other("This EPUB has no table of contents.");
    let path = opf_path(a).ok_or_else(none)?;
    let opf = a.xml(&path).ok_or_else(none)?;
    let dir = path.rfind('/').map(|i| &path[..=i]).unwrap_or("");
    let items: Vec<(String, String)> =
        opf.all("item").into_iter().filter_map(|i| Some((i.attr("id")?.to_string(), i.attr("href")?.to_string()))).collect();
    let pages: Vec<String> = opf
        .all("itemref")
        .into_iter()
        .filter_map(|r| {
            let idref = r.attr("idref")?;
            items.iter().find(|(id, _)| id == idref).map(|(_, href)| href.clone())
        })
        .filter(|href| !href.is_empty())
        .map(|href| {
            let file = href.split('#').next().unwrap_or_default();
            let file = crate::net::percent_decode(file).unwrap_or_else(|| file.to_string());
            a.read(&format!("{dir}{file}")).map(|page| epub_page_text(&page)).unwrap_or_default()
        })
        .filter(|t| !t.is_empty())
        .collect();
    Ok(pages.join("\n\n"))
}

/// The readable blocks of one EPUB page, each once even when nested.
fn epub_page_text(page: &str) -> String {
    let doc = dom_query::Document::from(page);
    doc.select("script, style").remove();
    let blocks: Vec<String> = doc
        .select("body")
        .select("h1, h2, h3, h4, p, li, blockquote, pre")
        .nodes()
        .iter()
        .filter(|b| !b.parent().is_some_and(|p| p.is("p, li, blockquote, pre") || p.ancestors_it(None).any(|x| x.is("p, li, blockquote, pre"))))
        .map(|b| b.text().trim().to_string())
        .collect();
    let texts = if blocks.is_empty() { vec![doc.select("body").text().to_string()] } else { blocks };
    texts.into_iter().filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A zip with these members, for tests.
    pub fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut out);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in files {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        out.into_inner()
    }

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn docx_keeps_paragraphs_tables_and_breaks_in_order() {
        let doc = format!(
            r#"<w:document {W}><w:body>
<w:p><w:r><w:t>Intro</w:t><w:tab/><w:t>text</w:t><w:br/><w:t>next line</w:t></w:r></w:p>
<w:p><w:r><w:t>  </w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>
<w:sdt><w:sdtContent><w:p><w:r><w:t>Boxed</w:t></w:r></w:p></w:sdtContent></w:sdt>
<w:p><w:r><w:t>End &amp; done</w:t></w:r></w:p>
</w:body></w:document>"#
        );
        let core = r#"<cp:coreProperties xmlns:cp="c" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title> My Report </dc:title></cp:coreProperties>"#;
        let out = read_office("Report.DOCX", &zip_of(&[("word/document.xml", &doc), ("docProps/core.xml", core)])).unwrap();
        assert_eq!(out.title, "My Report");
        assert_eq!(out.text, "Intro\ttext\nnext line\n\nA | B\n\nBoxed\n\nEnd & done");
    }

    #[test]
    fn pptx_reads_slides_in_number_order_with_notes() {
        let slide = |t: &str| format!(r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{t}</a:t></a:r></a:p><a:p/></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#);
        let notes = r#"<p:notes xmlns:p="p" xmlns:a="a"><a:p><a:r><a:t>Say</a:t></a:r><a:r><a:t>this</a:t></a:r></a:p></p:notes>"#;
        let data = zip_of(&[
            ("ppt/slides/slide10.xml", &slide("Ten")),
            ("ppt/slides/slide2.xml", &slide("Two")),
            ("ppt/notesSlides/notesSlide2.xml", notes),
            ("ppt/slides/_rels/slide2.xml.rels", "<x/>"),
        ]);
        let out = read_office("deck.pptx", &data).unwrap();
        assert_eq!(out.text, "Slide 1\nTwo\nNotes: Say this\n\nSlide 2\nTen");
        assert_eq!(out.title, "");
    }

    #[test]
    fn xlsx_resolves_shared_and_inline_strings() {
        let shared = r#"<sst><si><t>Name</t></si><si><r><t>Ada</t></r><r><t> L.</t></r></si></sst>"#;
        let workbook = r#"<workbook><sheets><sheet name="People" sheetId="1"/><sheet name="" sheetId="2"/></sheets></workbook>"#;
        let s1 = r#"<worksheet><sheetData><row><c t="s"><v>0</v></c><c><v>42</v></c></row><row><c t="s"><v>1</v></c><c t="inlineStr"><is><t>x</t></is></c></row><row><c/></row></sheetData></worksheet>"#;
        let s2 = r#"<worksheet><sheetData><row><c><v>1</v></c><c/></row></sheetData></worksheet>"#;
        let data = zip_of(&[("xl/sharedStrings.xml", shared), ("xl/workbook.xml", workbook), ("xl/worksheets/sheet1.xml", s1), ("xl/worksheets/sheet2.xml", s2)]);
        assert_eq!(read_office("book.xlsx", &data).unwrap().text, "People\nName\t42\nAda L.\tx\n\nSheet 2\n1");
    }

    #[test]
    fn epub_follows_the_spine_and_skips_nested_blocks() {
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;
        let opf = r#"<package><metadata><dc:title xmlns:dc="d">A Book</dc:title></metadata><manifest>
<item id="c2" href="ch%202.xhtml"/><item id="c1" href="ch1.xhtml#start"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/><itemref idref="missing"/></spine></package>"#;
        let ch1 = "<html><body><h1>One</h1><script>x()</script><blockquote><p>Quoted</p></blockquote><ul><li>Item</li></ul></body></html>";
        let ch2 = "<html><body><div>Loose text</div></body></html>";
        let data = zip_of(&[("META-INF/container.xml", container), ("OEBPS/content.opf", opf), ("OEBPS/ch1.xhtml", ch1), ("OEBPS/ch 2.xhtml", ch2)]);
        let out = read_office("book.epub", &data).unwrap();
        assert_eq!(out.title, "A Book");
        assert_eq!(out.text, "One\n\nQuoted\n\nItem\n\nLoose text");
    }

    #[test]
    fn old_binary_formats_get_a_clear_error() {
        let err = read_office("old.doc.docx", b"\xD0\xCF\x11\xE0 not a zip").err().unwrap();
        assert!(err.to_string().contains("Older .doc, .ppt and .xls files are not supported"));
        let err = read_office("empty.docx", &zip_of(&[("x.xml", "<x/>")])).err().unwrap();
        assert_eq!(err.to_string(), "This Word file has no document body.");
    }
}

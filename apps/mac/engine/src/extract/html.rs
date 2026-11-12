//! Readable text out of a web page: Readability (dom_smoothie, a port of
//! Mozilla's), falling back to the page's text without scripts, navigation and
//! footers. Port of `readHtml` in `apps/desktop/src/platform/extract.ts`.

pub struct HtmlText {
    pub title: String,
    pub text: String,
}

/// `url` resolves relative links; it may be None or a file URL.
pub fn read_html(html: &str, url: Option<&str>) -> HtmlText {
    let page = dom_query::Document::from(html);
    let fallback_title = page.select("title").first().text().trim().to_string();

    let config = dom_smoothie::Config { text_mode: dom_smoothie::TextMode::Formatted, ..Default::default() };
    let article = std::panic::catch_unwind(|| {
        let mut r = dom_smoothie::Readability::new(html, url, Some(config.clone()))
            .or_else(|_| dom_smoothie::Readability::new(html, None, Some(config.clone())))
            .ok()?;
        r.parse().ok()
    })
    .ok()
    .flatten();
    if let Some(a) = article.filter(|a| !a.text_content.trim().is_empty()) {
        let title = a.title.trim();
        return HtmlText { title: if title.is_empty() { fallback_title } else { title.to_string() }, text: tidy(&a.text_content) };
    }

    page.select("script, style, noscript, nav, footer, header, svg").remove();
    HtmlText { title: fallback_title, text: tidy(&page.select("body").text()) }
}

/// Collapses spaces, trims around line breaks and keeps at most one blank line.
pub fn tidy(text: &str) -> String {
    let mut spaced = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c == ' ' || c == '\t' || c == '\u{a0}' {
            if !in_space {
                spaced.push(' ');
            }
            in_space = true;
        } else {
            spaced.push(c);
            in_space = false;
        }
    }
    let lines: Vec<&str> = spaced.split('\n').map(|l| l.trim_matches(' ')).collect();
    super::office::collapse_blank_lines(&lines.join("\n")).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readability_finds_the_article() {
        let para = "Rust gives you memory safety without a garbage collector, and this paragraph is long enough to count as content. ".repeat(4);
        let html = format!(
            "<html><head><title>Site | Post</title></head><body><nav><a href='/'>Home</a> <a href='/about'>About</a></nav>\
             <article><h1>Why Rust</h1><p>{para}</p><p>{para}</p></article><footer>Copyright footer</footer></body></html>"
        );
        let out = read_html(&html, Some("https://example.com/post"));
        assert!(out.text.contains("memory safety"), "{}", out.text);
        assert!(!out.text.contains("Copyright footer"), "{}", out.text);
        assert!(!out.title.is_empty());
    }

    #[test]
    fn short_pages_keep_their_text_without_scripts() {
        let out = read_html(
            "<html><head><title> Tiny </title><style>p{}</style></head><body><header>Top</header><p>Just   a\u{a0}line</p><script>x()</script></body></html>",
            None,
        );
        assert_eq!(out.title, "Tiny");
        assert!(out.text.contains("Just a line"), "{}", out.text);
        assert!(!out.text.contains("x()") && !out.text.contains("p{}"), "{}", out.text);
    }

    #[test]
    fn tidy_collapses_whitespace() {
        assert_eq!(tidy("  a \t b \n\n\n\n  c  \n"), "a b\n\nc");
    }
}

//! Interactive documents returned with a reader answer.
pub const MAX_HTML_BYTES: usize = 512 * 1024;

pub const GUIDANCE: &str = r#"When the question asks to visualize, animate or interactively explain a figure, return a short explanation followed by ONE complete fenced ```html block containing a self-contained HTML document. Use inline CSS, SVG and JavaScript, with no external libraries, fonts, images, fetches, links or other network resources. Explain the specific supplied figure and its linked text. Preserve axes, labels and relationships that are legible; never invent measurements or portray a schematic as the original data. Label schematic interpretations clearly. Give every control an accessible label and make the layout responsive down to 300px. Use a light or dark color scheme via prefers-color-scheme. For animations include Play, Pause, Step and Reset controls, start paused, and respect prefers-reduced-motion. Animate with CSS or the Web Animations API. For a button that returns to the paper, use parent.postMessage({type:'pdf-page',page:N}, '*') with the actual one-based PDF page number. The page runs in an isolated frame without access to the host app or network. Do not include HTML for an ordinary text question unless it helps answer the request."#;

#[derive(Clone)]
pub struct Visualization {
    pub html: String,
    pub before: String,
    pub after: String,
}

pub enum HtmlAnswer {
    Complete(Visualization),
    Incomplete(String),
    TooLarge(String),
}

pub fn parse(answer: &str) -> Option<HtmlAnswer> {
    let mut offset = 0;
    let mut start = None;
    for line in answer.split_inclusive('\n') {
        let fence = line.trim();
        if let Some((before, body)) = start {
            if fence == "```" {
                let before = answer[..before].trim().to_owned();
                return Some(if offset - body > MAX_HTML_BYTES {
                    HtmlAnswer::TooLarge(before)
                } else {
                    HtmlAnswer::Complete(Visualization { html: answer[body..offset].trim().into(), before, after: answer[offset + line.len()..].trim().into() })
                });
            }
        } else if fence.eq_ignore_ascii_case("```html") || fence.eq_ignore_ascii_case("```figure-html") {
            start = Some((offset, offset + line.len()));
        }
        offset += line.len();
    }
    start.map(|(before, _)| HtmlAnswer::Incomplete(answer[..before].trim().into()))
}

#[derive(Clone)]
pub struct OpenVisualization {
    pub message_id: String,
    pub document: Visualization,
    pub restart: u64,
}

#[derive(Clone, Copy)]
pub struct ReaderSplit {
    pub preferred: f32,
    pub available: f32,
}
impl Default for ReaderSplit {
    fn default() -> Self {
        Self { preferred: 380.0, available: 1032.0 }
    }
}
impl ReaderSplit {
    pub const MIN: f32 = 280.0;
    pub fn max(&self) -> f32 {
        (self.available - 328.0).clamp(Self::MIN, 900.0)
    }
    pub fn width(&self) -> f32 {
        self.preferred.clamp(Self::MIN, self.max())
    }
    pub fn resize(&mut self, width: f32) {
        if width.is_finite() {
            self.preferred = width.clamp(Self::MIN, self.max());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_complete_html_answers_are_renderable() {
        let Some(HtmlAnswer::Complete(v)) = parse("A schematic.\n\n```html\n<html><button>Play</button></html>\n```\n\nSee p. 7.") else {
            panic!("complete HTML")
        };
        assert_eq!(v.before, "A schematic.");
        assert_eq!(v.after, "See p. 7.");
        assert!(v.html.contains("<button>Play</button>"));
        assert!(matches!(parse("Explanation\n```html\n<script>"), Some(HtmlAnswer::Incomplete(_))));
        assert!(parse("```javascript\nalert(1)\n```").is_none());
        assert!(matches!(parse(&format!("```html\n{}\n```", "x".repeat(MAX_HTML_BYTES + 1))), Some(HtmlAnswer::TooLarge(_))));
    }
    #[test]
    fn resizing_keeps_paper_and_chat_usable_and_recovers_after_window_resize() {
        let mut split = ReaderSplit::default();
        split.resize(650.0);
        assert_eq!(split.width(), 650.0);
        split.available = 720.0;
        assert_eq!(split.width(), 392.0);
        split.available = 1032.0;
        assert_eq!(split.width(), 650.0);
        split.resize(10.0);
        assert_eq!(split.width(), ReaderSplit::MIN);
        split.resize(f32::NAN);
        assert_eq!(split.width(), ReaderSplit::MIN);
    }
}

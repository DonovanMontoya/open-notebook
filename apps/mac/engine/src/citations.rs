//! Citations in answers look like `[Source title]`. These helpers turn them into
//! links the UI can render as tabs, and find the passage a citation points at.
//! Port of `packages/engine/src/citations.ts`. Offsets are byte offsets into the Rust string.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitableSource {
    pub id: String,
    pub title: Option<String>,
}

static EXTENSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(pdf|docx?|md|txt|html?)$").unwrap());
static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+").unwrap());

fn normalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let bare = EXTENSION.replace(&lower, "");
    NON_WORD.replace_all(&bare, " ").trim().to_string()
}

/// Finds the source a bracketed label refers to: id, exact title, then prefix, then containment.
pub fn match_source<'a>(label: &str, sources: &'a [CitableSource]) -> Option<&'a CitableSource> {
    // Open Notebook servers cite by record id, like [source:abc123].
    if let Some(s) = sources.iter().find(|s| s.id == label.trim()) {
        return Some(s);
    }
    let want = normalize(label);
    if want.chars().count() < 2 {
        return None;
    }
    let titled: Vec<(&CitableSource, String)> =
        sources.iter().filter_map(|s| s.title.as_deref().filter(|t| !t.is_empty()).map(|t| (s, normalize(t)))).collect();
    titled
        .iter()
        .find(|(_, t)| *t == want)
        .or_else(|| titled.iter().find(|(_, t)| t.starts_with(&want) || want.starts_with(t.as_str())))
        .or_else(|| titled.iter().find(|(_, t)| want.chars().count() >= 6 && (t.contains(&want) || want.contains(t.as_str()))))
        .map(|(s, _)| *s)
}

static BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(!?)\[([^\]\n]{2,160})\]").unwrap());
static LABEL_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*;\s*").unwrap());
static INSIGHTS_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\(insights\)\s*$").unwrap());

pub struct Linked {
    pub markdown: String,
    pub cited: Vec<CitableSource>,
}

/// Rewrites `[Title]` citations as Markdown links to `cite:<index>` so a renderer
/// can draw them as tabs. Links, images and footnote-style `[1]` markers are left alone.
pub fn link_citations(markdown: &str, sources: &[CitableSource]) -> Linked {
    let mut cited: Vec<CitableSource> = Vec::new();
    let mut out = String::with_capacity(markdown.len());
    let mut copied = 0;
    let mut pos = 0;
    while let Some(caps) = BRACKET.captures_at(markdown, pos) {
        let whole = caps.get(0).unwrap();
        // `(?![(:[])`: a bracket followed by a link target is not a citation.
        if markdown[whole.end()..].starts_with(['(', ':', '[']) {
            pos = whole.start() + markdown[whole.start()..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        out.push_str(&markdown[copied..whole.start()]);
        copied = whole.end();
        pos = whole.end();
        if !caps[1].is_empty() {
            out.push_str(whole.as_str());
            continue;
        }
        // One bracket may hold several citations: [A; B].
        let inner = &caps[2];
        let parts: Vec<Option<String>> = LABEL_SPLIT
            .split(inner)
            .map(|label| {
                let source = match_source(label, sources)?;
                let index = match cited.iter().position(|c| c == source) {
                    Some(i) => i,
                    None => {
                        cited.push(source.clone());
                        cited.len() - 1
                    }
                };
                // The prompt labels insight-only context "Title (insights)"; the tab shows just the title.
                let text = if source.id == label.trim() {
                    source.title.clone().unwrap_or_else(|| "Source".into())
                } else {
                    INSIGHTS_SUFFIX.replace(label, "").into_owned()
                };
                Some(format!("[{}](cite:{index})", text.replace(['[', ']'], "")))
            })
            .collect();
        if parts.iter().all(Option::is_some) {
            out.push_str(&parts.into_iter().flatten().collect::<Vec<_>>().join(" "));
        } else {
            out.push_str(whole.as_str());
        }
    }
    out.push_str(&markdown[copied..]);
    Linked { markdown: out, cited }
}

static WORDS4: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]{4,}").unwrap());
static PARAGRAPH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\n]+(?:\n[^\n]+)*").unwrap());

fn terms(s: &str) -> HashSet<String> {
    WORDS4.find_iter(&s.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

/// The paragraph of a source that best matches some text, as byte offsets.
pub fn best_passage(full_text: &str, query: &str) -> Option<(usize, usize)> {
    let want = terms(query);
    if want.is_empty() || full_text.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize, f64)> = None;
    for m in PARAGRAPH.find_iter(full_text) {
        let have = terms(m.as_str());
        let hits = want.iter().filter(|t| have.contains(*t)).count();
        // Prefer dense matches over long paragraphs that match by sheer size.
        let score = hits as f64 / (have.len().max(8) as f64).sqrt();
        if hits > 0 && best.is_none_or(|b| score > b.2) {
            best = Some((m.start(), m.end(), score));
        }
    }
    best.map(|(s, e, _)| (s, e))
}

/// The sentence around a byte position in some text, for use as a passage query.
pub fn sentence_around(text: &str, index: usize) -> String {
    let index = index.min(text.len());
    let before = &text[..index];
    let start = [before.rfind(". "), before.rfind('\n')].into_iter().flatten().max().map_or(0, |i| i + 1);
    let ends: Vec<usize> = [text[index..].find(". "), text[index..].find('\n')].into_iter().flatten().map(|i| i + index).collect();
    let end = ends.iter().min().map_or(text.len(), |e| e + 1);
    text[start..end].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> Vec<CitableSource> {
        [("s1", "FTC-2022-Bringing-Dark-Patterns-to-Light.pdf"), ("s2", "Training language models to follow instructions"), ("s3", "Plan")]
            .into_iter()
            .map(|(id, t)| CitableSource { id: id.into(), title: Some(t.into()) })
            .collect()
    }

    #[test]
    fn match_source_handles_file_names_prefixes_and_case() {
        let s = sources();
        assert_eq!(match_source("FTC 2022 Bringing Dark Patterns to Light", &s).unwrap().id, "s1");
        assert_eq!(match_source("training language models", &s).unwrap().id, "s2");
        assert_eq!(match_source("plan", &s).unwrap().id, "s3");
        assert!(match_source("Unrelated", &s).is_none());
    }

    #[test]
    fn link_citations_turns_known_titles_into_cite_links() {
        let linked = link_citations(
            "Dark patterns are common [FTC 2022 Bringing Dark Patterns to Light]. See [the docs](https://x.y) and [1]. Also [Plan; Training language models] and [Nope].",
            &sources(),
        );
        assert_eq!(
            linked.markdown,
            "Dark patterns are common [FTC 2022 Bringing Dark Patterns to Light](cite:0). See [the docs](https://x.y) and [1]. Also [Plan](cite:1) [Training language models](cite:2) and [Nope]."
        );
        assert_eq!(linked.cited.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s1", "s3", "s2"]);
    }

    #[test]
    fn best_passage_picks_the_paragraph_that_shares_the_most_terms() {
        let text = "Intro about nothing much.\n\nThe launch happens on March third in Lisbon.\n\nBudget notes follow here.";
        let (s, e) = best_passage(text, "When is the launch in Lisbon?").unwrap();
        assert_eq!(&text[s..e], "The launch happens on March third in Lisbon.");
        assert!(best_passage(text, "zz").is_none());
    }

    #[test]
    fn sentence_around_returns_the_sentence_containing_an_index() {
        let text = "First point here. Second point [Plan] matters. Third.";
        assert_eq!(sentence_around(text, text.find("[Plan]").unwrap()), "Second point [Plan] matters.");
    }

    #[test]
    fn server_citations_by_record_id_show_the_source_title() {
        let s = [CitableSource { id: "source:s9".into(), title: Some("Primary sources".into()) }];
        let linked = link_citations("Shown in [source:s9] and [source_insight:x1].", &s);
        assert_eq!(linked.markdown, "Shown in [Primary sources](cite:0) and [source_insight:x1].");
        assert_eq!(linked.cited[0].id, "source:s9");
    }
}

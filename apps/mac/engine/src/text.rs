//! Text helpers shared by the local backend: chunking, vectors, titles and links.
//! Port of `packages/engine/src/text.ts`.
//!
//! Lengths are counted in characters (Unicode scalar values) where the TypeScript
//! counts UTF-16 code units; they agree for text without astral characters.

use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;

/// Number of characters in `s`.
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of character `n` (or the end of `s`).
pub fn char_offset(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// `s.slice(start, end)` in characters.
pub fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let a = char_offset(s, start);
    let b = if end <= start { a } else { a + char_offset(&s[a..], end - start) };
    &s[a..b]
}

/// The first `n` characters of `s`.
pub fn take_chars(s: &str, n: usize) -> &str {
    &s[..char_offset(s, n)]
}

/// The last `n` characters of `s`.
pub fn last_chars(s: &str, n: usize) -> &str {
    let len = char_len(s);
    &s[char_offset(s, len.saturating_sub(n))..]
}

static CRLF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r\n?").unwrap());
static BLANKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// Splits text into overlapping chunks on paragraph, then sentence, boundaries.
pub fn chunk_text(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let clean = CRLF.replace_all(text, "\n");
    let clean = BLANKS.replace_all(&clean, "\n\n");
    let clean = clean.trim();
    if clean.is_empty() {
        return Vec::new();
    }
    if char_len(clean) <= max_chars {
        return vec![clean.to_string()];
    }

    let mut pieces: Vec<String> = Vec::new();
    for paragraph in clean.split("\n\n") {
        if char_len(paragraph) <= max_chars {
            pieces.push(paragraph.to_string());
            continue;
        }
        // Long paragraph: split on sentence ends, and hard-split anything still too long.
        for sentence in split_sentences(paragraph) {
            let chars: Vec<char> = sentence.chars().collect();
            if chars.is_empty() {
                continue;
            }
            for part in chars.chunks(max_chars.max(1)) {
                pieces.push(part.iter().collect());
            }
        }
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for piece in pieces {
        if !current.is_empty() && char_len(&current) + char_len(&piece) + 2 > max_chars {
            let tail = if overlap > 0 { last_chars(&current, overlap).to_string() } else { String::new() };
            chunks.push(std::mem::take(&mut current));
            current = match tail.find(' ') {
                Some(cut) => tail[cut + 1..].to_string(),
                None => String::new(),
            };
        }
        if current.is_empty() {
            current = piece;
        } else {
            current.push_str("\n\n");
            current.push_str(&piece);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// The default chunk size used for sources.
pub fn chunk_source_text(text: &str) -> Vec<String> {
    chunk_text(text, 1800, 200)
}

/// `paragraph.split(/(?<=[.!?])\s+/)`.
fn split_sentences(paragraph: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut iter = paragraph.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if matches!(c, '.' | '!' | '?') {
            let end = i + c.len_utf8();
            let mut next = end;
            while let Some(&(j, w)) = iter.peek() {
                if !w.is_whitespace() {
                    break;
                }
                next = j + w.len_utf8();
                iter.next();
            }
            if next > end {
                out.push(&paragraph[start..end]);
                start = next;
            }
        }
    }
    out.push(&paragraph[start..]);
    out
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (*x as f64, *y as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na != 0.0 && nb != 0.0 { dot / (na * nb).sqrt() } else { 0.0 }
}

// Embeddings are stored as base64-encoded little-endian float32 arrays, as the TypeScript engine writes them.

pub fn encode_vector(vector: &[f32]) -> String {
    let bytes: Vec<u8> = vector.iter().flat_map(|v| v.to_le_bytes()).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// None when the text is not base64 of whole float32 values.
pub fn decode_vector(encoded: &str) -> Option<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded.trim()).ok()?;
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
}

/// Short title from the first line of some text.
pub fn title_from(text: &str, max: usize) -> String {
    let line = text.trim().split('\n').next().unwrap_or("").trim();
    if char_len(line) > max { format!("{}…", take_chars(line, max.saturating_sub(1)).trim_end()) } else { line.to_string() }
}

static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s<>"'),\]]+"#).unwrap());
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^https?:").unwrap());
static BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]+)+(/\S*)?$").unwrap());
static TRAILING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.,;]+$").unwrap());

/// Every http(s) address in some pasted text, deduplicated, in order.
pub fn parse_links(text: &str) -> Vec<String> {
    let found = LINK.find_iter(text).map(|m| m.as_str().to_string());
    let bare = text.split_whitespace().filter(|t| !SCHEME.is_match(t) && BARE.is_match(t)).map(|t| format!("https://{t}"));
    let mut out: Vec<String> = Vec::new();
    for u in found.chain(bare) {
        let u = TRAILING.replace(&u, "").into_owned();
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)```(?:json)?").unwrap());

/// The JSON object in a model reply, ignoring code fences and text around it.
pub fn json_object(raw: &str) -> crate::Result<serde_json::Value> {
    let text = FENCE.replace_all(raw, "");
    let bad = || crate::Error::other("The reply has no JSON object");
    let start = text.find('{').ok_or_else(bad)?;
    let end = text.rfind('}').ok_or_else(bad)?;
    if end < start {
        return Err(bad());
    }
    Ok(serde_json::from_str(&text[start..=end])?)
}

static WORDS3: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{L}\p{N}]{3,}").unwrap());

/// Distinct lower-case words of three or more letters or digits, in order.
pub fn keyword_terms(query: &str) -> Vec<String> {
    let lower = query.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for m in WORDS3.find_iter(&lower) {
        if !out.iter().any(|t| t == m.as_str()) {
            out.push(m.as_str().to_string());
        }
    }
    out
}

/// The share of terms the text contains.
pub fn keyword_score(text: &str, terms: &[String]) -> f64 {
    if terms.is_empty() {
        return 0.0;
    }
    let lower = text.to_lowercase();
    let hits = terms.iter().filter(|t| lower.contains(t.as_str())).count();
    hits as f64 / terms.len() as f64
}

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// About `size` characters around the first query term, for search results.
pub fn snippet(text: &str, query: &str, size: usize) -> String {
    let terms = keyword_terms(query);
    let lower = text.to_lowercase();
    // Positions are found in the lower-cased text and mapped back by character count.
    let at = terms.iter().filter_map(|t| lower.find(t.as_str()).map(|b| char_len(&lower[..b]))).min().unwrap_or(0);
    let start = at.saturating_sub(60);
    let body = char_slice(text, start, start + size);
    let body = SPACES.replace_all(body, " ");
    let more = start + size < char_len(text);
    format!("{}{}{}", if start > 0 { "…" } else { "" }, body.trim(), if more { "…" } else { "" })
}

/// Collapses whitespace and cuts at a word boundary.
pub fn clip(text: &str, max: usize) -> String {
    let t = SPACES.replace_all(text, " ");
    let t = t.trim();
    if char_len(t) <= max {
        return t.to_string();
    }
    let cut = take_chars(t, max);
    let cut = match cut.rfind(char::is_whitespace) {
        Some(i) => &cut[..i],
        None => cut,
    };
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_text_keeps_short_text_whole_and_splits_long_text_with_overlap() {
        assert_eq!(chunk_text("  short  ", 1800, 200), ["short"]);
        assert!(chunk_text("", 1800, 200).is_empty());

        let paragraph = |n: usize| format!("Paragraph {n} {}", "word ".repeat(80));
        let text = (0..12).map(paragraph).collect::<Vec<_>>().join("\n\n");
        let chunks = chunk_text(&text, 1000, 100);
        assert!(chunks.len() > 3);
        for c in &chunks {
            assert!(char_len(c) <= 1100, "chunk of {} chars is within bounds", c.len());
        }
        assert!(chunks.join(" ").contains("Paragraph 11"), "nothing is dropped");
    }

    #[test]
    fn chunk_text_hard_splits_a_single_enormous_sentence() {
        assert_eq!(chunk_text(&"x".repeat(5000), 1000, 0).len(), 5);
    }

    #[test]
    fn chunk_text_splits_on_sentence_ends() {
        let s = format!("{}. {}! {}", "a".repeat(600), "b".repeat(600), "c".repeat(600));
        let chunks = chunk_text(&s, 1000, 0);
        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].ends_with('.'));
    }

    #[test]
    fn vectors_survive_base64_round_trips_and_cosine_similarity_behaves() {
        let v = [0.25f32, -1.5, 3.0, 0.0];
        assert_eq!(decode_vector(&encode_vector(&v)).unwrap(), v);
        // The same bytes the TypeScript engine writes for [1, 2].
        assert_eq!(encode_vector(&[1.0, 2.0]), "AACAPwAAAEA=");
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn title_from_uses_the_first_line_and_truncates() {
        assert_eq!(title_from("Hello\nworld", 60), "Hello");
        assert_eq!(title_from(&"a".repeat(100), 10), "aaaaaaaaa…");
    }

    #[test]
    fn parse_links_finds_every_address_once() {
        let text = "Read https://a.com/x, then (https://b.org/paper.pdf).\nhttps://a.com/x\narxiv.org/abs/2203.02155 and not-a-link";
        assert_eq!(parse_links(text), ["https://a.com/x", "https://b.org/paper.pdf", "https://arxiv.org/abs/2203.02155"]);
    }

    #[test]
    fn json_object_ignores_fences_and_prose() {
        let v = json_object("Sure:\n```json\n{\"a\": 1}\n```").unwrap();
        assert_eq!(v["a"], 1);
        assert!(json_object("nothing").is_err());
    }

    #[test]
    fn snippets_and_clips() {
        assert_eq!(snippet("Short text about zebras.", "zebras", 220), "Short text about zebras.");
        let long = format!("{} zebra {}", "x ".repeat(100), "y ".repeat(200));
        let s = snippet(&long, "zebra", 100);
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("zebra"));
        assert_eq!(clip("one two three", 9), "one two…");
        assert_eq!(keyword_terms("The the CAT, a cat!"), ["the", "cat"]);
    }
}

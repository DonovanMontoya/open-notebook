//! Checks an episode script against the material it was written from.
//! Port of `packages/engine/src/podcast/check.ts`.
//!
//! A model lists the script's factual claims, rules on each one and copies the
//! passage that backs it. The quotes are then looked up in the material here,
//! in code, so a claim only counts as supported when its evidence is really in
//! the user's sources.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::script::PodcastScript;
use crate::backend::Result;
use crate::text::json_object;
use crate::types::{CheckedClaim, ClaimVerdict};
use crate::Error;

/// The result of one check, as saved in `podcast_check.result` (with `writer` and `history`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCheck {
    pub claims: Vec<CheckedClaim>,
    pub supported: u32,
    pub overstated: u32,
    pub unsupported: u32,
    /// Distinct sources that back at least one claim.
    pub sources_used: u32,
    /// Distinct sources in the material (notes not counted).
    pub sources_total: u32,
}

pub const CHECK_SYSTEM: &str = "You are a careful fact-checker. You compare a podcast script with the research material it was written from and report exactly what the material does and does not support. You reply with JSON only.";

pub fn check_prompt(script: &PodcastScript, material: &str) -> String {
    let mut n = 0;
    let numbered = script
        .chapters
        .iter()
        .map(|c| {
            let mut lines = vec![format!("## {}", c.title)];
            for l in &c.lines {
                n += 1;
                lines.push(format!("[{n}] {}", l.text));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    [
        "Check this podcast script against the research material below.".to_string(),
        [
            "List every factual claim the script makes: findings, numbers, what a study did or concluded, causes and effects, definitions, and descriptions of what a source says. Skip greetings, transitions, questions, jokes and opinions the host clearly frames as their own.",
            "For each claim give a verdict:",
            "- \"supported\": the material says this, at the same strength.",
            "- \"overstated\": the material says something related but weaker, narrower or hedged, or the script drops a caveat, generalizes beyond what was studied, or treats a correlation or proposal as an established result.",
            "- \"unsupported\": the material does not say this, even if it may be true elsewhere.",
            "For \"supported\" and \"overstated\", copy the passage from the material that the claim rests on, word for word, at most 40 words. Do not paraphrase, fix typos or join separate passages. For \"unsupported\" leave the quote empty.",
            "For \"overstated\" and \"unsupported\", add one plain sentence saying what is wrong.",
        ]
        .join("\n"),
        "Answer with JSON only, in exactly this shape:".into(),
        r#"{"claims": [{"line": 12, "claim": "Short restatement", "verdict": "supported", "quote": "Exact words from the material", "note": ""}]}"#.into(),
        format!("# Script\n\n{numbered}"),
        format!("# Material\n\n{material}"),
    ]
    .join("\n\n")
}

static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+").unwrap());
static ELLIPSIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.\.\.|…").unwrap());

/// Lowercase words and numbers only, so quotes match across punctuation, case and line breaks.
fn normalize(text: &str) -> String {
    NON_WORD.replace_all(&text.to_lowercase(), " ").trim().to_string()
}

/// One piece of the material an episode is written from. Saved as JSON in `podcast_prompt.material`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MaterialPart {
    pub title: String,
    pub text: String,
    /// A passage from a source too long to send whole.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excerpt: bool,
    /// One of the user's notes rather than a source.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub note: bool,
}

/// The material as the models read it, one headed section per part.
pub fn material_text(parts: &[MaterialPart]) -> String {
    parts
        .iter()
        .map(|p| format!("## {}{}\n{}", if p.note { "Note: " } else if p.excerpt { "Excerpt from " } else { "" }, p.title, p.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub is_note: bool,
    pub text: String,
}

pub fn material_sections(parts: &[MaterialPart]) -> Vec<Section> {
    parts.iter().map(|p| Section { title: p.title.clone(), is_note: p.note, text: normalize(&p.text) }).collect()
}

const WINDOW: usize = 4;

/// Finds the section a quote comes from. Short quotes must appear whole.
/// Longer ones may differ slightly (hyphenation, a dropped or misspelled word
/// from a PDF), so 80% of their words must be covered by four-word runs that
/// appear in one section.
pub fn find_quote<'a>(quote: &str, sections: &'a [Section]) -> Option<&'a Section> {
    let fragments: Vec<String> = ELLIPSIS.split(quote).map(normalize).filter(|f| f.split(' ').count() >= 3).collect();
    if fragments.is_empty() {
        return None;
    }
    sections.iter().find(|s| {
        fragments.iter().all(|f| {
            if s.text.contains(f.as_str()) {
                return true;
            }
            let words: Vec<&str> = f.split(' ').collect();
            if words.len() < 8 {
                return false;
            }
            let mut covered = vec![false; words.len()];
            for i in 0..=words.len() - WINDOW {
                if s.text.contains(&words[i..i + WINDOW].join(" ")) {
                    covered[i..i + WINDOW].fill(true);
                }
            }
            covered.iter().filter(|c| **c).count() as f64 / words.len() as f64 >= 0.8
        })
    })
}

fn trimmed(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// `Math.round(Number(x)) || 1`.
fn line_number(v: &Value) -> i64 {
    let n = match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    let r = (n + 0.5).floor() as i64;
    if r == 0 { 1 } else { r }
}

/// Reads the checker's JSON and verifies every quote against the material.
pub fn parse_check(raw: &str, script: &PodcastScript, material: &[MaterialPart]) -> Result<ScriptCheck> {
    let data = json_object(raw).map_err(|_| Error::other("The claim check came back as invalid JSON."))?;
    let Some(list) = data["claims"].as_array() else {
        return Err(Error::other("The claim check came back without a list of claims."));
    };
    let lines = script.lines();
    let sections = material_sections(material);
    let mut claims: Vec<CheckedClaim> = Vec::new();
    for c in list {
        let Some(claim) = trimmed(&c["claim"]) else { continue };
        let line = line_number(&c["line"]).clamp(1, lines.len().max(1) as i64) as usize;
        let quote = trimmed(&c["quote"]);
        let mut verdict = match c["verdict"].as_str() {
            Some("supported") => ClaimVerdict::Supported,
            Some("overstated") => ClaimVerdict::Overstated,
            _ => ClaimVerdict::Unsupported,
        };
        let mut note = trimmed(&c["note"]);
        let found = quote.as_deref().and_then(|q| find_quote(q, &sections));
        if verdict == ClaimVerdict::Supported && found.is_none() {
            verdict = ClaimVerdict::Unsupported;
            note = Some("The checker cited a passage that is not actually there, so this claim has no evidence.".into());
        }
        claims.push(CheckedClaim {
            line,
            said: lines.get(line - 1).map(|s| s.to_string()).unwrap_or_default(),
            claim,
            verdict,
            quote,
            source: found.map(|s| s.title.clone()),
            quote_found: found.is_some(),
            note,
        });
    }
    claims.sort_by_key(|c| c.line);

    let count = |v: ClaimVerdict| claims.iter().filter(|c| c.verdict == v).count() as u32;
    let sources: BTreeSet<&str> = sections.iter().filter(|s| !s.is_note).map(|s| s.title.as_str()).collect();
    let used: BTreeSet<&str> = claims
        .iter()
        .filter(|c| c.verdict == ClaimVerdict::Supported)
        .filter_map(|c| c.source.as_deref())
        .filter(|s| sources.contains(s))
        .collect();
    Ok(ScriptCheck {
        supported: count(ClaimVerdict::Supported),
        overstated: count(ClaimVerdict::Overstated),
        unsupported: count(ClaimVerdict::Unsupported),
        sources_used: used.len() as u32,
        sources_total: sources.len() as u32,
        claims,
    })
}

pub const RESEARCH_SYSTEM: &str =
    "You are a research assistant who reads sources closely and reports what they actually show, including their limits. You write plain text.";

/// Study notes on each source, written before the script so findings keep their strength and caveats.
pub fn research_prompt(material: &str, focus: Option<&str>) -> String {
    [
        "Read the research material below and write study notes on each source, to be used by someone writing a podcast about it.".to_string(),
        focus.filter(|f| !f.is_empty()).map(|f| format!("The episode will focus on: {f}")).unwrap_or_default(),
        [
            "For each source, under a heading with its title:",
            "- The question it asks and why it matters.",
            "- What was done: method, data, sample size, setting.",
            "- The main findings, with exact numbers, each with a short quote copied word for word.",
            "- The limitations and caveats the authors state, and what the source does not show.",
            "- How strong the evidence is: an experiment, an observational study, a model or simulation, a survey, an opinion piece or a proposal.",
            "Then note where sources agree, disagree or build on each other.",
            "Do not add anything the material does not say.",
        ]
        .join("\n"),
        format!("# Material\n\n{material}"),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

pub const REVISE_SYSTEM: &str =
    "You are an editor who corrects podcast scripts so every claim matches the research it is based on. You reply with JSON only.";

fn verdict_name(v: ClaimVerdict) -> &'static str {
    match v {
        ClaimVerdict::Supported => "supported",
        ClaimVerdict::Overstated => "overstated",
        ClaimVerdict::Unsupported => "unsupported",
    }
}

/// Asks for the script back with the flagged lines corrected and everything else left alone.
pub fn revise_prompt(script: &PodcastScript, flagged: &[&CheckedClaim], material: &str) -> String {
    let problems = flagged
        .iter()
        .map(|c| {
            let mut parts = vec![format!("- Line {} ({}): \"{}\"", c.line, verdict_name(c.verdict), c.said), format!("  Claim: {}", c.claim)];
            if let Some(note) = c.note.as_deref().filter(|n| !n.is_empty()) {
                parts.push(format!("  Problem: {note}"));
            }
            if let Some(quote) = c.quote.as_deref().filter(|q| !q.is_empty() && c.quote_found) {
                parts.push(format!("  What the source says: \"{quote}\""));
            }
            parts.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    [
        "A fact-check found problems in this podcast script. Fix them and return the whole script.".to_string(),
        [
            "Rules:",
            "- For an overstated claim, say what the source says at the strength it says it, and keep its caveats.",
            "- For a claim that is not in the sources, remove it, or have the speaker frame it clearly as their own view or an open question.",
            "- Change only what you need to. Keep the title, the chapters, the speakers, the voice and the length close to the original.",
            "- Write for the ear, as before: no lists, no Markdown, no stage directions.",
        ]
        .join("\n"),
        format!("# Problems\n{problems}"),
        "Answer with JSON only, in the same shape as the script:".into(),
        format!("# Script\n{}", serde_json::to_string(script).unwrap_or_default()),
        format!("# Material\n\n{material}"),
    ]
    .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::podcast::tests::script;

    pub(crate) fn material() -> Vec<MaterialPart> {
        vec![
            MaterialPart {
                title: "FTC report".into(),
                text: "Staff found that 76% of subscription sites used at least one dark pattern.\n## Methods\nThe report does not measure how much money users lost.".into(),
                ..Default::default()
            },
            MaterialPart {
                title: "Design study".into(),
                excerpt: true,
                text: "In a lab study of 120 people, hidden cancel buttons roughly doubled the time to cancel —\nbut the effect on real-world churn was not tested.".into(),
                ..Default::default()
            },
            MaterialPart { title: "My thoughts".into(), note: true, text: "Ask about enforcement.".into(), ..Default::default() },
        ]
    }

    #[test]
    fn the_claim_check_verifies_every_quote_against_the_material_itself() {
        let raw = serde_json::json!({
            "claims": [
                { "line": 1, "claim": "76% of sites used a dark pattern", "verdict": "supported", "quote": "staff found that 76% of subscription sites used at least one dark pattern" },
                // Punctuation and a line break differ from the source, the words do not.
                { "line": 2, "claim": "Hidden buttons double cancel time", "verdict": "supported", "quote": "hidden cancel buttons roughly doubled the time to cancel, but the effect" },
                { "line": 2, "claim": "Hidden buttons raise churn", "verdict": "overstated", "quote": "the effect on real-world churn was not tested", "note": "Churn was not measured." },
                // The model says supported, but the quote is made up.
                { "line": 3, "claim": "Users lost $2 billion", "verdict": "supported", "quote": "users lost an estimated two billion dollars to dark patterns" },
                { "line": 3, "claim": "Regulators will act soon", "verdict": "unsupported", "quote": "" },
            ],
        });
        let check = parse_check(&format!("```json\n{raw}\n```"), &script(), &material()).unwrap();
        let verdicts: Vec<ClaimVerdict> = check.claims.iter().map(|c| c.verdict).collect();
        use ClaimVerdict::*;
        assert_eq!(verdicts, [Supported, Supported, Overstated, Unsupported, Unsupported]);
        assert_eq!(check.claims[0].source.as_deref(), Some("FTC report"));
        assert_eq!(check.claims[1].source.as_deref(), Some("Design study"));
        assert!(!check.claims[3].quote_found);
        assert!(check.claims[3].note.as_deref().unwrap().contains("not actually there"));
        assert_eq!(check.claims[2].said, "four five");
        assert_eq!((check.supported, check.overstated, check.unsupported), (2, 1, 2));
        assert_eq!((check.sources_used, check.sources_total), (2, 2), "notes are not counted as sources");
    }

    #[test]
    fn find_quote_tolerates_a_few_pdf_slips_in_a_long_quote_but_not_a_different_sentence() {
        let sections = material_sections(&material());
        let hit = find_quote("In a lab study of 120 people, hidden cancel butons roughly doubled the time to cancel", &sections);
        assert_eq!(hit.map(|s| s.title.as_str()), Some("Design study"));
        assert!(find_quote("In a field study of 9000 people, hidden cancel buttons tripled churn", &sections).is_none());
        assert!(find_quote("76%", &sections).is_none(), "too short to count as evidence");
    }

    #[test]
    fn a_broken_check_reply_is_a_readable_error() {
        assert!(parse_check("no json here", &script(), &material()).unwrap_err().to_string().contains("invalid JSON"));
        assert!(parse_check(r#"{"x": 1}"#, &script(), &material()).unwrap_err().to_string().contains("without a list"));
    }

    #[test]
    fn the_saved_material_keeps_the_typescript_shape() {
        let json = serde_json::to_value(material()).unwrap();
        assert_eq!(json[0], serde_json::json!({ "title": "FTC report", "text": material()[0].text }));
        assert_eq!(json[1]["excerpt"], true);
        assert_eq!(json[2]["note"], true);
        assert!(material_text(&material()).starts_with("## FTC report\n"));
        assert!(material_text(&material()).contains("## Excerpt from Design study\n") && material_text(&material()).contains("## Note: My thoughts\n"));
    }
}

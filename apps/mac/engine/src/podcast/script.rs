//! Writes the episode script from the notebook with the chosen chat model.
//! Port of `packages/engine/src/podcast/script.ts`.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::WORDS_PER_SEGMENT;
use crate::backend::Result;
use crate::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptLine {
    /// Index of the speaker in the speaker profile.
    pub host: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptChapter {
    pub title: String,
    pub lines: Vec<ScriptLine>,
}

/// The script as it is saved on the episode (`podcast_episode.script`), in the TypeScript shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PodcastScript {
    pub title: String,
    pub summary: String,
    pub chapters: Vec<ScriptChapter>,
}

impl PodcastScript {
    /// Spoken lines in order, numbered from 1 across chapters.
    pub fn lines(&self) -> Vec<&str> {
        self.chapters.iter().flat_map(|c| c.lines.iter().map(|l| l.text.as_str())).collect()
    }

    pub fn line_count(&self) -> usize {
        self.chapters.iter().map(|c| c.lines.len()).sum()
    }
}

/// A speaker as the script writer sees them.
#[derive(Clone, Copy, Debug)]
pub struct Persona<'a> {
    pub name: &'a str,
    pub backstory: &'a str,
    pub personality: &'a str,
}

pub struct ScriptInput<'a> {
    pub notebook_name: &'a str,
    pub material: &'a str,
    pub speakers: Vec<Persona<'a>>,
    pub briefing: &'a str,
    pub segments: u32,
    pub language: Option<&'a str>,
    pub focus: Option<&'a str>,
    /// Study notes on the sources, from the research pass.
    pub notes: Option<&'a str>,
}

pub const SCRIPT_SYSTEM: &str =
    "You are an audio producer who writes accurate, engaging podcast scripts from research material. You reply with JSON only.";

pub fn script_prompt(input: &ScriptInput<'_>) -> String {
    let speakers = &input.speakers;
    let cast = if speakers.len() == 1 {
        format!("one host, {} (host 0), speaking directly to the listener.", speakers[0].name)
    } else {
        let names = speakers.iter().enumerate().map(|(i, s)| format!("{} (host {i})", s.name)).collect::<Vec<_>>().join(", ");
        format!("{} speakers who talk with each other, not at the listener: {names}.", speakers.len())
    };
    let personas = speakers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut lines = vec![format!("- {} (host {i})", s.name)];
            if !s.backstory.is_empty() {
                lines.push(format!("  Background: {}", s.backstory));
            }
            if !s.personality.is_empty() {
                lines.push(format!("  Personality: {}", s.personality));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let words = input.segments * WORDS_PER_SEGMENT;
    let focus = input.focus.filter(|f| !f.is_empty());
    let language = input.language.filter(|l| !l.is_empty());
    let notes = input.notes.filter(|n| !n.is_empty());

    [
        format!("Write the script for an audio episode about the research notebook \"{}\". The show has {cast}", input.notebook_name),
        format!("# Speakers\n{personas}"),
        format!("# Briefing\n{}", input.briefing),
        focus.map(|f| format!("# Focus of this episode\n{f}")).unwrap_or_default(),
        format!("Length: about {words} spoken words in {} chapters.", input.segments),
        language.map(|l| format!("Write the whole script in this language: {l}.")).unwrap_or_default(),
        [
            "Rules:",
            "- Base every claim on the material below. Name the source when it helps the listener (\"the FTC report says…\"). Never invent facts, numbers or quotes.",
            "- Write for the ear: short sentences, plain words, contractions, no lists, no Markdown, no URLs, no stage directions or sound effects.",
            "- Each line is one turn of speech, usually one to four sentences. Open with a hook, not a greeting. End with a short takeaway.",
            "- Keep each speaker in character. Chapter titles are short and specific, like a table of contents.",
        ]
        .join("\n"),
        "Answer with JSON only, in exactly this shape:".into(),
        r#"{"title": "Episode title", "summary": "One sentence about the episode", "chapters": [{"title": "Chapter title", "lines": [{"host": 0, "text": "Spoken words"}]}]}"#.into(),
        notes
            .map(|n| format!("# Research notes\nThese notes were made from the material below. Use them to keep each finding at the strength its source states, with its caveats.\n\n{n}"))
            .unwrap_or_default(),
        "# Material".into(),
        input.material.to_string(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)```(?:json)?").unwrap());
static DIRECTIONS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[[^\]]*\]|\([^)]*(laughs|pause|music|sighs)[^)]*\)").unwrap());
static MARKDOWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*_#`>]").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Removes Markdown and bracketed directions a model may slip into spoken text.
fn clean_spoken(text: &str) -> String {
    let t = DIRECTIONS.replace_all(text, "");
    let t = MARKDOWN.replace_all(&t, "");
    SPACES.replace_all(&t, " ").trim().to_string()
}

/// `Number(x) || 0` for a host index.
fn number(v: &Value) -> f64 {
    let n = match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) if s.trim().is_empty() => 0.0,
        Value::String(s) => s.trim().parse().unwrap_or(f64::NAN),
        Value::Bool(b) => *b as u8 as f64,
        _ => 0.0,
    };
    if n.is_nan() { 0.0 } else { n }
}

fn trimmed(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Reads the model's JSON, tolerating code fences and stray text around it.
pub fn parse_script(raw: &str, host_count: usize) -> Result<PodcastScript> {
    let text = FENCE.replace_all(raw, "");
    let (start, end) = match (text.find('{'), text.rfind('}')) {
        (Some(s), Some(e)) if e > s => (s, e),
        _ => return Err(Error::other("The script came back without the expected structure. Try again.")),
    };
    let data: Value = serde_json::from_str(&text[start..=end])
        .map_err(|_| Error::other("The script came back as invalid JSON. Try again, or pick a stronger chat model."))?;
    let max_host = host_count.saturating_sub(1) as f64;
    let chapters: Vec<ScriptChapter> = data["chapters"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, c)| ScriptChapter {
            title: trimmed(&c["title"]).unwrap_or_else(|| format!("Part {}", i + 1)),
            lines: c["lines"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|l| {
                    let host = if l["host"].is_null() { &l["speaker"] } else { &l["host"] };
                    ScriptLine {
                        host: number(host).clamp(0.0, max_host).trunc() as usize,
                        text: l["text"].as_str().map(clean_spoken).unwrap_or_default(),
                    }
                })
                .filter(|l| !l.text.is_empty())
                .collect(),
        })
        .filter(|c| !c.lines.is_empty())
        .collect();
    if chapters.is_empty() {
        return Err(Error::other("The script had no spoken lines. Try again."));
    }
    Ok(PodcastScript {
        title: trimmed(&data["title"]).unwrap_or_else(|| "Untitled episode".into()),
        summary: data["summary"].as_str().map(|s| s.trim().to_string()).unwrap_or_default(),
        chapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_script_accepts_fenced_json_clamps_hosts_and_strips_stage_directions() {
        let raw = "Sure!\n```json\n{\"title\":\"T\",\"summary\":\"S\",\"chapters\":[{\"title\":\"A\",\"lines\":[{\"host\":5,\"text\":\"Hi **there** [laughs]\"},{\"host\":0,\"text\":\"\"}]},{\"title\":\"Empty\",\"lines\":[]}]}\n```";
        let script = parse_script(raw, 2).unwrap();
        assert_eq!(script.chapters, [ScriptChapter { title: "A".into(), lines: vec![ScriptLine { host: 1, text: "Hi there".into() }] }]);
        assert!(parse_script("no json here", 2).unwrap_err().to_string().contains("expected structure"));
    }

    #[test]
    fn untitled_parts_and_speaker_fields_are_read_like_the_typescript() {
        let raw = r#"{"chapters":[{"lines":[{"speaker":"1","text":"Hello (pauses) there"}]}]}"#;
        let script = parse_script(raw, 3).unwrap();
        assert_eq!(script.title, "Untitled episode");
        assert_eq!(script.chapters[0].title, "Part 1");
        assert_eq!(script.chapters[0].lines[0], ScriptLine { host: 1, text: "Hello there".into() });
        assert!(parse_script(r#"{"chapters": []}"#, 1).unwrap_err().to_string().contains("no spoken lines"));
    }

    #[test]
    fn the_prompt_names_the_cast_and_leaves_out_empty_parts() {
        let speakers = vec![Persona { name: "Ana", backstory: "", personality: "Calm" }];
        let input = ScriptInput {
            notebook_name: "N",
            material: "M",
            speakers,
            briefing: "B",
            segments: 2,
            language: None,
            focus: None,
            notes: None,
        };
        let prompt = script_prompt(&input);
        assert!(prompt.contains("The show has one host, Ana (host 0), speaking directly to the listener."));
        assert!(prompt.contains("- Ana (host 0)\n  Personality: Calm"));
        assert!(prompt.contains("about 560 spoken words in 2 chapters"));
        assert!(!prompt.contains("Focus") && !prompt.contains("Research notes") && !prompt.contains("Background"));
        assert!(prompt.ends_with("# Material\n\nM"));
    }
}

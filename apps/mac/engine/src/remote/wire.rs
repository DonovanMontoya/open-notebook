//! Server response shapes (`packages/api-client/src/types.ts`, `api/models.py`)
//! and how they map to the engine's types. Fields the server may send as null
//! are optional here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::types::*;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ServerSource {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub topics: Option<Vec<String>>,
    #[serde(default)]
    pub asset: Option<SourceAsset>,
    #[serde(default)]
    pub full_text: Option<String>,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default)]
    pub embedded_chunks: u32,
    #[serde(default)]
    pub insights_count: u32,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub file_available: Option<bool>,
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub processing_info: Option<Map<String, Value>>,
    #[serde(default)]
    pub notebooks: Option<Vec<String>>,
}

impl ServerSource {
    pub fn summary(&self) -> SourceSummary {
        SourceSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            topics: self.topics.clone().unwrap_or_default(),
            asset: self.asset.clone(),
            embedded: self.embedded,
            embedded_chunks: self.embedded_chunks,
            insights_count: self.insights_count,
            created: self.created.clone(),
            updated: self.updated.clone(),
            file_available: self.file_available.unwrap_or(false),
            status: self.status.clone(),
            // The web UI shows processing_info.error for failed sources.
            error: self
                .processing_info
                .as_ref()
                .and_then(|p| p.get("error"))
                .and_then(Value::as_str)
                .filter(|e| !e.is_empty())
                .map(str::to_string),
            notebooks: self.notebooks.clone().unwrap_or_default(),
        }
    }

    pub fn detail(self) -> SourceDetail {
        let summary = self.summary();
        SourceDetail { summary, full_text: self.full_text.unwrap_or_default() }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerChatMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// Human and AI messages; anything else (tool or system turns) is left out.
pub fn messages(list: Vec<ServerChatMessage>) -> Vec<ChatMessage> {
    list.into_iter()
        .filter_map(|m| {
            let kind = match m.kind.as_str() {
                "human" => MessageKind::Human,
                "ai" => MessageKind::Ai,
                _ => return None,
            };
            Some(ChatMessage { id: m.id, kind, content: m.content, timestamp: m.timestamp })
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerChatSession {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub message_count: Option<u32>,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<ServerChatMessage>>,
}

impl ServerChatSession {
    pub fn session(&self) -> ChatSession {
        ChatSession {
            id: self.id.clone(),
            title: self.title.clone(),
            notebook_id: self.notebook_id.clone().unwrap_or_default(),
            created: self.created.clone(),
            updated: self.updated.clone(),
            message_count: self.message_count,
            model_override: self.model_override.clone(),
        }
    }

    pub fn with_messages(self) -> ChatSessionWithMessages {
        let session = self.session();
        ChatSessionWithMessages { session, messages: messages(self.messages.unwrap_or_default()) }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExecuteChatResponse {
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub messages: Vec<ServerChatMessage>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BuildContextResponse {
    pub context: Value,
    #[serde(default)]
    pub token_count: Option<f64>,
    #[serde(default)]
    pub char_count: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerInsight {
    pub id: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub insight_type: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

impl From<ServerInsight> for Insight {
    fn from(i: ServerInsight) -> Self {
        Insight {
            id: i.id,
            source_id: i.source_id,
            kind: i.insight_type,
            content: i.content,
            created: i.created,
            status: Some("completed".into()),
            error: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerTransformation {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub apply_default: bool,
}

impl From<ServerTransformation> for Transformation {
    fn from(t: ServerTransformation) -> Self {
        Transformation { id: t.id, name: t.name, title: t.title, description: t.description, prompt: t.prompt, apply_default: t.apply_default }
    }
}

pub fn transformation_body(t: &Transformation) -> Value {
    json!({ "name": t.name, "title": t.title, "description": t.description, "prompt": t.prompt, "apply_default": t.apply_default })
}

#[derive(Clone, Debug, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub relevance: Option<f64>,
    #[serde(default)]
    pub similarity: Option<f64>,
    #[serde(default)]
    pub matches: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub results: Vec<SearchResult>,
}

impl From<SearchResult> for SearchHit {
    fn from(r: SearchResult) -> Self {
        let parent = r.parent_id.filter(|p| !p.is_empty());
        let is_note = parent.as_deref().is_some_and(|p| p.starts_with("note:")) || r.id.starts_with("note:");
        SearchHit {
            kind: if is_note { HitKind::Note } else { HitKind::Source },
            id: parent.unwrap_or(r.id),
            title: r.title.unwrap_or_default(),
            snippet: r.matches.and_then(|m| m.into_iter().next()).unwrap_or_default(),
            score: r.similarity.or(r.relevance).unwrap_or(0.0),
            notebook_id: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerUsageGroup {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub feature: String,
    #[serde(default)]
    pub calls: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub audio_seconds: f64,
    #[serde(default)]
    pub estimated_usd: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerUsage {
    #[serde(default)]
    pub days: u32,
    #[serde(default)]
    pub estimated_usd: Option<f64>,
    #[serde(default)]
    pub calls: u64,
    #[serde(default)]
    pub groups: Vec<ServerUsageGroup>,
}

impl From<ServerUsage> for UsageReport {
    fn from(u: ServerUsage) -> Self {
        UsageReport {
            days: u.days,
            calls: u.calls,
            estimated_usd: u.estimated_usd,
            estimated: false,
            groups: u
                .groups
                .into_iter()
                .map(|g| UsageGroup {
                    model: g.model,
                    feature: g.feature,
                    calls: g.calls,
                    input_tokens: g.input_tokens,
                    output_tokens: g.output_tokens,
                    audio_seconds: g.audio_seconds,
                    estimated_usd: g.estimated_usd,
                })
                .collect(),
        }
    }
}

// Podcasts

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NamedRef {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TranscriptLine {
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub dialogue: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Transcript {
    #[serde(default)]
    pub transcript: Option<Vec<TranscriptLine>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PodcastEpisode {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub briefing: Option<String>,
    #[serde(default)]
    pub audio_file: Option<String>,
    #[serde(default)]
    pub audio_url: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub job_status: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub episode_profile: Option<NamedRef>,
    #[serde(default)]
    pub speaker_profile: Option<NamedRef>,
    #[serde(default)]
    pub transcript: Option<Transcript>,
}

fn server_status(job: Option<&str>, has_audio: bool) -> EpisodeStatus {
    match job.unwrap_or_default() {
        "completed" => EpisodeStatus::Completed,
        "failed" | "error" => EpisodeStatus::Failed,
        "running" | "processing" => EpisodeStatus::Voicing,
        "pending" | "submitted" => EpisodeStatus::Queued,
        _ if has_audio => EpisodeStatus::Completed,
        _ => EpisodeStatus::Queued,
    }
}

/// "two_hosts-casual" reads "Two hosts casual".
fn voice_label(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_run = false;
    for c in name.chars() {
        if c == '_' || c == '-' {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    // `/^\w/` only matches an ASCII word character.
    if let Some(first) = out.chars().next().filter(|c| c.is_ascii_alphanumeric()) {
        out.replace_range(..1, &first.to_ascii_uppercase().to_string());
    }
    out
}

impl From<PodcastEpisode> for Episode {
    fn from(e: PodcastEpisode) -> Self {
        let has_audio = e.audio_file.as_deref().is_some_and(|a| !a.is_empty());
        Episode {
            id: e.id,
            notebook_id: None,
            name: e.name,
            summary: None,
            status: server_status(e.job_status.as_deref(), has_audio),
            progress: None,
            error: e.error_message.filter(|m| !m.is_empty()),
            created: e.created.unwrap_or_default(),
            duration: None,
            chapters: Vec::new(),
            transcript: e
                .transcript
                .and_then(|t| t.transcript)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|l| {
                    let text = l.dialogue?.trim().to_string();
                    (!text.is_empty()).then(|| EpisodeLine { speaker: l.speaker.unwrap_or_default(), text, start: None, end: None })
                })
                .collect(),
            exact_timing: false,
            voice_label: e.speaker_profile.and_then(|p| p.name).map(|n| voice_label(&n)),
            effort: None,
            check: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerEpisodeProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub speaker_config: Option<String>,
    #[serde(default)]
    pub speaker_config_name: Option<String>,
    #[serde(default)]
    pub outline_llm: Option<String>,
    #[serde(default)]
    pub transcript_llm: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub default_briefing: Option<String>,
    #[serde(default)]
    pub num_segments: u32,
    #[serde(default)]
    pub max_tokens: Option<u64>,
}

impl From<ServerEpisodeProfile> for EpisodeProfile {
    fn from(p: ServerEpisodeProfile) -> Self {
        EpisodeProfile {
            id: p.id,
            name: p.name,
            description: p.description.unwrap_or_default(),
            speaker_profile_id: p.speaker_config.unwrap_or_default(),
            language: p.language,
            briefing: p.default_briefing.unwrap_or_default(),
            segments: p.num_segments,
        }
    }
}

/// The save body; the server's outline and transcript models are kept from `existing`.
pub fn episode_profile_body(p: &EpisodeProfile, existing: Option<&ServerEpisodeProfile>) -> Value {
    let mut body = json!({
        "name": p.name,
        "description": p.description,
        "speaker_config": p.speaker_profile_id,
        "language": p.language,
        "default_briefing": p.briefing,
        "num_segments": p.segments,
    });
    if let Some(e) = existing {
        body["outline_llm"] = json!(e.outline_llm);
        body["transcript_llm"] = json!(e.transcript_llm);
        body["max_tokens"] = json!(e.max_tokens);
    }
    body
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerSpeaker {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub voice_id: String,
    #[serde(default)]
    pub backstory: Option<String>,
    #[serde(default)]
    pub personality: Option<String>,
    #[serde(default)]
    pub voice_model: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerSpeakerProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub voice_model: Option<String>,
    #[serde(default)]
    pub speakers: Vec<ServerSpeaker>,
}

impl From<ServerSpeakerProfile> for SpeakerProfile {
    fn from(p: ServerSpeakerProfile) -> Self {
        SpeakerProfile {
            id: p.id,
            name: p.name,
            description: p.description.unwrap_or_default(),
            voice_kind: None,
            voice_model: p.voice_model.unwrap_or_default(),
            speakers: p
                .speakers
                .into_iter()
                .map(|s| Speaker { name: s.name, voice_id: s.voice_id, backstory: s.backstory.unwrap_or_default(), personality: s.personality.unwrap_or_default() })
                .collect(),
        }
    }
}

pub fn speaker_profile_body(p: &SpeakerProfile) -> Value {
    json!({
        "name": p.name,
        "description": p.description,
        "voice_model": if p.voice_model.is_empty() { Value::Null } else { json!(p.voice_model) },
        "speakers": p.speakers.iter().map(|s| json!({
            "name": s.name, "voice_id": s.voice_id, "backstory": s.backstory, "personality": s.personality,
        })).collect::<Vec<_>>(),
    })
}

#[derive(Clone, Debug, Deserialize)]
pub struct PodcastGenerationResponse {
    #[serde(default)]
    pub job_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: String,
}

// Server admin screens

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AuthStatus {
    pub auth_enabled: bool,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ServerCredential {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub modalities: Vec<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub has_api_key: bool,
    #[serde(default)]
    pub model_count: Option<u32>,
    #[serde(default)]
    pub decryption_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewCredential {
    pub name: String,
    pub provider: String,
    pub modalities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ServerJob {
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

/// A credential or model test.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct TestResult {
    #[serde(default)]
    pub success: Option<bool>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct RebuildStarted {
    #[serde(default)]
    pub command_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ModelProviders {
    #[serde(default)]
    pub available: Vec<String>,
    #[serde(default)]
    pub supported_types: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Language {
    pub code: String,
    pub name: String,
}

// Chat context

/// The server's names for how much of an item goes into chat.
pub const FULL: &str = "full content";
pub const INSIGHTS: &str = "insights";
pub const NOT_IN: &str = "not in";

/// The server's context config: the app's choices over the web app's defaults
/// (a source's insights when it has some, its full text otherwise; every note).
pub fn server_context(sources: &[SourceSummary], notes: &[Note], selection: Option<&ContextSelection>) -> Value {
    let mut src: BTreeMap<&str, &str> = sources.iter().map(|s| (s.id.as_str(), if s.insights_count > 0 { INSIGHTS } else { FULL })).collect();
    let mut nts: BTreeMap<&str, &str> = notes.iter().map(|n| (n.id.as_str(), FULL)).collect();
    if let Some(sel) = selection {
        for (id, mode) in &sel.sources {
            if let Some(slot) = src.get_mut(id.as_str()) {
                *slot = match mode {
                    SourceContext::Full => FULL,
                    SourceContext::Insights => INSIGHTS,
                    SourceContext::Off => NOT_IN,
                };
            }
        }
        for (id, on) in &sel.notes {
            if let Some(slot) = nts.get_mut(id.as_str()) {
                *slot = if *on { FULL } else { NOT_IN };
            }
        }
    }
    json!({ "sources": src, "notes": nts })
}

// Study decks kept as notes

pub const STUDY_PREFIX: &str = "<!-- open-notebook-study:v1 -->\n";

/// Decks stored as notes; a damaged deck note is skipped, like on the server.
pub fn decks_from_notes(notes: &[Note]) -> Vec<StoredDeck> {
    notes
        .iter()
        .filter_map(|n| {
            let body = n.content.as_deref()?.strip_prefix(STUDY_PREFIX)?;
            let deck: StudyDeck = serde_json::from_str(body).ok()?;
            Some(StoredDeck { deck, note_id: n.id.clone(), created: Some(n.created.clone()) })
        })
        .collect()
}

/// Citation ids in an answer, `[source:…]` or `[note:…]`, unique and in order, at most 12.
pub fn cited_ids(answer: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = answer;
    while let Some(open) = rest.find('[') {
        rest = &rest[open + 1..];
        let body_len = rest.find(|c: char| c == ']' || c.is_whitespace()).unwrap_or(rest.len());
        if !rest[body_len..].starts_with(']') {
            continue;
        }
        let id = &rest[..body_len];
        let ok = ["source:", "note:"].iter().any(|p| id.len() > p.len() && id.starts_with(p));
        if ok && !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    out.truncate(12);
    out
}

/// The first line of a message, shortened to `max` characters with an ellipsis.
pub fn title_from(text: &str, max: usize) -> String {
    let line = text.trim().split('\n').next().unwrap_or_default().trim();
    if line.chars().count() > max {
        let cut: String = line.chars().take(max - 1).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.to_string()
    }
}

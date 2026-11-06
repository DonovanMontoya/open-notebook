//! Data shapes shared by the UI and both backends. They follow the server API
//! (`packages/api-client/src/types.ts`) and the TypeScript engine
//! (`packages/engine/src/backend.ts`), so JSON from either side reads the same.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Notebook {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub archived: bool,
    pub created: String,
    pub updated: String,
    #[serde(default)]
    pub source_count: u32,
    #[serde(default)]
    pub note_count: u32,
}

#[derive(Clone, Debug, Default)]
pub struct NotebookUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub archived: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: Option<String>,
    pub content: Option<String>,
    /// "human" or "ai".
    pub note_type: Option<String>,
    pub created: String,
    pub updated: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceAsset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceSummary {
    pub id: String,
    pub title: Option<String>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub asset: Option<SourceAsset>,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default)]
    pub embedded_chunks: u32,
    #[serde(default)]
    pub insights_count: u32,
    pub created: String,
    pub updated: String,
    #[serde(default)]
    pub file_available: bool,
    /// "new", "queued", "running", "completed" or "failed"; absent means done.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// Notebook ids the source is in.
    #[serde(default)]
    pub notebooks: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceDetail {
    #[serde(flatten)]
    pub summary: SourceSummary,
    #[serde(default)]
    pub full_text: String,
}

/// What the user adds as a source.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceInput {
    Link { url: String, title: Option<String> },
    Text { content: String, title: Option<String> },
    /// A file read on device. `content` is its text (empty for audio and video, which are transcribed).
    Document { title: String, content: String, file_name: Option<String>, file: Option<Vec<u8>>, mime: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewSource {
    pub input: SourceInput,
    /// Transformation ids to run once the source is processed.
    pub transformations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceFile {
    pub data: Vec<u8>,
    pub name: String,
    pub mime: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub notebook_id: String,
    pub created: String,
    pub updated: String,
    #[serde(default)]
    pub message_count: Option<u32>,
    #[serde(default)]
    pub model_override: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Human,
    Ai,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: MessageKind,
    pub content: String,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatSessionWithMessages {
    #[serde(flatten)]
    pub session: ChatSession,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceContext {
    Full,
    Insights,
    Off,
}

/// How much of each source and note goes into chat. Missing entries use the defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextSelection {
    #[serde(default)]
    pub sources: BTreeMap<String, SourceContext>,
    /// false leaves the note out.
    #[serde(default)]
    pub notes: BTreeMap<String, bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HitKind {
    Source,
    Note,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: HitKind,
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    pub notebook_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Transformation {
    pub id: String,
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    /// Pre-selected when adding sources.
    #[serde(default)]
    pub apply_default: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Insight {
    pub id: String,
    pub source_id: String,
    /// The transformation title it came from, e.g. "Dense Summary".
    #[serde(rename = "type")]
    pub kind: String,
    pub content: String,
    pub created: Option<String>,
    /// "running", "completed" or "failed".
    pub status: Option<String>,
    pub error: Option<String>,
}

// Ask

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskSearch {
    pub term: String,
    pub instructions: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AskStep {
    Strategy { reasoning: String, searches: Vec<AskSearch> },
    Answer { term: String, answer: String },
    Final { answer: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AskDocument {
    pub id: String,
    pub title: String,
    pub notebook_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AskResult {
    pub answer: String,
    pub documents: Vec<AskDocument>,
}

// Study decks, stored as notes with the server's marker and JSON shape.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StudyKind {
    Flashcards,
    Quiz,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyItem {
    pub question: String,
    pub answer: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub correct_index: Option<usize>,
    #[serde(default)]
    pub source_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyDeck {
    pub version: u32,
    pub kind: StudyKind,
    pub title: String,
    pub items: Vec<StudyItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredDeck {
    pub deck: StudyDeck,
    pub note_id: String,
    pub created: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudyRequest {
    pub kind: StudyKind,
    pub source_ids: Vec<String>,
    /// 1 to 20.
    pub count: u32,
    /// "easy", "medium" or "hard".
    pub difficulty: String,
    pub language: Option<String>,
    pub instructions: Option<String>,
}

// Podcasts

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpisodeStatus {
    Queued,
    Researching,
    Writing,
    Checking,
    Revising,
    Voicing,
    Saving,
    Completed,
    Failed,
}

/// How much checking goes into an episode before it is voiced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PodcastEffort {
    Quick,
    #[default]
    Standard,
    Careful,
    Thorough,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EpisodeLine {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EpisodeChapter {
    pub title: String,
    pub start: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EpisodePrompt {
    pub model: String,
    pub system: String,
    pub prompt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimVerdict {
    Supported,
    Overstated,
    Unsupported,
}

/// One factual claim from an episode script, as the claim check judged it.
/// Same shape as `CheckedClaim` in `packages/engine/src/podcast/check.ts`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckedClaim {
    /// 1-based number of the spoken line the claim comes from.
    pub line: usize,
    /// The spoken line, as written.
    #[serde(default)]
    pub said: String,
    /// The claim, restated briefly.
    pub claim: String,
    pub verdict: ClaimVerdict,
    /// The passage the checker gave as evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    /// The title of the source the quote was found in, when it was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Whether the quote was found in the material.
    #[serde(default)]
    pub quote_found: bool,
    /// Why the claim is overstated or unsupported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckCounts {
    pub supported: u32,
    pub overstated: u32,
    pub unsupported: u32,
}

/// How closely an episode's claims follow the notebook.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeCheck {
    /// "checking", "done" or "failed".
    pub status: String,
    pub model: String,
    #[serde(default)]
    pub writer: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub claims: Vec<CheckedClaim>,
    #[serde(default)]
    pub supported: u32,
    #[serde(default)]
    pub overstated: u32,
    #[serde(default)]
    pub unsupported: u32,
    #[serde(default)]
    pub sources_used: u32,
    #[serde(default)]
    pub sources_total: u32,
    #[serde(default)]
    pub history: Vec<CheckCounts>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub id: String,
    pub notebook_id: Option<String>,
    pub name: String,
    pub summary: Option<String>,
    pub status: EpisodeStatus,
    /// (done, total)
    pub progress: Option<(u32, u32)>,
    pub error: Option<String>,
    pub created: String,
    pub duration: Option<f64>,
    pub chapters: Vec<EpisodeChapter>,
    pub transcript: Vec<EpisodeLine>,
    pub exact_timing: bool,
    pub voice_label: Option<String>,
    pub effort: Option<PodcastEffort>,
    pub check: Option<EpisodeCheck>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateEpisode {
    pub notebook_id: String,
    pub episode_profile: String,
    pub speaker_profile: Option<String>,
    pub focus: Option<String>,
    pub name: Option<String>,
    pub effort: Option<PodcastEffort>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Speaker {
    pub name: String,
    /// Voice id for the provider (OpenAI voice name, Gemini voice, ElevenLabs voice id).
    /// Saved on device as `voice`, the Tauri app's key; the server calls it `voice_id`.
    #[serde(rename = "voice", alias = "voice_id")]
    pub voice_id: String,
    #[serde(default)]
    pub backstory: String,
    #[serde(default)]
    pub personality: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// On device: "openai", "openai-live", "gemini" or "elevenlabs". Absent for server profiles.
    #[serde(default)]
    pub voice_kind: Option<String>,
    pub voice_model: String,
    pub speakers: Vec<Speaker>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub speaker_profile_id: String,
    #[serde(default)]
    pub language: Option<String>,
    pub briefing: String,
    pub segments: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PodcastProfiles {
    pub episode: Vec<EpisodeProfile>,
    pub speaker: Vec<SpeakerProfile>,
}

/// A model registered on a server (connected mode).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    /// language, embedding, text_to_speech or speech_to_text.
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageGroup {
    pub model: String,
    pub feature: String,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub audio_seconds: f64,
    pub estimated_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    pub days: u32,
    pub calls: u64,
    pub estimated_usd: Option<f64>,
    pub groups: Vec<UsageGroup>,
    /// On device, token counts are estimated from text length.
    pub estimated: bool,
}

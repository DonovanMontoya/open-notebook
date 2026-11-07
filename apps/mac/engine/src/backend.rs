//! Everything the UI needs, implemented twice: on device (`LocalBackend`) and
//! against an Open Notebook server (`RemoteBackend`). Mirrors the `Backend`
//! interface in `packages/engine/src/backend.ts`.
//!
//! Calls block: the UI runs them on worker threads and wakes the window when
//! they return. Long calls take a [`Cancel`] and a callback for progress.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::*;

pub type Result<T> = std::result::Result<T, crate::Error>;

/// Set from the UI to stop a running request (Stop button, closing a view).
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    /// Err(Error::Cancelled) once cancelled, for `?` in loops.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() { Err(crate::Error::Cancelled) } else { Ok(()) }
    }
}

/// Receives text as a reply streams in.
pub type OnText<'a> = &'a mut (dyn FnMut(&str) + Send);

pub struct SendChat<'a> {
    pub notebook_id: String,
    /// None starts a new session titled from the message.
    pub session_id: Option<String>,
    pub message: String,
    pub context: Option<ContextSelection>,
    /// A server model id, or "provider::model" on device.
    pub model_override: Option<String>,
    pub on_text: Option<OnText<'a>>,
    pub cancel: Cancel,
}

pub struct SendSourceChat<'a> {
    pub source_id: String,
    pub session_id: Option<String>,
    pub message: String,
    pub model_override: Option<String>,
    pub on_text: Option<OnText<'a>>,
    pub cancel: Cancel,
}

#[derive(Clone, Debug)]
pub struct SendChatResult {
    pub session: ChatSession,
    pub messages: Vec<ChatMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Local,
    Remote,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PodcastMode {
    /// Writes and voices episodes on device.
    Local,
    /// Uses the server's podcast profiles and jobs.
    Server,
}

/// Where an episode's audio is.
#[derive(Clone, Debug, PartialEq)]
pub enum EpisodeAudio {
    File(std::path::PathBuf),
    /// Downloaded on demand; the request needs the server password, if any.
    Url {
        url: String,
        password: Option<String>,
    },
}

pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    /// Whether chat replies stream token by token.
    fn streams_chat(&self) -> bool;
    fn podcast_mode(&self) -> PodcastMode;

    fn list_notebooks(&self) -> Result<Vec<Notebook>>;
    fn get_notebook(&self, id: &str) -> Result<Notebook>;
    fn create_notebook(&self, name: &str, description: &str) -> Result<Notebook>;
    fn update_notebook(&self, id: &str, update: NotebookUpdate) -> Result<Notebook>;
    fn delete_notebook(&self, id: &str) -> Result<()>;

    fn list_sources(&self, notebook_id: &str) -> Result<Vec<SourceSummary>>;
    fn get_source(&self, id: &str) -> Result<SourceDetail>;
    /// Saves the source and starts processing it in the background; poll `list_sources` for status.
    fn add_source(&self, notebook_id: &str, source: NewSource) -> Result<SourceSummary>;
    /// All sources across notebooks, each with the notebook ids it is in.
    fn list_all_sources(&self) -> Result<Vec<SourceSummary>>;
    fn link_source(&self, source_id: &str, notebook_id: &str) -> Result<()>;
    fn unlink_source(&self, source_id: &str, notebook_id: &str) -> Result<()>;
    fn rename_source(&self, id: &str, title: &str) -> Result<SourceDetail>;
    /// The original file of an uploaded source, if there is one.
    fn download_source(&self, id: &str) -> Result<Option<SourceFile>>;
    fn retry_source(&self, id: &str) -> Result<()>;
    fn delete_source(&self, id: &str) -> Result<()>;

    fn list_notes(&self, notebook_id: &str) -> Result<Vec<Note>>;
    fn create_note(&self, notebook_id: &str, title: Option<&str>, content: &str, ai: bool) -> Result<Note>;
    fn update_note(&self, id: &str, title: Option<&str>, content: Option<&str>) -> Result<Note>;
    fn delete_note(&self, id: &str) -> Result<()>;

    fn list_chat_sessions(&self, notebook_id: &str) -> Result<Vec<ChatSession>>;
    fn get_chat_session(&self, id: &str) -> Result<ChatSessionWithMessages>;
    fn send_chat_message(&self, args: SendChat<'_>) -> Result<SendChatResult>;
    fn delete_chat_session(&self, id: &str) -> Result<()>;
    fn rename_chat_session(&self, id: &str, title: &str) -> Result<()>;

    fn list_source_chat_sessions(&self, source_id: &str) -> Result<Vec<ChatSession>>;
    fn get_source_chat_session(&self, source_id: &str, session_id: &str) -> Result<ChatSessionWithMessages>;
    fn send_source_chat_message(&self, args: SendSourceChat<'_>) -> Result<SendChatResult>;
    fn delete_source_chat_session(&self, source_id: &str, session_id: &str) -> Result<()>;

    fn search(&self, query: &str, notebook_id: Option<&str>) -> Result<Vec<SearchHit>>;
    /// Researches a question across all notebooks (or one) and answers with citations.
    fn ask(&self, question: &str, notebook_id: Option<&str>, on_step: &mut (dyn FnMut(AskStep) + Send), cancel: &Cancel) -> Result<AskResult>;

    fn generate_study_deck(&self, notebook_id: &str, request: StudyRequest) -> Result<StudyDeck>;
    fn list_study_decks(&self, notebook_id: &str) -> Result<Vec<StoredDeck>>;

    fn list_transformations(&self) -> Result<Vec<Transformation>>;
    /// Creates the transformation when `id` is empty.
    fn save_transformation(&self, t: Transformation) -> Result<Transformation>;
    fn delete_transformation(&self, id: &str) -> Result<()>;
    fn run_transformation(&self, transformation_id: &str, text: &str, cancel: &Cancel) -> Result<String>;
    fn transformation_instructions(&self) -> Result<String>;
    fn set_transformation_instructions(&self, text: &str) -> Result<()>;

    fn list_insights(&self, source_id: &str) -> Result<Vec<Insight>>;
    /// Starts a transformation on a source; the insight appears in `list_insights`.
    fn create_insight(&self, source_id: &str, transformation_id: &str) -> Result<()>;
    fn delete_insight(&self, id: &str) -> Result<()>;
    fn save_insight_as_note(&self, insight_id: &str, notebook_id: &str) -> Result<Note>;

    fn usage(&self, days: u32) -> Result<UsageReport>;
    /// Re-embeds every source with the current embedding model. Returns how many were queued.
    fn rebuild_index(&self) -> Result<u32>;
    /// Models registered on the server; empty on device.
    fn list_models(&self) -> Result<Vec<ServerModel>>;

    fn list_episodes(&self, notebook_id: &str) -> Result<Vec<Episode>>;
    fn get_episode(&self, id: &str) -> Result<Episode>;
    /// Starts an episode in the background.
    fn create_episode(&self, input: CreateEpisode) -> Result<Episode>;
    fn delete_episode(&self, id: &str) -> Result<()>;
    fn episode_audio(&self, id: &str) -> Result<EpisodeAudio>;
    fn episode_prompt(&self, id: &str) -> Result<Option<EpisodePrompt>>;
    fn check_episode(&self, id: &str) -> Result<()>;
    fn retry_episode(&self, id: &str) -> Result<()>;
    fn podcast_profiles(&self) -> Result<PodcastProfiles>;
    fn save_episode_profile(&self, p: EpisodeProfile) -> Result<EpisodeProfile>;
    fn delete_episode_profile(&self, id: &str) -> Result<()>;
    fn save_speaker_profile(&self, p: SpeakerProfile) -> Result<SpeakerProfile>;
    fn delete_speaker_profile(&self, id: &str) -> Result<()>;
    /// The on-device engine, for features only it has (live talk).
    fn local(&self) -> Option<&crate::local::LocalBackend> {
        None
    }
}

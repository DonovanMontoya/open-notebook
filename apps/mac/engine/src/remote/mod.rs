//! Connected mode: the same [`Backend`] surface, served by an Open Notebook
//! server. Port of `packages/engine/src/remote.ts` over `packages/api-client`.

mod api;
mod client;
mod wire;

#[cfg(test)]
mod tests;

pub use api::Upload;
pub use client::{Body, Client, DEFAULT_TIMEOUT, Download, error_message, normalize_base_url};
pub use wire::{
    AuthStatus, Language, ModelProviders, NewCredential, RebuildStarted, ServerCredential, ServerJob, TestResult, cited_ids, title_from,
};

use serde_json::{Map, json};

use crate::Error;
use crate::backend::*;
use crate::net::{SharedHttp, UreqHttp};
use crate::types::*;
use wire::*;

pub struct RemoteBackend {
    client: Client,
}

impl RemoteBackend {
    /// `password` is the server's OPEN_NOTEBOOK_PASSWORD, if it has one (`Secrets` keeps it as `SERVER_PASSWORD`).
    pub fn new(server_url: &str, password: Option<&str>) -> Self {
        Self::with_http(server_url, password, UreqHttp::shared())
    }

    /// The same, with another HTTP layer (tests).
    pub fn with_http(server_url: &str, password: Option<&str>, http: SharedHttp) -> Self {
        RemoteBackend { client: Client::new(server_url, password, http) }
    }

    /// Every server endpoint, for the server settings screens (credentials, models, jobs).
    pub fn client(&self) -> &Client {
        &self.client
    }

    fn session_with_messages(s: ServerChatSession) -> SendChatResult {
        let full = s.with_messages();
        SendChatResult { messages: full.messages.clone(), session: full.session }
    }
}

impl Backend for RemoteBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Remote
    }

    /// The server answers notebook chat in one response.
    fn streams_chat(&self) -> bool {
        false
    }

    fn podcast_mode(&self) -> PodcastMode {
        PodcastMode::Server
    }

    fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        self.client.list_notebooks(None)
    }

    fn get_notebook(&self, id: &str) -> Result<Notebook> {
        self.client.get_notebook(id)
    }

    fn create_notebook(&self, name: &str, description: &str) -> Result<Notebook> {
        self.client.create_notebook(name, Some(description))
    }

    fn update_notebook(&self, id: &str, update: NotebookUpdate) -> Result<Notebook> {
        self.client.update_notebook(id, &update)
    }

    fn delete_notebook(&self, id: &str) -> Result<()> {
        self.client.delete_notebook(id)
    }

    fn list_sources(&self, notebook_id: &str) -> Result<Vec<SourceSummary>> {
        Ok(self.client.list_sources(notebook_id)?.iter().map(ServerSource::summary).collect())
    }

    fn get_source(&self, id: &str) -> Result<SourceDetail> {
        Ok(self.client.get_source(id)?.detail())
    }

    fn add_source(&self, notebook_id: &str, source: NewSource) -> Result<SourceSummary> {
        let transformations = source.transformations;
        let (kind, body, title) = match source.input {
            // Files go to the server as they are, so it can read PDFs, Office files, audio and video itself.
            SourceInput::Document { title, file_name: Some(file_name), file: Some(data), mime, .. } => {
                let upload = Upload { data: &data, file_name: &file_name, mime: mime.as_deref().unwrap_or(""), title: Some(&title), transformations: &transformations };
                return Ok(self.client.upload_source(notebook_id, upload)?.summary());
            }
            SourceInput::Document { title, content, .. } => ("text", ("content", content), Some(title)),
            SourceInput::Text { content, title } => ("text", ("content", content), title),
            SourceInput::Link { url, title } => ("link", ("url", url), title),
        };
        let mut map = Map::new();
        map.insert("type".into(), json!(kind));
        map.insert(body.0.into(), json!(body.1));
        if let Some(t) = title {
            map.insert("title".into(), json!(t));
        }
        if !transformations.is_empty() {
            map.insert("transformations".into(), json!(transformations));
        }
        Ok(self.client.create_source(notebook_id, map)?.summary())
    }

    fn list_all_sources(&self) -> Result<Vec<SourceSummary>> {
        Ok(self.client.list_all_sources()?.iter().map(ServerSource::summary).collect())
    }

    fn link_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.client.link_source(source_id, notebook_id)
    }

    fn unlink_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.client.unlink_source(source_id, notebook_id)
    }

    fn rename_source(&self, id: &str, title: &str) -> Result<SourceDetail> {
        Ok(self.client.update_source(id, title)?.detail())
    }

    fn download_source(&self, id: &str) -> Result<Option<SourceFile>> {
        match self.client.download_source(id) {
            Ok(file) => Ok(Some(SourceFile { data: file.data, name: file.name.unwrap_or_else(|| "source".into()), mime: file.mime })),
            Err(Error::Server { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn retry_source(&self, id: &str) -> Result<()> {
        self.client.retry_source(id)
    }

    fn delete_source(&self, id: &str) -> Result<()> {
        self.client.delete_source(id)
    }

    fn list_notes(&self, notebook_id: &str) -> Result<Vec<Note>> {
        self.client.list_notes(notebook_id)
    }

    fn create_note(&self, notebook_id: &str, title: Option<&str>, content: &str, ai: bool) -> Result<Note> {
        self.client.create_note(notebook_id, title, content, Some(if ai { "ai" } else { "human" }))
    }

    fn update_note(&self, id: &str, title: Option<&str>, content: Option<&str>) -> Result<Note> {
        self.client.update_note(id, title, content)
    }

    fn delete_note(&self, id: &str) -> Result<()> {
        self.client.delete_note(id)
    }

    fn list_chat_sessions(&self, notebook_id: &str) -> Result<Vec<ChatSession>> {
        Ok(self.client.list_chat_sessions(notebook_id)?.iter().map(ServerChatSession::session).collect())
    }

    fn get_chat_session(&self, id: &str) -> Result<ChatSessionWithMessages> {
        Ok(self.client.get_chat_session(id)?.with_messages())
    }

    /// The server answers in one response; the full reply is passed to `on_text` once.
    fn send_chat_message(&self, mut args: SendChat<'_>) -> Result<SendChatResult> {
        let c = &self.client;
        args.cancel.check()?;
        let session_id = match args.session_id.take() {
            Some(id) => id,
            None => c.create_chat_session(&args.notebook_id, Some(&title_from(&args.message, 40)))?.id,
        };
        let (sources, notes) = std::thread::scope(|s| {
            let sources = s.spawn(|| c.list_sources(&args.notebook_id));
            let notes = c.list_notes(&args.notebook_id);
            (sources.join().unwrap_or_else(|_| Err(Error::other("Listing sources failed"))), notes)
        });
        let sources: Vec<SourceSummary> = sources?.iter().map(ServerSource::summary).collect();
        let config = server_context(&sources, &notes?, args.context.as_ref());
        args.cancel.check()?;
        let context = c.build_context(&args.notebook_id, config)?.context;
        args.cancel.check()?;
        let result = c.execute_chat(&session_id, &args.message, context, args.model_override.as_deref())?;
        if let Some(reply) = result.messages.iter().rev().find(|m| m.kind == "ai")
            && let Some(on_text) = args.on_text.as_mut()
        {
            on_text(&reply.content);
        }
        Ok(Self::session_with_messages(c.get_chat_session(&session_id)?))
    }

    fn delete_chat_session(&self, id: &str) -> Result<()> {
        self.client.delete_chat_session(id)
    }

    fn rename_chat_session(&self, id: &str, title: &str) -> Result<()> {
        self.client.rename_chat_session(id, title)
    }

    fn list_source_chat_sessions(&self, source_id: &str) -> Result<Vec<ChatSession>> {
        Ok(self.client.list_source_chat_sessions(source_id)?.iter().map(ServerChatSession::session).collect())
    }

    fn get_source_chat_session(&self, source_id: &str, session_id: &str) -> Result<ChatSessionWithMessages> {
        Ok(self.client.get_source_chat_session(source_id, session_id)?.with_messages())
    }

    /// Source chat streams: each chunk of the server's event stream goes to `on_text` as it arrives.
    fn send_source_chat_message(&self, mut args: SendSourceChat<'_>) -> Result<SendChatResult> {
        let c = &self.client;
        args.cancel.check()?;
        let session_id = match args.session_id.take() {
            Some(id) => id,
            None => c.create_source_chat_session(&args.source_id, Some(&title_from(&args.message, 40)))?.id,
        };
        let mut on_text = args.on_text.take();
        c.send_source_chat(&args.source_id, &session_id, &args.message, args.model_override.as_deref(), &args.cancel, &mut |chunk| {
            if let Some(f) = on_text.as_mut() {
                f(chunk);
            }
        })?;
        Ok(Self::session_with_messages(c.get_source_chat_session(&args.source_id, &session_id)?))
    }

    fn delete_source_chat_session(&self, source_id: &str, session_id: &str) -> Result<()> {
        self.client.delete_source_chat_session(source_id, session_id)
    }

    fn search(&self, query: &str, notebook_id: Option<&str>) -> Result<Vec<SearchHit>> {
        Ok(self.client.search(query, notebook_id, false, None)?.results.into_iter().map(SearchHit::from).collect())
    }

    /// The server's Ask, with its default chat model for every step.
    fn ask(&self, question: &str, notebook_id: Option<&str>, on_step: &mut (dyn FnMut(AskStep) + Send), cancel: &Cancel) -> Result<AskResult> {
        let c = &self.client;
        let defaults = c.model_defaults()?;
        let model = defaults
            .get("default_chat_model")
            .cloned()
            .flatten()
            .filter(|m| !m.is_empty())
            .ok_or_else(|| Error::Setup("The server has no default chat model. Set one in its Models settings.".into()))?;
        cancel.check()?;
        let answer = c.ask(question, &model, &model, &model, notebook_id)?;
        cancel.check()?;
        on_step(AskStep::Final { answer: answer.clone() });
        let ids = cited_ids(&answer);
        let documents = std::thread::scope(|s| {
            let handles: Vec<_> = ids.iter().map(|id| s.spawn(move || cited_document(c, id))).collect();
            handles.into_iter().zip(&ids).map(|(h, id)| h.join().unwrap_or_else(|_| AskDocument { id: id.clone(), title: id.clone(), notebook_id: None })).collect()
        });
        Ok(AskResult { answer, documents })
    }

    fn generate_study_deck(&self, notebook_id: &str, request: StudyRequest) -> Result<StudyDeck> {
        self.client.generate_study_deck(notebook_id, &request)
    }

    /// The server keeps decks as notes too; reading them from notes keeps their note ids.
    fn list_study_decks(&self, notebook_id: &str) -> Result<Vec<StoredDeck>> {
        Ok(decks_from_notes(&self.client.list_notes(notebook_id)?))
    }

    fn list_transformations(&self) -> Result<Vec<Transformation>> {
        Ok(self.client.list_transformations()?.into_iter().map(Transformation::from).collect())
    }

    fn save_transformation(&self, t: Transformation) -> Result<Transformation> {
        let body = transformation_body(&t);
        let saved = if t.id.is_empty() { self.client.create_transformation(body)? } else { self.client.update_transformation(&t.id, body)? };
        Ok(saved.into())
    }

    fn delete_transformation(&self, id: &str) -> Result<()> {
        self.client.delete_transformation(id)
    }

    fn run_transformation(&self, transformation_id: &str, text: &str, cancel: &Cancel) -> Result<String> {
        cancel.check()?;
        let out = self.client.execute_transformation(transformation_id, text)?;
        cancel.check()?;
        Ok(out)
    }

    fn transformation_instructions(&self) -> Result<String> {
        self.client.transformation_instructions()
    }

    fn set_transformation_instructions(&self, text: &str) -> Result<()> {
        self.client.set_transformation_instructions(text)
    }

    fn list_insights(&self, source_id: &str) -> Result<Vec<Insight>> {
        Ok(self.client.source_insights(source_id)?.into_iter().map(Insight::from).collect())
    }

    fn create_insight(&self, source_id: &str, transformation_id: &str) -> Result<()> {
        self.client.create_insight(source_id, transformation_id)
    }

    fn delete_insight(&self, id: &str) -> Result<()> {
        self.client.delete_insight(id)
    }

    fn save_insight_as_note(&self, insight_id: &str, notebook_id: &str) -> Result<Note> {
        self.client.save_insight_as_note(insight_id, notebook_id)
    }

    fn usage(&self, days: u32) -> Result<UsageReport> {
        Ok(self.client.usage(days)?.into())
    }

    /// Starts a full rebuild on the server. It doesn't say how many items it queued, so this returns 0.
    fn rebuild_index(&self) -> Result<u32> {
        self.client.rebuild_embeddings(true)?;
        Ok(0)
    }

    fn list_models(&self) -> Result<Vec<ServerModel>> {
        self.client.list_models()
    }

    /// The server keeps one episode list for all notebooks.
    fn list_episodes(&self, _notebook_id: &str) -> Result<Vec<Episode>> {
        Ok(self.client.list_podcast_episodes()?.into_iter().map(Episode::from).collect())
    }

    fn get_episode(&self, id: &str) -> Result<Episode> {
        Ok(self.client.get_podcast_episode(id)?.into())
    }

    /// The server generates by profile name. The episode record appears in the list once
    /// the server picks up the job; until then this returns a placeholder with a `job:` id.
    fn create_episode(&self, input: CreateEpisode) -> Result<Episode> {
        let c = &self.client;
        let (episodes, speakers) = std::thread::scope(|s| {
            let speakers = s.spawn(|| c.speaker_profiles());
            (c.episode_profiles(), speakers.join().unwrap_or_else(|_| Err(Error::other("Listing speaker profiles failed"))))
        });
        let (episodes, speakers) = (episodes?, speakers?);
        let ep = episodes.iter().find(|p| p.id == input.episode_profile).ok_or_else(|| Error::other("Choose an episode profile."))?;
        let wanted = input.speaker_profile.as_deref().filter(|s| !s.is_empty()).or(ep.speaker_config.as_deref());
        let sp = speakers.iter().find(|p| Some(p.id.as_str()) == wanted).ok_or_else(|| Error::other("Choose a speaker profile."))?;
        let name = match input.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => format!("{} episode", c.get_notebook(&input.notebook_id)?.name),
        };
        let focus = input.focus.as_deref().map(str::trim).filter(|f| !f.is_empty());
        let job = c.generate_podcast(json!({
            "episode_profile": ep.name,
            "speaker_profile": sp.name,
            "episode_name": name,
            "notebook_id": input.notebook_id,
            "briefing_suffix": focus,
        }))?;
        Ok(Episode {
            id: format!("job:{}", job.job_id),
            notebook_id: None,
            name,
            summary: None,
            status: EpisodeStatus::Queued,
            progress: None,
            error: None,
            created: now_iso(),
            duration: None,
            chapters: Vec::new(),
            transcript: Vec::new(),
            exact_timing: false,
            voice_label: None,
            effort: None,
            check: None,
        })
    }

    fn delete_episode(&self, id: &str) -> Result<()> {
        self.client.delete_podcast_episode(id)
    }

    /// A short-lived URL that streams without the password; it is passed along anyway.
    fn episode_audio(&self, id: &str) -> Result<EpisodeAudio> {
        let url = self.client.episode_audio_url(id)?;
        Ok(EpisodeAudio::Url { url, password: self.client.password().map(str::to_string) })
    }

    /// The server does not expose the prompt it used.
    fn episode_prompt(&self, _id: &str) -> Result<Option<EpisodePrompt>> {
        Ok(None)
    }

    fn check_episode(&self, _id: &str) -> Result<()> {
        Err(Error::other("Claim checks run on episodes made on this device."))
    }

    fn retry_episode(&self, id: &str) -> Result<()> {
        self.client.retry_podcast_episode(id)
    }

    fn podcast_profiles(&self) -> Result<PodcastProfiles> {
        let c = &self.client;
        let (episode, speaker) = std::thread::scope(|s| {
            let speakers = s.spawn(|| c.speaker_profiles());
            (c.episode_profiles(), speakers.join().unwrap_or_else(|_| Err(Error::other("Listing speaker profiles failed"))))
        });
        Ok(PodcastProfiles {
            episode: episode?.into_iter().map(EpisodeProfile::from).collect(),
            speaker: speaker?.into_iter().map(SpeakerProfile::from).collect(),
        })
    }

    /// Keeps the server's outline and transcript models when editing.
    fn save_episode_profile(&self, p: EpisodeProfile) -> Result<EpisodeProfile> {
        let id = Some(p.id.as_str()).filter(|i| !i.is_empty());
        let existing = match id {
            Some(id) => self.client.episode_profiles()?.into_iter().find(|e| e.id == id),
            None => None,
        };
        let saved = self.client.save_episode_profile(episode_profile_body(&p, existing.as_ref()), id)?;
        Ok(EpisodeProfile { id: saved.id, ..p })
    }

    fn delete_episode_profile(&self, id: &str) -> Result<()> {
        self.client.delete_episode_profile(id)
    }

    fn save_speaker_profile(&self, p: SpeakerProfile) -> Result<SpeakerProfile> {
        let id = Some(p.id.as_str()).filter(|i| !i.is_empty());
        let saved = self.client.save_speaker_profile(speaker_profile_body(&p), id)?;
        Ok(SpeakerProfile { id: saved.id, ..p })
    }

    fn delete_speaker_profile(&self, id: &str) -> Result<()> {
        self.client.delete_speaker_profile(id)
    }
}

/// The title of a cited source or note; the id itself when it can't be read.
fn cited_document(c: &Client, id: &str) -> AskDocument {
    let found = if id.starts_with("note:") {
        c.get_note(id).map(|n| AskDocument { id: id.into(), title: n.title.unwrap_or_else(|| "Note".into()), notebook_id: None })
    } else {
        c.get_source(id).map(|s| AskDocument {
            id: id.into(),
            title: s.title.unwrap_or_else(|| "Source".into()),
            notebook_id: s.notebooks.and_then(|n| n.into_iter().next()),
        })
    };
    found.unwrap_or_else(|_| AskDocument { id: id.into(), title: id.into(), notebook_id: None })
}

/// Now, as JavaScript's `toISOString()` writes it.
fn now_iso() -> String {
    let now = time::OffsetDateTime::now_utc();
    let format = time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");
    now.format(&format).unwrap_or_default()
}

//! App state and the work queue. Backend calls block, so they run on worker threads; each
//! sends back a closure that applies its result to the state, and the waker asks Rune for a
//! frame, whose `poll` runs the closures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use notebook_engine::local::LocalBackend;
use notebook_engine::providers::{ModelInfo, ProviderKind};
use notebook_engine::remote::RemoteBackend;
use notebook_engine::settings::{self, AppSettings, Mode, Secrets};
use notebook_engine::types::*;
use notebook_engine::{Backend, Cancel};
use rune::{TextEdit, Waker};
use rune_kit::shell::PaneState;

pub type Apply = Box<dyn FnOnce(&mut App) + Send>;
/// What a confirmed Yes does.
pub type OnYes = Box<dyn Fn(&mut App)>;

/// A question waiting for an answer in the confirm dialog.
pub struct Confirm {
    pub question: String,
    /// The Yes button's word.
    pub verb: &'static str,
    pub yes: OnYes,
}

/// Sends results back to the UI thread from anywhere.
#[derive(Clone)]
pub struct Inbox {
    tx: Sender<Apply>,
    waker: Option<Waker>,
}

impl Inbox {
    pub fn send(&self, f: impl FnOnce(&mut App) + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
        if let Some(w) = &self.waker {
            w();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Home,
    Notebook(String),
    Settings,
    Ask,
    Library,
    Transformations,
    Usage,
    Server,
    Profiles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookTab {
    Chat,
    Notes,
    Study,
    Listen,
}

/// The add-source sheet.
#[derive(Default)]
pub struct AddSource {
    pub kind: AddKind,
    pub url: TextEdit,
    pub title: TextEdit,
    pub text: TextEdit,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddKind {
    #[default]
    Link,
    Text,
    File,
}

/// A note being written or edited.
pub struct NoteDraft {
    /// None for a new note.
    pub id: Option<String>,
    pub title: TextEdit,
    pub content: TextEdit,
}

/// The open notebook.
pub struct Open {
    pub id: String,
    pub notebook: Option<Notebook>,
    pub sources: Vec<SourceSummary>,
    pub notes: Vec<Note>,
    pub sessions: Vec<ChatSession>,
    pub session: Option<ChatSessionWithMessages>,
    pub tab: NotebookTab,
    pub draft: TextEdit,
    /// The question and reply while a message is in flight.
    pub pending: Option<(String, String)>,
    pub sending: Option<Cancel>,
    /// The source open in the reader, and the passage to light up (byte range).
    pub reading: Option<(SourceDetail, Option<(usize, usize)>)>,
    pub add: Option<AddSource>,
    pub note: Option<NoteDraft>,
    pub sessions_open: bool,
    pub error: Option<String>,
    /// Chat about one source (id, title) instead of the whole notebook.
    pub scope: Option<(String, String)>,
    /// Insights of the source being read.
    pub insights: Vec<Insight>,
    pub insight_picker: bool,
    pub transformations: Vec<Transformation>,
    pub decks: Vec<StoredDeck>,
    pub study: Option<crate::study::StudyForm>,
    pub viewing: Option<crate::study::DeckView>,
    pub episodes: Vec<Episode>,
    pub new_episode: Option<crate::listen::NewEpisode>,
    pub episode_views: std::collections::HashMap<String, crate::listen::EpisodeOpen>,
}

impl Open {
    fn new(id: String) -> Open {
        Open {
            id,
            notebook: None,
            sources: Vec::new(),
            notes: Vec::new(),
            sessions: Vec::new(),
            session: None,
            tab: NotebookTab::Chat,
            draft: TextEdit::new(""),
            pending: None,
            sending: None,
            reading: None,
            add: None,
            note: None,
            sessions_open: false,
            error: None,
            scope: None,
            insights: Vec::new(),
            insight_picker: false,
            transformations: Vec::new(),
            decks: Vec::new(),
            study: None,
            viewing: None,
            episodes: Vec::new(),
            new_episode: None,
            episode_views: std::collections::HashMap::new(),
        }
    }

    /// Sources still being read, or insights being written, which `poll` refreshes until they finish.
    pub fn processing(&self) -> bool {
        self.sources.iter().any(|s| matches!(s.status.as_deref(), Some("new" | "queued" | "running" | "processing")))
            || self.insights.iter().any(|i| i.status.as_deref() == Some("running"))
            || self.episodes_in_progress()
    }

    /// Episodes still being made, or whose claims are still being checked.
    pub fn episodes_in_progress(&self) -> bool {
        self.episodes
            .iter()
            .any(|e| !matches!(e.status, EpisodeStatus::Completed | EpisodeStatus::Failed) || e.check.as_ref().is_some_and(|c| c.status == "checking"))
    }
}

/// A key or server check: running, worked (with what it found) or failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    Checking,
    Working(String),
    Failed(String),
}

/// A provider's model list for the pickers.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelList {
    Loading,
    Ready(Vec<ModelInfo>),
    Failed(String),
}

/// The model jobs on device, in the order Settings shows them.
pub const MODEL_ROLES: [&str; 4] = ["chat", "transformation", "embedding", "transcription"];

/// Providers that can do a job: those with a saved key, and OpenAI-compatible, which local
/// servers run without one. Anthropic has no embeddings or speech to text.
pub fn providers_for(role: &str, saved: impl Fn(ProviderKind) -> bool) -> Vec<ProviderKind> {
    ProviderKind::ALL
        .into_iter()
        .filter(|&k| saved(k) || k == ProviderKind::OpenAiCompatible)
        .filter(|&k| !(k == ProviderKind::Anthropic && matches!(role, "embedding" | "transcription")))
        .collect()
}

/// A provider's models that fit a job.
pub fn models_for<'a>(role: &str, kind: ProviderKind, models: &'a [ModelInfo]) -> Vec<&'a ModelInfo> {
    use notebook_engine::providers::Capability;
    models
        .iter()
        .filter(|m| match role {
            "transcription" if kind == ProviderKind::Gemini => m.capabilities.contains(&Capability::Chat),
            "transcription" => {
                let id = m.id.to_lowercase();
                id.contains("transcri") || id.contains("whisper")
            }
            "embedding" => m.capabilities.is_empty() || m.capabilities.contains(&Capability::Embedding),
            _ => m.capabilities.is_empty() || m.capabilities.contains(&Capability::Chat),
        })
        .collect()
}

/// The settings screen's fields. Key fields start empty: a saved key is never shown, and
/// pasting a new one replaces it.
pub struct SettingsForm {
    pub server_url: TextEdit,
    pub server_password: TextEdit,
    pub keys: Vec<(ProviderKind, TextEdit)>,
    pub compatible_base_url: TextEdit,
    pub chat_model: TextEdit,
    pub chat_provider: ProviderKind,
    pub embedding_model: TextEdit,
    pub embedding_provider: ProviderKind,
    pub transcription_model: TextEdit,
    pub transcription_provider: ProviderKind,
    /// ElevenLabs voices for podcasts; not a chat provider.
    pub elevenlabs: TextEdit,
    pub saved: bool,
    pub open_select: Option<&'static str>,
    /// Transformations, insights and study decks; empty uses the chat model.
    pub transformation_model: TextEdit,
    pub transformation_provider: ProviderKind,
    /// Secret names that have a saved value.
    pub saved_keys: BTreeSet<String>,
    /// Key and server checks, by provider id, "elevenlabs" or "server".
    pub checks: BTreeMap<String, Check>,
    pub models: BTreeMap<ProviderKind, ModelList>,
    /// The model pickers' search and open state, by role.
    pub pickers: BTreeMap<&'static str, rune_kit::ComboboxState>,
    pub syncing: bool,
    /// What the last iCloud Keychain switch did.
    pub sync: Option<Result<usize, settings::SyncError>>,
    pub rebuild: Option<Check>,
}

impl SettingsForm {
    pub fn from(settings: &AppSettings, secrets: &Secrets) -> SettingsForm {
        let model = |m: &Option<settings::ModelSetting>| m.as_ref().map(|m| m.model.clone()).unwrap_or_default();
        let provider = |m: &Option<settings::ModelSetting>, d| m.as_ref().and_then(|m| ProviderKind::parse(&m.provider)).unwrap_or(d);
        let saved_keys = settings::all_secret_names().into_iter().filter(|n| secrets.get(n).ok().flatten().is_some()).collect();
        let mut f = SettingsForm {
            server_url: TextEdit::new(settings.server_url.clone()),
            server_password: TextEdit::new(""),
            keys: ProviderKind::ALL.iter().map(|&p| (p, TextEdit::new(""))).collect(),
            compatible_base_url: TextEdit::new(settings.compatible_base_url.clone()),
            chat_model: TextEdit::new(model(&settings.chat)),
            chat_provider: provider(&settings.chat, ProviderKind::Anthropic),
            embedding_model: TextEdit::new(model(&settings.embedding)),
            embedding_provider: provider(&settings.embedding, ProviderKind::OpenAi),
            transcription_model: TextEdit::new(model(&settings.transcription)),
            transcription_provider: provider(&settings.transcription, ProviderKind::OpenAi),
            elevenlabs: TextEdit::new(""),
            saved: false,
            open_select: None,
            transformation_model: TextEdit::new(model(&settings.transformation)),
            transformation_provider: provider(&settings.transformation, ProviderKind::Anthropic),
            saved_keys,
            checks: BTreeMap::new(),
            models: BTreeMap::new(),
            pickers: BTreeMap::new(),
            syncing: false,
            sync: None,
            rebuild: None,
        };
        f.fix_providers();
        f
    }

    pub fn has_key(&self, name: &str) -> bool {
        self.saved_keys.contains(name)
    }

    pub fn has_provider_key(&self, kind: ProviderKind) -> bool {
        self.has_key(&settings::provider_secret(kind.as_str()))
    }

    /// A job's provider and model field.
    pub fn slot(&mut self, role: &str) -> (&mut ProviderKind, &mut TextEdit) {
        match role {
            "chat" => (&mut self.chat_provider, &mut self.chat_model),
            "transformation" => (&mut self.transformation_provider, &mut self.transformation_model),
            "embedding" => (&mut self.embedding_provider, &mut self.embedding_model),
            _ => (&mut self.transcription_provider, &mut self.transcription_model),
        }
    }

    pub fn provider_of(&self, role: &str) -> ProviderKind {
        match role {
            "chat" => self.chat_provider,
            "transformation" => self.transformation_provider,
            "embedding" => self.embedding_provider,
            _ => self.transcription_provider,
        }
    }

    /// Providers the job can use now.
    pub fn providers(&self, role: &str) -> Vec<ProviderKind> {
        providers_for(role, |k| self.has_provider_key(k))
    }

    /// A job with no model yet whose provider has no key moves to the first one that has.
    pub fn fix_providers(&mut self) {
        for role in MODEL_ROLES {
            let choices = self.providers(role);
            let (provider, model) = self.slot(role);
            if model.text.trim().is_empty()
                && !choices.contains(provider)
                && let Some(first) = choices.first()
            {
                *provider = *first;
            }
        }
    }

    /// Picks a provider for a job, filling in its usual model when the provider changes.
    pub fn choose_provider(&mut self, role: &str, kind: ProviderKind) {
        let (provider, model) = self.slot(role);
        if *provider != kind {
            *provider = kind;
            *model = TextEdit::new(settings::default_model(role, kind));
        }
    }
}

pub struct App {
    pub data_dir: PathBuf,
    pub settings: AppSettings,
    pub secrets: Secrets,
    pub backend: Option<Arc<dyn Backend>>,
    /// The same backend as `backend` in connected mode, for the admin calls only a server has.
    pub remote: Option<Arc<RemoteBackend>>,
    pub server: Option<crate::server::ServerPage>,
    pub playing: Option<crate::listen::NowPlaying>,
    pub podcast_profiles: Option<PodcastProfiles>,
    pub profiles: Option<crate::listen::ProfilesPage>,
    pub backend_error: Option<String>,
    pub route: Route,
    pub sidebar: PaneState,
    pub notebooks: Vec<Notebook>,
    pub loaded: bool,
    pub open: Option<Open>,
    pub new_notebook: Option<TextEdit>,
    pub form: Option<SettingsForm>,
    pub ask: Option<crate::pages::AskPage>,
    pub library: Option<crate::pages::LibraryPage>,
    pub transformations: Option<crate::pages::TransformationsPage>,
    pub usage: Option<crate::pages::UsagePage>,
    pub toast: Option<String>,
    /// When the toast appeared (app clock), so `poll` can clear it.
    pub toast_since: Option<f64>,
    /// A question waiting for Yes: what it asks, what Yes does and the Yes button's word.
    pub confirm: Option<Confirm>,
    /// Small remembered choices (playback rate, per-notebook chat context, last options), in
    /// `prefs.json` in the data folder; in memory for scenes and tests.
    pub prefs: rune::prefs::Prefs,
    /// When `poll` next refreshes sources that are still being read (app clock).
    pub next_refresh: Option<f64>,
    inbox: Inbox,
    rx: Receiver<Apply>,
    pub live: crate::live::LiveTalk,
}

impl App {
    /// A state with no backend yet, its settings and keys read from `data_dir`.
    fn empty(data_dir: PathBuf) -> App {
        let (tx, rx) = channel();
        let prefs = rune::prefs::Prefs::at(data_dir.join("prefs.json"));
        App {
            settings: AppSettings::load(&data_dir),
            secrets: Secrets::new(&data_dir),
            data_dir,
            backend: None,
            remote: None,
            server: None,
            playing: None,
            podcast_profiles: None,
            profiles: None,
            backend_error: None,
            route: Route::Home,
            sidebar: PaneState::new(248.0).range(180.0, 420.0),
            notebooks: Vec::new(),
            loaded: false,
            open: None,
            new_notebook: None,
            form: None,
            ask: None,
            library: None,
            transformations: None,
            usage: None,
            toast: None,
            toast_since: None,
            confirm: None,
            prefs,
            next_refresh: None,
            inbox: Inbox { tx, waker: None },
            rx,
            live: Default::default(),
        }
    }

    pub fn init() -> App {
        let mut app = App::empty(settings::data_dir());
        app.connect();
        app
    }

    /// A state with no backend, for headless scenes that fill in their own data.
    pub fn offline() -> App {
        let mut app = App::empty(std::env::temp_dir().join("open-notebook-scenes"));
        app.settings = AppSettings::default();
        app.prefs = rune::prefs::Prefs::memory();
        app.loaded = true;
        app
    }

    /// A state on a given backend (tests).
    #[cfg(test)]
    pub fn with_backend(data_dir: PathBuf, backend: Arc<dyn Backend>) -> App {
        let mut app = App::empty(data_dir);
        app.prefs = rune::prefs::Prefs::memory();
        app.backend = Some(backend);
        app.refresh_notebooks();
        app
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.inbox.waker = Some(waker);
    }

    /// Applies results that arrived from workers. True when any did.
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        while let Ok(f) = self.rx.try_recv() {
            f(self);
            any = true;
        }
        any
    }

    /// (Re)opens the backend for the current mode.
    pub fn connect(&mut self) {
        self.backend_error = None;
        self.remote = None;
        self.server = None;
        let backend: Result<Arc<dyn Backend>, String> = match self.settings.mode {
            Mode::Local => LocalBackend::open(&self.data_dir).map(|b| Arc::new(b) as Arc<dyn Backend>).map_err(|e| e.to_string()),
            Mode::Remote => {
                let password = self.secrets.get(settings::SERVER_PASSWORD).ok().flatten();
                let remote = Arc::new(RemoteBackend::new(&self.settings.server_url, password.as_deref()));
                self.remote = Some(remote.clone());
                Ok(remote as Arc<dyn Backend>)
            }
        };
        match backend {
            Ok(b) => {
                self.backend = Some(b);
                self.refresh_notebooks();
            }
            Err(e) => {
                self.backend = None;
                self.backend_error = Some(e);
            }
        }
    }

    /// Runs `work` on a worker thread and applies `done` to the state with its result.
    pub fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&dyn Backend) -> notebook_engine::Result<T> + Send + 'static,
        done: impl FnOnce(&mut App, notebook_engine::Result<T>) + Send + 'static,
    ) {
        let Some(backend) = self.backend.clone() else { return };
        let inbox = self.inbox.clone();
        std::thread::spawn(move || {
            let result = work(backend.as_ref());
            inbox.send(move |app| done(app, result));
        });
    }

    /// The waker that asks Rune for a frame, once `setup` has run.
    pub fn waker(&self) -> Option<Waker> {
        self.inbox.waker.clone()
    }

    pub fn inbox(&self) -> Inbox {
        self.inbox.clone()
    }

    /// Asks before `yes` runs; the Yes button says "Delete".
    pub fn ask(&mut self, question: impl Into<String>, yes: impl Fn(&mut App) + 'static) {
        self.ask_to("Delete", question, yes);
    }

    /// Asks before `yes` runs, with its own word on the Yes button ("Remove", "Discard", "Rebuild").
    pub fn ask_to(&mut self, verb: &'static str, question: impl Into<String>, yes: impl Fn(&mut App) + 'static) {
        self.confirm = Some(Confirm { question: question.into(), verb, yes: Box::new(yes) });
    }

    pub fn fail(&mut self, e: impl std::fmt::Display) {
        self.toast = Some(e.to_string());
    }

    // Notebooks

    pub fn refresh_notebooks(&self) {
        self.run(
            |b| b.list_notebooks(),
            |app, r| match r {
                Ok(list) => {
                    app.notebooks = list;
                    app.loaded = true;
                }
                Err(e) => {
                    app.loaded = true;
                    app.fail(e);
                }
            },
        );
    }

    pub fn create_notebook(&mut self) {
        let Some(edit) = self.new_notebook.take() else { return };
        let name = edit.text.trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.run(
            move |b| b.create_notebook(&name, ""),
            |app, r| match r {
                Ok(nb) => {
                    let id = nb.id.clone();
                    app.notebooks.insert(0, nb);
                    app.open_notebook(id);
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn delete_notebook(&mut self, id: String) {
        let gone = id.clone();
        self.run(
            move |b| b.delete_notebook(&id),
            move |app, r| match r {
                Ok(()) => {
                    app.notebooks.retain(|n| n.id != gone);
                    if app.open.as_ref().is_some_and(|o| o.id == gone) {
                        app.open = None;
                        app.route = Route::Home;
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn open_notebook(&mut self, id: String) {
        if self.open.as_ref().is_some_and(|o| o.id == id) {
            self.route = Route::Notebook(id);
            return;
        }
        self.open = Some(Open::new(id.clone()));
        self.route = Route::Notebook(id.clone());
        self.load_notebook(id);
    }

    /// Opens a notebook without loading it, for scenes that fill in their own data.
    pub fn open_offline(&mut self, id: &str) {
        self.open = Some(Open::new(id.into()));
        self.route = Route::Notebook(id.into());
    }

    /// Loads everything the notebook view shows.
    pub fn load_notebook(&self, id: String) {
        let i = id.clone();
        self.run(move |b| b.get_notebook(&i), with_open(&id, |o, nb| o.notebook = Some(nb)));
        self.refresh_sources();
        self.load_transformations();
        self.load_decks();
        self.load_episodes();
        let i = id.clone();
        self.run(move |b| b.list_notes(&i), with_open(&id, |o, notes| o.notes = notes));
        let i = id.clone();
        self.run(move |b| b.list_chat_sessions(&i), {
            let id = id.clone();
            move |app: &mut App, r: notebook_engine::Result<Vec<ChatSession>>| match r {
                Ok(sessions) => {
                    let latest = sessions.first().map(|s| s.id.clone());
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == id) {
                        o.sessions = sessions;
                    }
                    if let Some(sid) = latest {
                        app.open_session(sid);
                    }
                }
                Err(e) => app.fail(e),
            }
        });
    }

    pub fn refresh_sources(&self) {
        if let Some(o) = self.open.as_ref()
            && let Some((d, _)) = &o.reading
            && o.insights.iter().any(|i| i.status.as_deref() == Some("running"))
        {
            self.load_insights(d.summary.id.clone());
        }
        if self.open.as_ref().is_some_and(|o| o.episodes_in_progress()) {
            self.load_episodes();
        }
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let i = id.clone();
        self.run(move |b| b.list_sources(&i), with_open(&id, |o, sources| o.sources = sources));
    }

    pub fn refresh_notes(&self) {
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let i = id.clone();
        self.run(move |b| b.list_notes(&i), with_open(&id, |o, notes| o.notes = notes));
    }

    // Chat

    pub fn open_session(&self, session_id: String) {
        let Some(o) = self.open.as_ref() else { return };
        let (id, scope) = (o.id.clone(), o.scope.as_ref().map(|(sid, _)| sid.clone()));
        self.run(
            move |b| match &scope {
                Some(source) => b.get_source_chat_session(source, &session_id),
                None => b.get_chat_session(&session_id),
            },
            with_open(&id, |o, s| {
                o.session = Some(s);
                o.sessions_open = false;
            }),
        );
    }

    /// Switches the chat to one source, or back to the notebook with None, and opens its latest conversation.
    pub fn chat_scope(&mut self, scope: Option<(String, String)>) {
        let Some(o) = self.open.as_mut() else { return };
        if o.sending.is_some() {
            return;
        }
        o.scope = scope.clone();
        o.tab = NotebookTab::Chat;
        o.session = None;
        o.sessions.clear();
        o.error = None;
        let id = o.id.clone();
        let nb = id.clone();
        self.run(
            move |b| match &scope {
                Some((source, _)) => b.list_source_chat_sessions(source),
                None => b.list_chat_sessions(&nb),
            },
            move |app: &mut App, r: notebook_engine::Result<Vec<ChatSession>>| match r {
                Ok(sessions) => {
                    let latest = sessions.first().map(|s| s.id.clone());
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == id) {
                        o.sessions = sessions;
                    }
                    if let Some(sid) = latest {
                        app.open_session(sid);
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn new_chat(&mut self) {
        if let Some(o) = self.open.as_mut() {
            o.session = None;
            o.sessions_open = false;
            o.draft = TextEdit::new("");
        }
    }

    pub fn send(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let message = o.draft.text.trim().to_owned();
        if message.is_empty() || o.sending.is_some() {
            return;
        }
        let cancel = Cancel::new();
        o.sending = Some(cancel.clone());
        o.pending = Some((message.clone(), String::new()));
        o.draft = TextEdit::new("");
        o.error = None;
        let notebook_id = o.id.clone();
        let session_id = o.session.as_ref().map(|s| s.session.id.clone());
        let scope = o.scope.as_ref().map(|(sid, _)| sid.clone());
        let inbox = self.inbox.clone();
        let id = notebook_id.clone();
        self.run(
            move |b| {
                let mut on_text = |delta: &str| {
                    let delta = delta.to_owned();
                    let id = id.clone();
                    inbox.send(move |app| {
                        if let Some((_, reply)) = app.open.as_mut().filter(|o| o.id == id).and_then(|o| o.pending.as_mut()) {
                            reply.push_str(&delta);
                        }
                    });
                };
                match scope {
                    Some(source_id) => b.send_source_chat_message(notebook_engine::backend::SendSourceChat {
                        source_id,
                        session_id,
                        message,
                        model_override: None,
                        on_text: Some(&mut on_text),
                        cancel,
                    }),
                    None => b.send_chat_message(notebook_engine::backend::SendChat {
                        notebook_id,
                        session_id,
                        message,
                        context: None,
                        model_override: None,
                        on_text: Some(&mut on_text),
                        cancel,
                    }),
                }
            },
            move |app, r| {
                let Some(o) = app.open.as_mut() else { return };
                o.sending = None;
                match r {
                    Ok(result) => {
                        o.pending = None;
                        let session = result.session.clone();
                        if !o.sessions.iter().any(|s| s.id == session.id) {
                            o.sessions.insert(0, session.clone());
                        }
                        o.session = Some(ChatSessionWithMessages { session, messages: result.messages });
                    }
                    Err(notebook_engine::Error::Cancelled) => o.pending = None,
                    Err(e) => {
                        // Put the question back so it can be sent again.
                        if let Some((q, _)) = o.pending.take() {
                            o.draft = TextEdit::new(q);
                        }
                        o.error = Some(e.to_string());
                    }
                }
            },
        );
    }

    pub fn stop(&mut self) {
        if let Some(c) = self.open.as_ref().and_then(|o| o.sending.as_ref()) {
            c.cancel();
        }
    }

    pub fn delete_session(&mut self, session_id: String) {
        let gone = session_id.clone();
        let scope = self.open.as_ref().and_then(|o| o.scope.as_ref().map(|(sid, _)| sid.clone()));
        self.run(
            move |b| match &scope {
                Some(source) => b.delete_source_chat_session(source, &session_id),
                None => b.delete_chat_session(&session_id),
            },
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut() {
                        o.sessions.retain(|s| s.id != gone);
                        if o.session.as_ref().is_some_and(|s| s.session.id == gone) {
                            o.session = None;
                        }
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    // Sources

    pub fn read_source(&mut self, source_id: String, passage_query: Option<String>) {
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        if let Some(o) = self.open.as_mut() {
            o.insights.clear();
            o.insight_picker = false;
        }
        self.load_insights(source_id.clone());
        self.run(
            move |b| b.get_source(&source_id),
            with_open(&id, move |o, detail: SourceDetail| {
                let passage = passage_query.and_then(|q| notebook_engine::citations::best_passage(&detail.full_text, &q));
                o.reading = Some((detail, passage));
            }),
        );
    }

    pub fn add_source(&mut self, input: SourceInput) {
        let Some(o) = self.open.as_mut() else { return };
        if let Some(a) = o.add.as_mut() {
            a.busy = true;
            a.error = None;
        }
        let id = o.id.clone();
        let nb = id.clone();
        self.run(
            move |b| {
                let defaults = b.list_transformations().unwrap_or_default().into_iter().filter(|t| t.apply_default).map(|t| t.id).collect();
                b.add_source(&nb, NewSource { input, transformations: defaults })
            },
            move |app, r| {
                let Some(o) = app.open.as_mut().filter(|o| o.id == id) else { return };
                match r {
                    Ok(source) => {
                        o.add = None;
                        o.sources.insert(0, source);
                    }
                    Err(e) => {
                        if let Some(a) = o.add.as_mut() {
                            a.busy = false;
                            a.error = Some(e.to_string());
                        }
                    }
                }
            },
        );
    }

    pub fn delete_source(&mut self, source_id: String) {
        let gone = source_id.clone();
        self.run(
            move |b| b.delete_source(&source_id),
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut() {
                        o.sources.retain(|s| s.id != gone);
                        if o.reading.as_ref().is_some_and(|(d, _)| d.summary.id == gone) {
                            o.reading = None;
                        }
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn retry_source(&mut self, source_id: String) {
        self.run(
            move |b| b.retry_source(&source_id),
            |app, r| match r {
                Ok(()) => app.refresh_sources(),
                Err(e) => app.fail(e),
            },
        );
    }

    // Notes

    pub fn save_note(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let Some(draft) = o.note.take() else { return };
        let (title, content) = (draft.title.text.trim().to_owned(), draft.content.text.clone());
        if content.trim().is_empty() && title.is_empty() {
            return;
        }
        let nb = o.id.clone();
        self.run(
            move |b| {
                let title = (!title.is_empty()).then_some(title.as_str());
                match &draft.id {
                    Some(id) => b.update_note(id, title, Some(&content)),
                    None => b.create_note(&nb, title, &content, false),
                }
            },
            |app, r| match r {
                Ok(_) => app.refresh_notes(),
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn save_reply_as_note(&mut self, content: String) {
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let title = notebook_engine::text::title_from(&content, 60);
        self.run(
            move |b| b.create_note(&nb, Some(&title), &content, true),
            |app, r| match r {
                Ok(_) => {
                    app.toast = Some("Saved to notes".into());
                    app.refresh_notes();
                }
                Err(e) => app.fail(e),
            },
        );
    }

    pub fn delete_note(&mut self, note_id: String) {
        let gone = note_id.clone();
        self.run(
            move |b| b.delete_note(&note_id),
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut() {
                        o.notes.retain(|n| n.id != gone);
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    // Settings

    pub fn open_settings(&mut self) {
        self.form = Some(SettingsForm::from(&self.settings, &self.secrets));
        self.route = Route::Settings;
        self.load_slot_models();
    }

    /// Saves the mode, server address, base URL and models, and any key or password pasted
    /// into a field (empty fields keep what is saved), then reconnects.
    pub fn save_settings(&mut self) {
        let sync = self.settings.sync_keys;
        let Some(form) = self.form.as_mut() else { return };
        let mut s = self.settings.clone();
        s.server_url = form.server_url.text.trim().to_owned();
        s.compatible_base_url = form.compatible_base_url.text.trim().to_owned();
        let model = |provider: ProviderKind, edit: &TextEdit| {
            let m = edit.text.trim();
            (!m.is_empty()).then(|| settings::ModelSetting { provider: provider.as_str().into(), model: m.into() })
        };
        s.chat = model(form.chat_provider, &form.chat_model);
        s.embedding = model(form.embedding_provider, &form.embedding_model);
        s.transcription = model(form.transcription_provider, &form.transcription_model);
        s.transformation = model(form.transformation_provider, &form.transformation_model);
        let mut errors = Vec::new();
        let mut pasted: Vec<(String, &mut TextEdit)> = form.keys.iter_mut().map(|(id, edit)| (settings::provider_secret(id.as_str()), edit)).collect();
        pasted.push((settings::provider_secret("elevenlabs"), &mut form.elevenlabs));
        pasted.push((settings::SERVER_PASSWORD.into(), &mut form.server_password));
        for (name, edit) in pasted {
            if edit.text.trim().is_empty() {
                continue;
            }
            match self.secrets.put(&name, &edit.text, sync) {
                Ok(()) => {
                    *edit = TextEdit::new("");
                    form.saved_keys.insert(name);
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
        form.fix_providers();
        if let Err(e) = s.save(&self.data_dir) {
            errors.push(e.to_string());
        }
        form.saved = errors.is_empty();
        let mode_changed = s.mode != self.settings.mode || s.server_url != self.settings.server_url;
        self.settings = s;
        if let Some(e) = errors.first() {
            self.fail(e);
        }
        if mode_changed {
            self.open = None;
            self.notebooks.clear();
            self.route = Route::Settings;
        }
        // Reopening reads the new settings and keys; on device that's the same database file.
        self.connect();
    }
}

/// A result handler that applies `f` to the open notebook if it is still `id`, or shows the error.
fn with_open<T: Send + 'static>(id: &str, f: impl FnOnce(&mut Open, T) + Send + 'static) -> impl FnOnce(&mut App, notebook_engine::Result<T>) + Send + 'static {
    let id = id.to_owned();
    move |app, r| match r {
        Ok(v) => {
            if let Some(o) = app.open.as_mut().filter(|o| o.id == id) {
                f(o, v);
            }
        }
        Err(e) => app.fail(e),
    }
}

// Settings: key checks, model lists, key sync, the server test and the index rebuild.
impl App {
    fn provider_config(&self, kind: ProviderKind) -> notebook_engine::providers::ProviderConfig {
        let base_url = self.form.as_ref().map_or_else(|| self.settings.compatible_base_url.clone(), |f| f.compatible_base_url.text.trim().to_owned());
        notebook_engine::providers::ProviderConfig {
            kind,
            api_key: self.secrets.get(&settings::provider_secret(kind.as_str())).ok().flatten(),
            base_url: (kind == ProviderKind::OpenAiCompatible).then_some(base_url),
        }
    }

    /// Lists a provider's models off the UI thread, for the pickers and its key check.
    pub fn load_models(&mut self, kind: ProviderKind, check: bool) {
        let Some(f) = self.form.as_mut() else { return };
        f.models.insert(kind, ModelList::Loading);
        if check {
            f.checks.insert(kind.as_str().into(), Check::Checking);
        }
        let config = self.provider_config(kind);
        self.run(
            move |_| notebook_engine::providers::create_provider(&config, notebook_engine::net::UreqHttp::shared()).list_models(),
            move |app, r| {
                let Some(f) = app.form.as_mut() else { return };
                let (list, status) = match r {
                    Ok(models) => (ModelList::Ready(models.clone()), Check::Working(format!("Working, {} models", models.len()))),
                    Err(e) => (ModelList::Failed(e.to_string()), Check::Failed(e.to_string())),
                };
                f.models.insert(kind, list);
                if check || f.checks.contains_key(kind.as_str()) {
                    f.checks.insert(kind.as_str().into(), status);
                }
            },
        );
    }

    /// Loads the model lists of the providers the jobs use.
    pub fn load_slot_models(&mut self) {
        let Some(f) = self.form.as_ref() else { return };
        let kinds: BTreeSet<ProviderKind> =
            MODEL_ROLES.iter().map(|r| f.provider_of(r)).filter(|&k| f.has_provider_key(k) || k == ProviderKind::OpenAiCompatible).collect();
        for kind in kinds {
            self.load_models(kind, false);
        }
    }

    /// Saves a pasted key (and, for OpenAI-compatible, the base URL), then checks it by
    /// listing the provider's models.
    pub fn save_and_check_key(&mut self, kind: ProviderKind) {
        let sync = self.settings.sync_keys;
        let Some(f) = self.form.as_mut() else { return };
        let name = settings::provider_secret(kind.as_str());
        let Some(edit) = f.keys.iter_mut().find(|(k, _)| *k == kind).map(|(_, e)| e) else { return };
        if !edit.text.trim().is_empty() {
            if let Err(e) = self.secrets.put(&name, &edit.text, sync) {
                f.checks.insert(kind.as_str().into(), Check::Failed(e.to_string()));
                return;
            }
            *edit = TextEdit::new("");
            f.saved_keys.insert(name);
            f.fix_providers();
        }
        let url = f.compatible_base_url.text.trim().to_owned();
        if kind == ProviderKind::OpenAiCompatible && url != self.settings.compatible_base_url {
            self.settings.compatible_base_url = url;
            if let Err(e) = self.settings.save(&self.data_dir) {
                self.fail(e);
            }
        }
        self.load_models(kind, true);
    }

    /// Deletes a saved key from this Mac and iCloud Keychain.
    pub fn remove_key(&mut self, name: String) {
        if let Err(e) = self.secrets.delete(&name) {
            self.fail(e);
            return;
        }
        if let Some(f) = self.form.as_mut() {
            f.saved_keys.remove(&name);
            f.checks.remove(name.strip_prefix("provider:").unwrap_or(&name));
            f.fix_providers();
        }
    }

    /// Saves a pasted ElevenLabs key, then checks it by listing its voices.
    pub fn save_and_check_elevenlabs(&mut self) {
        use notebook_engine::podcast::voices::{VoiceConfig, VoiceKind, create_voice_provider};
        let sync = self.settings.sync_keys;
        let Some(f) = self.form.as_mut() else { return };
        let name = settings::provider_secret("elevenlabs");
        if !f.elevenlabs.text.trim().is_empty() {
            if let Err(e) = self.secrets.put(&name, &f.elevenlabs.text, sync) {
                f.checks.insert("elevenlabs".into(), Check::Failed(e.to_string()));
                return;
            }
            f.elevenlabs = TextEdit::new("");
            f.saved_keys.insert(name.clone());
        }
        f.checks.insert("elevenlabs".into(), Check::Checking);
        let config = VoiceConfig {
            kind: VoiceKind::ElevenLabs,
            model: VoiceKind::ElevenLabs.default_model().into(),
            api_key: self.secrets.get(&name).ok().flatten(),
            voices: Vec::new(),
            hosts: Vec::new(),
        };
        self.run(
            move |_| create_voice_provider(&config, notebook_engine::net::UreqHttp::shared(), None).list_voices(),
            |app, r| {
                if let Some(f) = app.form.as_mut() {
                    let status = match r {
                        Ok(v) => Check::Working(format!("Working, {} voices", v.len())),
                        Err(e) => Check::Failed(e.to_string()),
                    };
                    f.checks.insert("elevenlabs".into(), status);
                }
            },
        );
    }

    /// Turns iCloud Keychain sync on or off, moving every key to the matching store. The
    /// setting changes only when the move works.
    pub fn set_key_sync(&mut self, enabled: bool) {
        let Some(f) = self.form.as_mut() else { return };
        if f.syncing {
            return;
        }
        f.syncing = true;
        f.sync = None;
        let secrets = self.secrets.clone();
        let inbox = self.inbox();
        std::thread::spawn(move || {
            let result = secrets.set_sync(enabled, !enabled || settings::Keychain::available());
            inbox.send(move |app| {
                if result.is_ok() {
                    app.settings.sync_keys = enabled;
                    if let Err(e) = app.settings.save(&app.data_dir) {
                        app.fail(e);
                    }
                }
                if let Some(f) = app.form.as_mut() {
                    f.syncing = false;
                    f.sync = Some(result);
                }
            });
        });
    }

    /// Server mode: saves the address and any pasted password, reconnects, then checks the
    /// server by signing in and listing its notebooks.
    pub fn save_and_test_server(&mut self) {
        self.save_settings();
        let Some(remote) = self.remote() else { return };
        if let Some(f) = self.form.as_mut() {
            f.checks.insert("server".into(), Check::Checking);
        }
        self.run(
            move |_| {
                remote.client().auth_status()?;
                Ok(remote.client().list_notebooks(None)?.len())
            },
            |app, r| {
                if let Some(f) = app.form.as_mut() {
                    let status = match r {
                        Ok(1) => Check::Working("Connected, 1 notebook".into()),
                        Ok(n) => Check::Working(format!("Connected, {n} notebooks")),
                        Err(e) => Check::Failed(e.to_string()),
                    };
                    f.checks.insert("server".into(), status);
                }
            },
        );
    }

    pub fn forget_server_password(&mut self) {
        if let Err(e) = self.secrets.delete(settings::SERVER_PASSWORD) {
            self.fail(e);
            return;
        }
        if let Some(f) = self.form.as_mut() {
            f.saved_keys.remove(settings::SERVER_PASSWORD);
        }
    }

    /// On device: reads and indexes every source again with the current embedding model.
    pub fn rebuild_local_index(&mut self) {
        let Some(f) = self.form.as_mut() else { return };
        f.rebuild = Some(Check::Checking);
        self.run(
            |b| b.rebuild_index(),
            |app, r| {
                if let Some(f) = app.form.as_mut() {
                    f.rebuild = Some(match r {
                        Ok(1) => Check::Working("Re-indexing 1 source".into()),
                        Ok(n) => Check::Working(format!("Re-indexing {n} sources")),
                        Err(e) => Check::Failed(e.to_string()),
                    });
                }
            },
        );
    }
}

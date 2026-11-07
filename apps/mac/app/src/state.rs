//! App state and the work queue. Backend calls block, so they run on worker threads; each
//! sends back a closure that applies its result to the state, and the waker asks Rune for a
//! frame, whose `poll` runs the closures.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use notebook_engine::local::LocalBackend;
use notebook_engine::providers::ProviderKind;
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
    /// One row per source being added, once Add is pressed.
    pub items: Option<Vec<BatchItem>>,
    /// Files chosen or dropped, with their sizes.
    pub files: Vec<(PathBuf, u64)>,
    /// Transformations to run on the new sources; None until changed, which means the defaults.
    pub chosen: Option<std::collections::BTreeSet<String>>,
    /// Every source, for the From other notebooks tab, once loaded.
    pub library: Option<Vec<SourceSummary>>,
    /// Sources picked on that tab.
    pub picked: std::collections::BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddKind {
    #[default]
    Link,
    Text,
    File,
    /// Sources already in other notebooks.
    Library,
}

/// Where one source of a batch has got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemState {
    Waiting,
    Adding,
    Added,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchItem {
    pub label: String,
    pub state: ItemState,
}

/// One source to add in a batch.
#[derive(Clone, Debug)]
pub enum AddJob {
    Input(SourceInput),
    /// A file on disk, read on the worker; `title` replaces the name when set.
    File {
        path: PathBuf,
        title: Option<String>,
    },
    /// A source from another notebook, linked into this one.
    Link(String),
}

impl AddSource {
    /// "Adding 2 of 3" while a batch runs.
    pub fn progress(&self) -> Option<String> {
        let items = self.items.as_ref().filter(|_| self.busy)?;
        let done = items.iter().filter(|i| matches!(i.state, ItemState::Added | ItemState::Failed(_))).count();
        Some(format!("Adding {} of {}", (done + 1).min(items.len()), items.len()))
    }

    /// Adds files not already on the list.
    pub fn add_files(&mut self, paths: &[PathBuf]) {
        for p in paths {
            if !self.files.iter().any(|(f, _)| f == p) {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                self.files.push((p.clone(), size));
            }
        }
        if !paths.is_empty() {
            self.kind = AddKind::File;
        }
    }

    /// Sources on the From other notebooks tab: those not in `notebook_id` yet.
    pub fn elsewhere<'a>(&'a self, notebook_id: &'a str) -> impl Iterator<Item = &'a SourceSummary> + 'a {
        self.library.iter().flatten().filter(move |s| !s.notebooks.iter().any(|n| n == notebook_id))
    }

    /// How many sources Add would add on the current tab.
    pub fn count(&self, notebook_id: &str) -> usize {
        match self.kind {
            AddKind::Link => notebook_engine::text::parse_links(&self.url.text).len(),
            AddKind::File => self.files.len(),
            AddKind::Text => usize::from(!self.text.text.trim().is_empty()),
            AddKind::Library => self.elsewhere(notebook_id).filter(|s| self.picked.contains(&s.id)).count(),
        }
    }

    /// The transformations to run: those chosen, else the ones marked to apply by default.
    pub fn selected(&self, all: &[Transformation]) -> std::collections::BTreeSet<String> {
        self.chosen.clone().unwrap_or_else(|| all.iter().filter(|t| t.apply_default).map(|t| t.id.clone()).collect())
    }

    /// The sources to add for the current tab, each with its row label. The title field names
    /// the source only when there is one.
    pub fn jobs(&self, notebook_id: &str) -> Vec<(String, AddJob)> {
        let title = Some(self.title.text.trim().to_owned()).filter(|t| !t.is_empty());
        let single = self.count(notebook_id) == 1;
        match self.kind {
            AddKind::Link => notebook_engine::text::parse_links(&self.url.text)
                .into_iter()
                .map(|url| {
                    let label = url.trim_start_matches("https://").trim_start_matches("http://").to_owned();
                    (label, AddJob::Input(SourceInput::Link { url, title: title.clone().filter(|_| single) }))
                })
                .collect(),
            AddKind::File => self
                .files
                .iter()
                .map(|(path, _)| {
                    let label = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "File".into());
                    (label, AddJob::File { path: path.clone(), title: title.clone().filter(|_| single) })
                })
                .collect(),
            AddKind::Text if !self.text.text.trim().is_empty() => {
                let label = title.clone().unwrap_or_else(|| "Pasted text".into());
                vec![(label, AddJob::Input(SourceInput::Text { content: self.text.text.trim().to_owned(), title }))]
            }
            AddKind::Text => Vec::new(),
            AddKind::Library => self
                .elsewhere(notebook_id)
                .filter(|s| self.picked.contains(&s.id))
                .map(|s| (s.title.clone().unwrap_or_else(|| "Untitled source".into()), AddJob::Link(s.id.clone())))
                .collect(),
        }
    }

    /// How many of a finished batch failed.
    pub fn failed(&self) -> usize {
        self.items.iter().flatten().filter(|i| matches!(i.state, ItemState::Failed(_))).count()
    }
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
    /// Until when to keep checking for an insight just asked for: a server's appear only once written.
    pub insights_expected: Option<std::time::Instant>,
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
            insights_expected: None,
        }
    }

    /// Whether a new insight may still turn up, so its list keeps being checked.
    pub fn expecting_insights(&self) -> bool {
        self.insights_expected.is_some_and(|t| std::time::Instant::now() < t)
    }

    /// Sources still being read, or insights being written, which `poll` refreshes until they finish.
    pub fn processing(&self) -> bool {
        self.sources.iter().any(|s| is_processing(s.status.as_deref()))
            || self.insights.iter().any(|i| i.status.as_deref() == Some("running"))
            || (self.reading.is_some() && self.expecting_insights())
            || self.episodes_in_progress()
    }

    /// Episodes still being made, or whose claims are still being checked.
    pub fn episodes_in_progress(&self) -> bool {
        self.episodes
            .iter()
            .any(|e| !matches!(e.status, EpisodeStatus::Completed | EpisodeStatus::Failed) || e.check.as_ref().is_some_and(|c| c.status == "checking"))
    }
}

/// A source status that means it is still being read: the engine's and the server's words.
pub fn is_processing(status: Option<&str>) -> bool {
    matches!(status, Some("new" | "queued" | "pending" | "running" | "processing"))
}

/// A source status that means reading it failed.
pub fn is_failed(status: Option<&str>) -> bool {
    matches!(status, Some("failed" | "error"))
}

/// The settings screen's fields.
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
}

impl SettingsForm {
    pub fn from(settings: &AppSettings, secrets: &Secrets) -> SettingsForm {
        let secret = |name: &str| secrets.get(name).ok().flatten().unwrap_or_default();
        let model = |m: &Option<settings::ModelSetting>| m.as_ref().map(|m| m.model.clone()).unwrap_or_default();
        let provider = |m: &Option<settings::ModelSetting>, d| m.as_ref().and_then(|m| ProviderKind::parse(&m.provider)).unwrap_or(d);
        SettingsForm {
            server_url: TextEdit::new(settings.server_url.clone()),
            server_password: TextEdit::new(secret(settings::SERVER_PASSWORD)),
            keys: ProviderKind::ALL.iter().map(|&p| (p, TextEdit::new(secret(&settings::provider_secret(p.as_str()))))).collect(),
            compatible_base_url: TextEdit::new(settings.compatible_base_url.clone()),
            chat_model: TextEdit::new(model(&settings.chat)),
            chat_provider: provider(&settings.chat, ProviderKind::Anthropic),
            embedding_model: TextEdit::new(model(&settings.embedding)),
            embedding_provider: provider(&settings.embedding, ProviderKind::OpenAi),
            transcription_model: TextEdit::new(model(&settings.transcription)),
            transcription_provider: provider(&settings.transcription, ProviderKind::OpenAi),
            elevenlabs: TextEdit::new(secret(&settings::provider_secret("elevenlabs"))),
            saved: false,
            open_select: None,
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
    /// The sidebar's archived notebooks are showing.
    pub show_archived: bool,
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
            show_archived: false,
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
            && (o.insights.iter().any(|i| i.status.as_deref() == Some("running")) || o.expecting_insights())
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

    /// Opens the add sheet, on the Files tab with `files` when there are some.
    pub fn open_add(&mut self, files: &[PathBuf]) {
        let Some(o) = self.open.as_mut() else { return };
        let mut a = AddSource::default();
        a.add_files(files);
        a.url.focused = files.is_empty();
        o.add = Some(a);
    }

    /// Loads every source for the From other notebooks tab.
    pub fn load_library(&self) {
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(
            |b| b.list_all_sources(),
            with_open(&id, |o, list| {
                if let Some(a) = o.add.as_mut() {
                    a.library = Some(list);
                }
            }),
        );
    }

    /// Adds what the sheet holds on its current tab.
    pub fn submit_add(&mut self) {
        let Some(o) = self.open.as_ref() else { return };
        let Some(a) = o.add.as_ref() else { return };
        if a.busy {
            return;
        }
        let jobs = a.jobs(&o.id);
        if jobs.is_empty() {
            let e = match a.kind {
                AddKind::Link => "Enter a link that starts with http:// or https://",
                AddKind::File => "Choose a file first",
                AddKind::Text => "Paste some text first",
                AddKind::Library => "Pick at least one source",
            };
            if let Some(a) = self.open.as_mut().and_then(|o| o.add.as_mut()) {
                a.error = Some(e.into());
            }
            return;
        }
        let apply = if a.kind == AddKind::Library { Vec::new() } else { a.selected(&o.transformations).into_iter().collect() };
        self.add_sources(jobs, apply);
    }

    /// Adds one source with the default transformations, outside the add sheet.
    #[cfg(test)]
    pub fn add_source(&mut self, input: SourceInput) {
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let nb = id.clone();
        self.run(
            move |b| {
                let defaults = b.list_transformations().unwrap_or_default().into_iter().filter(|t| t.apply_default).map(|t| t.id).collect();
                b.add_source(&nb, NewSource { input, transformations: defaults })
            },
            with_open(&id, |o, source| o.sources.insert(0, source)),
        );
    }

    /// Adds sources one at a time on a worker, each row of the sheet showing how it went. One
    /// failure doesn't stop the rest; the sheet closes once every source is in.
    pub fn add_sources(&mut self, jobs: Vec<(String, AddJob)>, transformations: Vec<String>) {
        let remote = self.remote.is_some();
        let Some(backend) = self.backend.clone() else { return };
        let Some(o) = self.open.as_mut() else { return };
        let Some(a) = o.add.as_mut() else { return };
        if a.busy || jobs.is_empty() {
            return;
        }
        a.busy = true;
        a.error = None;
        a.items = Some(jobs.iter().map(|(label, _)| BatchItem { label: label.clone(), state: ItemState::Waiting }).collect());
        let nb = o.id.clone();
        let inbox = self.inbox.clone();
        std::thread::spawn(move || {
            for (i, (_, job)) in jobs.into_iter().enumerate() {
                let id = nb.clone();
                inbox.send(move |app| set_item(app, &id, i, ItemState::Adding));
                let result = run_job(backend.as_ref(), &nb, job, &transformations, remote);
                let id = nb.clone();
                inbox.send(move |app| {
                    let state = match &result {
                        Ok(_) => ItemState::Added,
                        Err(e) => ItemState::Failed(e.clone()),
                    };
                    set_item(app, &id, i, state);
                    if let Ok(Some(source)) = result
                        && let Some(o) = app.open.as_mut().filter(|o| o.id == id)
                        && !o.sources.iter().any(|s| s.id == source.id)
                    {
                        o.sources.insert(0, source);
                    }
                });
            }
            inbox.send(move |app| {
                if let Some(o) = app.open.as_mut().filter(|o| o.id == nb)
                    && let Some(a) = o.add.as_mut()
                {
                    a.busy = false;
                    if a.failed() == 0 {
                        o.add = None;
                    }
                }
                app.refresh_sources();
                app.refresh_notebooks();
            });
        });
    }

    /// Whether a source is in other notebooks too, so removing it here leaves it there.
    pub fn source_is_shared(&self, source_id: &str) -> bool {
        let Some(o) = self.open.as_ref() else { return false };
        let reading = o.reading.as_ref().filter(|(d, _)| d.summary.id == source_id).map(|(d, _)| d.summary.notebooks.len());
        let listed = o.sources.iter().find(|s| s.id == source_id).map(|s| s.notebooks.len());
        reading.or(listed).unwrap_or(0) > 1
    }

    /// Asks, then deletes the source, or only takes it out of this notebook when others have it too.
    pub fn ask_delete_source(&mut self, source_id: String) {
        if self.source_is_shared(&source_id) {
            self.ask_to("Remove", "Remove this source from this notebook? It stays in your other notebooks.", move |s| s.delete_source(source_id.clone()));
        } else {
            self.ask("Delete this source? Its text, insights and chats are removed.", move |s| s.delete_source(source_id.clone()));
        }
    }

    pub fn delete_source(&mut self, source_id: String) {
        let gone = source_id.clone();
        let shared = self.source_is_shared(&source_id);
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(
            move |b| if shared { b.unlink_source(&source_id, &nb) } else { b.delete_source(&source_id) },
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
    }

    pub fn save_settings(&mut self) {
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
        let mut errors = Vec::new();
        for (id, edit) in &form.keys {
            if let Err(e) = self.secrets.set(&settings::provider_secret(id.as_str()), &edit.text) {
                errors.push(e.to_string());
            }
        }
        if let Err(e) = self.secrets.set(&settings::provider_secret("elevenlabs"), &form.elevenlabs.text) {
            errors.push(e.to_string());
        }
        if let Err(e) = self.secrets.set(settings::SERVER_PASSWORD, &form.server_password.text) {
            errors.push(e.to_string());
        }
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

fn set_item(app: &mut App, notebook_id: &str, i: usize, state: ItemState) {
    if let Some(item) = app.open.as_mut().filter(|o| o.id == notebook_id).and_then(|o| o.add.as_mut()).and_then(|a| a.items.as_mut()).and_then(|v| v.get_mut(i))
    {
        item.state = state;
    }
}

/// Documents the add sheet reads, then audio and video, which are transcribed.
const DOCUMENTS: [&str; 12] = ["pdf", "docx", "pptx", "xlsx", "epub", "html", "htm", "md", "markdown", "txt", "csv", "json"];
const MEDIA: [&str; 12] = ["mp3", "m4a", "wav", "aac", "ogg", "flac", "webm", "mp4", "mov", "mkv", "mpeg", "mpga"];

/// Every file extension the add sheet takes.
pub fn accepted() -> Vec<&'static str> {
    DOCUMENTS.iter().chain(MEDIA.iter()).copied().collect()
}

/// Adds one source of a batch. A file is read here on device; a server reads it itself.
fn run_job(b: &dyn Backend, notebook_id: &str, job: AddJob, transformations: &[String], remote: bool) -> Result<Option<SourceSummary>, String> {
    let add = |input| b.add_source(notebook_id, NewSource { input, transformations: transformations.to_vec() }).map(Some).map_err(|e| e.to_string());
    match job {
        AddJob::Input(input) => add(input),
        AddJob::Link(source_id) => b.link_source(&source_id, notebook_id).map(|()| None).map_err(|e| e.to_string()),
        AddJob::File { path, title } => add(read_document(&path, title, remote)?),
    }
}

/// A file as a document source: its text read on device, or left for the server to read.
pub fn read_document(path: &std::path::Path, title: Option<String>, remote: bool) -> Result<SourceInput, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "File".into());
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s).to_owned();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mime = notebook_engine::extract::mime_for(&name).map(String::from);
    if remote {
        return Ok(SourceInput::Document { title: title.unwrap_or(stem), content: String::new(), file_name: Some(name), file: Some(bytes), mime });
    }
    let x = notebook_engine::extract::extract(&name, mime.as_deref(), &bytes).map_err(|e| e.to_string())?;
    if !x.media && x.text.trim().is_empty() {
        return Err("No readable text. Scanned PDFs need OCR, which this version does not do.".into());
    }
    let title = title.or_else(|| Some(x.title).filter(|t| !t.trim().is_empty())).unwrap_or(stem);
    Ok(SourceInput::Document { title, content: x.text, file_name: Some(name), file: Some(bytes), mime })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A new empty folder under the system's temporary folder.
    pub(crate) fn scratch() -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("on-state-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A state on a fresh on-device backend with one notebook open.
    pub(crate) fn opened(dir: &std::path::Path) -> App {
        let backend: Arc<dyn Backend> = Arc::new(LocalBackend::open(dir).unwrap());
        let nb = backend.create_notebook("Sleep", "").unwrap();
        let mut app = App::with_backend(dir.to_path_buf(), backend);
        app.open_notebook(nb.id);
        settle(&mut app, |_| false);
        app
    }

    /// Applies worker results until `done`, or for a moment when it never is.
    pub(crate) fn settle(app: &mut App, done: impl Fn(&App) -> bool) {
        for _ in 0..200 {
            app.drain();
            if done(app) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_batch_reports_each_source_and_stays_open_on_failure() {
        let dir = scratch();
        let mut app = opened(&dir);
        let empty = dir.join("empty.txt");
        std::fs::write(&empty, "  \n").unwrap();
        app.open.as_mut().unwrap().add = Some(AddSource::default());
        let jobs = vec![
            ("Pasted text".to_owned(), AddJob::Input(SourceInput::Text { content: "Spindles help memory.".into(), title: None })),
            ("Elsewhere".to_owned(), AddJob::Link("source:missing".into())),
            ("empty.txt".to_owned(), AddJob::File { path: empty, title: None }),
        ];
        app.add_sources(jobs, Vec::new());
        assert_eq!(app.open.as_ref().unwrap().add.as_ref().unwrap().progress().as_deref(), Some("Adding 1 of 3"));
        settle(&mut app, |a| a.open.as_ref().unwrap().add.as_ref().is_none_or(|a| !a.busy));
        let o = app.open.as_ref().unwrap();
        let a = o.add.as_ref().expect("a failure keeps the sheet open");
        let states: Vec<&ItemState> = a.items.as_ref().unwrap().iter().map(|i| &i.state).collect();
        assert_eq!(states[0], &ItemState::Added);
        assert!(matches!(states[1], ItemState::Failed(_)));
        assert!(matches!(states[2], ItemState::Failed(e) if e.starts_with("No readable text")));
        assert_eq!(a.failed(), 2);
        assert_eq!(a.progress(), None);
        assert_eq!(o.sources.len(), 1);
    }

    #[test]
    fn a_batch_that_all_works_closes_the_sheet() {
        let dir = scratch();
        let mut app = opened(&dir);
        app.open.as_mut().unwrap().add = Some(AddSource::default());
        let job = |t: &str| (t.to_owned(), AddJob::Input(SourceInput::Text { content: t.into(), title: None }));
        app.add_sources(vec![job("One"), job("Two")], Vec::new());
        settle(&mut app, |a| a.open.as_ref().unwrap().add.is_none());
        assert!(app.open.as_ref().unwrap().add.is_none());
    }

    #[test]
    fn the_sheet_makes_one_job_per_source() {
        let mut a = AddSource { url: TextEdit::new("https://a.example/x\nnot a link\nhttp://b.example"), title: TextEdit::new("Named"), ..Default::default() };
        let jobs = a.jobs("nb");
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].0, "a.example/x");
        // A title names a single source only.
        assert!(matches!(&jobs[0].1, AddJob::Input(SourceInput::Link { title: None, .. })));
        a.url = TextEdit::new("https://a.example/x");
        assert!(matches!(&a.jobs("nb")[0].1, AddJob::Input(SourceInput::Link { title: Some(t), .. }) if t == "Named"));

        a.kind = AddKind::Library;
        let src = |id: &str, nbs: &[&str]| SourceSummary { id: id.into(), notebooks: nbs.iter().map(|n| n.to_string()).collect(), ..Default::default() };
        a.library = Some(vec![src("s1", &["nb"]), src("s2", &["other"]), src("s3", &["other"])]);
        a.picked = ["s1".to_owned(), "s2".to_owned()].into();
        assert_eq!(a.elsewhere("nb").count(), 2);
        assert_eq!(a.count("nb"), 1, "a source already here is not added again");

        a.kind = AddKind::Text;
        assert_eq!(a.count("nb"), 0);
        let t = |id: &str, on| Transformation { id: id.into(), apply_default: on, ..Default::default() };
        let all = [t("t1", true), t("t2", false)];
        assert_eq!(a.selected(&all), ["t1".to_owned()].into());
        a.chosen = Some(Default::default());
        assert!(a.selected(&all).is_empty());
    }

    #[test]
    fn statuses_from_both_backends() {
        assert!(is_processing(Some("pending")) && is_processing(Some("processing")) && !is_processing(Some("completed")));
        assert!(is_failed(Some("error")) && is_failed(Some("failed")) && !is_failed(None));
    }

    #[test]
    fn a_shared_source_is_only_unlinked() {
        let dir = scratch();
        let mut app = opened(&dir);
        let b = app.backend.clone().unwrap();
        let here = app.open.as_ref().unwrap().id.clone();
        let other = b.create_notebook("Other", "").unwrap();
        let src = b.add_source(&here, NewSource { input: SourceInput::Text { content: "Shared".into(), title: None }, transformations: Vec::new() }).unwrap();
        b.link_source(&src.id, &other.id).unwrap();
        app.refresh_sources();
        settle(&mut app, |a| a.open.as_ref().unwrap().sources.first().is_some_and(|s| s.notebooks.len() == 2));
        assert!(app.source_is_shared(&src.id));
        app.delete_source(src.id.clone());
        settle(&mut app, |a| a.open.as_ref().unwrap().sources.is_empty());
        assert!(b.list_sources(&other.id).unwrap().iter().any(|s| s.id == src.id));
    }
}

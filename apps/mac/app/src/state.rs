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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotebookTab {
    Chat,
    Notes,
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
        }
    }

    /// Sources still being read or embedded, which the view refreshes until they finish.
    pub fn processing(&self) -> bool {
        self.sources.iter().any(|s| matches!(s.status.as_deref(), Some("new" | "queued" | "running" | "processing")))
    }
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
    pub backend_error: Option<String>,
    pub route: Route,
    pub sidebar: PaneState,
    pub notebooks: Vec<Notebook>,
    pub loaded: bool,
    pub open: Option<Open>,
    pub new_notebook: Option<TextEdit>,
    pub form: Option<SettingsForm>,
    pub toast: Option<String>,
    /// When the toast appeared (app clock), so `poll` can clear it.
    pub toast_since: Option<f64>,
    /// A question waiting for Yes: what it asks and what Yes does.
    pub confirm: Option<(String, OnYes)>,
    /// When `poll` next refreshes sources that are still being read (app clock).
    pub next_refresh: Option<f64>,
    inbox: Inbox,
    rx: Receiver<Apply>,
}

impl App {
    /// A state with no backend yet, its settings and keys read from `data_dir`.
    fn empty(data_dir: PathBuf) -> App {
        let (tx, rx) = channel();
        App {
            settings: AppSettings::load(&data_dir),
            secrets: Secrets::new(&data_dir),
            data_dir,
            backend: None,
            backend_error: None,
            route: Route::Home,
            sidebar: PaneState::new(248.0).range(180.0, 420.0),
            notebooks: Vec::new(),
            loaded: false,
            open: None,
            new_notebook: None,
            form: None,
            toast: None,
            toast_since: None,
            confirm: None,
            next_refresh: None,
            inbox: Inbox { tx, waker: None },
            rx,
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
        app.loaded = true;
        app
    }

    /// A state on a given backend (tests).
    #[cfg(test)]
    pub fn with_backend(data_dir: PathBuf, backend: Arc<dyn Backend>) -> App {
        let mut app = App::empty(data_dir);
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
        let backend: Result<Arc<dyn Backend>, String> = match self.settings.mode {
            Mode::Local => LocalBackend::open(&self.data_dir).map(|b| Arc::new(b) as Arc<dyn Backend>).map_err(|e| e.to_string()),
            Mode::Remote => {
                let password = self.secrets.get(settings::SERVER_PASSWORD).ok().flatten();
                Ok(Arc::new(RemoteBackend::new(&self.settings.server_url, password.as_deref())) as Arc<dyn Backend>)
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

    pub fn inbox(&self) -> Inbox {
        self.inbox.clone()
    }

    /// Asks before `yes` runs.
    pub fn ask(&mut self, question: impl Into<String>, yes: impl Fn(&mut App) + 'static) {
        self.confirm = Some((question.into(), Box::new(yes)));
    }

    pub fn fail(&mut self, e: impl std::fmt::Display) {
        self.toast = Some(e.to_string());
    }

    // Notebooks

    pub fn refresh_notebooks(&self) {
        self.run(|b| b.list_notebooks(), |app, r| match r {
            Ok(list) => {
                app.notebooks = list;
                app.loaded = true;
            }
            Err(e) => {
                app.loaded = true;
                app.fail(e);
            }
        });
    }

    pub fn create_notebook(&mut self) {
        let Some(edit) = self.new_notebook.take() else { return };
        let name = edit.text.trim().to_owned();
        if name.is_empty() {
            return;
        }
        self.run(move |b| b.create_notebook(&name, ""), |app, r| match r {
            Ok(nb) => {
                let id = nb.id.clone();
                app.notebooks.insert(0, nb);
                app.open_notebook(id);
            }
            Err(e) => app.fail(e),
        });
    }

    pub fn delete_notebook(&mut self, id: String) {
        let gone = id.clone();
        self.run(move |b| b.delete_notebook(&id), move |app, r| match r {
            Ok(()) => {
                app.notebooks.retain(|n| n.id != gone);
                if app.open.as_ref().is_some_and(|o| o.id == gone) {
                    app.open = None;
                    app.route = Route::Home;
                }
            }
            Err(e) => app.fail(e),
        });
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
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(move |b| b.get_chat_session(&session_id), with_open(&id, |o, s| {
            o.session = Some(s);
            o.sessions_open = false;
        }));
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
                b.send_chat_message(notebook_engine::backend::SendChat {
                    notebook_id,
                    session_id,
                    message,
                    context: None,
                    model_override: None,
                    on_text: Some(&mut on_text),
                    cancel,
                })
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
        self.run(move |b| b.delete_chat_session(&session_id), move |app, r| match r {
            Ok(()) => {
                if let Some(o) = app.open.as_mut() {
                    o.sessions.retain(|s| s.id != gone);
                    if o.session.as_ref().is_some_and(|s| s.session.id == gone) {
                        o.session = None;
                    }
                }
            }
            Err(e) => app.fail(e),
        });
    }

    // Sources

    pub fn read_source(&mut self, source_id: String, passage_query: Option<String>) {
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(move |b| b.get_source(&source_id), with_open(&id, move |o, detail: SourceDetail| {
            let passage = passage_query.and_then(|q| notebook_engine::citations::best_passage(&detail.full_text, &q));
            o.reading = Some((detail, passage));
        }));
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
        self.run(move |b| b.delete_source(&source_id), move |app, r| match r {
            Ok(()) => {
                if let Some(o) = app.open.as_mut() {
                    o.sources.retain(|s| s.id != gone);
                    if o.reading.as_ref().is_some_and(|(d, _)| d.summary.id == gone) {
                        o.reading = None;
                    }
                }
            }
            Err(e) => app.fail(e),
        });
    }

    pub fn retry_source(&mut self, source_id: String) {
        self.run(move |b| b.retry_source(&source_id), |app, r| match r {
            Ok(()) => app.refresh_sources(),
            Err(e) => app.fail(e),
        });
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
        self.run(move |b| b.create_note(&nb, Some(&title), &content, true), |app, r| match r {
            Ok(_) => {
                app.toast = Some("Saved to notes".into());
                app.refresh_notes();
            }
            Err(e) => app.fail(e),
        });
    }

    pub fn delete_note(&mut self, note_id: String) {
        let gone = note_id.clone();
        self.run(move |b| b.delete_note(&note_id), move |app, r| match r {
            Ok(()) => {
                if let Some(o) = app.open.as_mut() {
                    o.notes.retain(|n| n.id != gone);
                }
            }
            Err(e) => app.fail(e),
        });
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
        let mut errors = Vec::new();
        for (id, edit) in &form.keys {
            if let Err(e) = self.secrets.set(&settings::provider_secret(id.as_str()), &edit.text) {
                errors.push(e.to_string());
            }
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

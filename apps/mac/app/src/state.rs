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
    /// One row per source being added, once Add is pressed.
    pub items: Option<Vec<BatchItem>>,
    /// Files chosen or dropped, with their sizes.
    pub files: Vec<(PathBuf, u64)>,
    /// The native document picker opens after the current frame finishes.
    #[cfg(target_os = "ios")]
    pub pick_files: bool,
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

/// The notebook details dialog.
pub struct NotebookDetails {
    pub name: TextEdit,
    pub description: TextEdit,
}

/// Search results for one query, or why the search failed.
pub type Hits = (String, Result<Vec<SearchHit>, String>);

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
    pub conversation_actions: bool,
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
    pub details: Option<NotebookDetails>,
    /// Search inside this notebook: the box, a count that drops stale results, the results and
    /// whether their list shows.
    pub search: TextEdit,
    pub search_gen: u64,
    pub hits: Option<Hits>,
    pub search_open: bool,
    /// Files are being dragged over the notebook.
    pub drop_over: bool,
    /// A note to open once the notes have loaded.
    pub pending_note: Option<String>,
    /// The note being edited shows as rendered Markdown.
    pub note_read: bool,
    /// The note's title and text when it opened, to tell whether it changed.
    pub note_base: (String, String),
    /// The note being written came from a chat answer.
    pub note_ai: bool,
    /// How much of each source and note chat reads, kept per notebook in prefs.
    pub context: ContextSelection,
    /// The reader's title being renamed.
    pub renaming_source: Option<TextEdit>,
    /// The reader's original file is being saved to Downloads.
    pub downloading: bool,
    /// The reader should scroll the lit passage into view once it is laid out.
    pub scroll_to_passage: bool,
    /// The insight shown in full, with its actions.
    pub insight_open: Option<String>,
    /// Insights saved to notes since the reader opened.
    pub saved_insights: std::collections::BTreeSet<String>,
    /// Chat models to pick from (value sent as the override, label), once loaded.
    pub models: Option<Vec<(String, String)>>,
    pub models_open: bool,
    /// The conversation's name being edited.
    pub renaming_chat: Option<TextEdit>,
    /// The citation last clicked (message id, citation index), marked until the rail goes back to the list.
    pub active_cite: Option<(String, usize)>,
    /// On a phone, the Sources tab is showing in place of the notebook's other tabs.
    pub sources_tab: bool,
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
            conversation_actions: false,
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
            details: None,
            search: TextEdit::new(""),
            search_gen: 0,
            hits: None,
            search_open: false,
            drop_over: false,
            pending_note: None,
            note_read: false,
            note_base: (String::new(), String::new()),
            note_ai: false,
            context: ContextSelection::default(),
            renaming_source: None,
            downloading: false,
            scroll_to_passage: false,
            insight_open: None,
            saved_insights: Default::default(),
            models: None,
            models_open: false,
            renaming_chat: None,
            active_cite: None,
            sources_tab: false,
        }
    }

    /// Where the conversation's model choice is kept: its session, or new chats in this scope.
    pub fn override_key(&self) -> String {
        match (&self.session, &self.scope) {
            (Some(s), _) => s.session.id.clone(),
            (None, Some((source, _))) => format!("new:{source}"),
            (None, None) => format!("new:{}", self.id),
        }
    }

    /// Whether `request` is the message this view is sending.
    pub fn is_sending(&self, request: &Cancel) -> bool {
        self.sending.as_ref().is_some_and(|c| c.same(request))
    }

    /// Whether chat can answer: a source chat, or a notebook with a source that has been read.
    pub fn chat_ready(&self) -> bool {
        self.scope.is_some() || self.sources.iter().any(|s| matches!(s.status.as_deref(), None | Some("completed")))
    }

    /// How much of a source chat reads: as chosen, else its insights when it has some, else all of it.
    pub fn source_mode(&self, s: &SourceSummary) -> SourceContext {
        self.context.sources.get(&s.id).copied().unwrap_or(if s.insights_count > 0 { SourceContext::Insights } else { SourceContext::Full })
    }

    /// Whether chat reads a note; notes are in unless left out.
    pub fn note_included(&self, id: &str) -> bool {
        self.context.notes.get(id).copied().unwrap_or(true)
    }

    /// Opens a saved note in the editor, rendered until double-clicked.
    pub fn edit_note(&mut self, note: &Note) {
        let (title, content) = (note.title.clone().unwrap_or_default(), note.content.clone().unwrap_or_default());
        self.tab = NotebookTab::Notes;
        self.note_read = !content.trim().is_empty();
        self.note_base = (title.clone(), content.clone());
        self.note_ai = note.note_type.as_deref() == Some("ai");
        let mut c = TextEdit::new(content);
        c.focused = !self.note_read;
        self.note = Some(NoteDraft { id: Some(note.id.clone()), title: TextEdit::new(title), content: c });
    }

    /// Starts a new note, prefilled with `content` (a chat answer when `ai`).
    pub fn new_note(&mut self, title: String, content: String, ai: bool) {
        self.tab = NotebookTab::Notes;
        self.note_read = false;
        self.note_base = (String::new(), String::new());
        self.note_ai = ai;
        let mut c = TextEdit::new(content);
        c.focused = true;
        self.note = Some(NoteDraft { id: None, title: TextEdit::new(title), content: c });
    }

    /// Whether the note being edited differs from what was saved.
    pub fn note_dirty(&self) -> bool {
        self.note.as_ref().is_some_and(|d| d.id.is_none() || d.title.text != self.note_base.0 || d.content.text != self.note_base.1)
    }

    /// Replaces the notes, opening one that was asked for before they loaded.
    pub fn set_notes(&mut self, notes: Vec<Note>) {
        self.notes = notes;
        if let Some(id) = self.pending_note.take() {
            match self.notes.iter().find(|n| n.id == id).cloned() {
                Some(n) => self.edit_note(&n),
                None => self.tab = NotebookTab::Notes,
            }
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

/// The next chat context for a source, in the order the pill cycles: insights only once there are some.
pub fn next_mode(mode: SourceContext, has_insights: bool) -> SourceContext {
    match (mode, has_insights) {
        (SourceContext::Insights, _) => SourceContext::Full,
        (SourceContext::Full, _) => SourceContext::Off,
        (SourceContext::Off, true) => SourceContext::Insights,
        (SourceContext::Off, false) => SourceContext::Full,
    }
}

/// The prefs key for the model chosen per conversation.
const MODELS_KEY: &str = "chat.models";

/// Labels for a picker: duplicates get " (2)", " (3)" so each names one entry.
pub fn unique_labels(labels: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    labels
        .into_iter()
        .map(|l| {
            let n = seen.entry(l.clone()).or_insert(0);
            *n += 1;
            if *n == 1 { l } else { format!("{l} ({n})") }
        })
        .collect()
}

/// The prefs key for a notebook's chat context.
fn context_key(notebook_id: &str) -> String {
    format!("context.{notebook_id}")
}

/// A source status that means it is still being read: the engine's and the server's words.
pub fn is_processing(status: Option<&str>) -> bool {
    matches!(status, Some("new" | "queued" | "pending" | "running" | "processing"))
}

/// A source status that means reading it failed.
pub fn is_failed(status: Option<&str>) -> bool {
    matches!(status, Some("failed" | "error"))
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
    /// The fold of a partly folded iPhone Duo, `[x, y, w, h]` in window points, while it divides
    /// the screen. The two pages of the notebook sit either side of it.
    pub fold: Option<[f32; 4]>,
    /// On iPhone, the height of the home indicator's strip at the bottom when no keyboard covers
    /// it. The app draws under it and pads its last row by this much.
    pub inset_bottom: f32,
    /// On iPhone, the on-screen keyboard is up.
    pub keyboard_up: bool,
    /// The sidebar's archived notebooks are showing.
    pub show_archived: bool,
    /// The new-notebook dialog's "What it's for", beside `new_notebook`'s name.
    pub new_notebook_description: TextEdit,
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
            fold: None,
            inset_bottom: 0.0,
            keyboard_up: false,
            show_archived: false,
            new_notebook_description: TextEdit::new(""),
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
            self.new_notebook = Some(edit);
            return;
        }
        let description = std::mem::replace(&mut self.new_notebook_description, TextEdit::new("")).text.trim().to_owned();
        self.run(
            move |b| b.create_notebook(&name, &description),
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
        let mut o = Open::new(id.clone());
        o.context = self.prefs.get(&context_key(&id)).unwrap_or_default();
        self.open = Some(o);
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
        self.run(move |b| b.list_notes(&i), with_open(&id, |o, notes| o.set_notes(notes)));
        let i = id.clone();
        self.run(move |b| b.list_chat_sessions(&i), {
            let id = id.clone();
            move |app: &mut App, r: notebook_engine::Result<Vec<ChatSession>>| match r {
                Ok(sessions) => {
                    // Only while this notebook's chat still shows, or it would open in another.
                    let Some(o) = app.open.as_mut().filter(|o| o.id == id && o.scope.is_none()) else { return };
                    let latest = sessions.first().map(|s| s.id.clone());
                    o.sessions = sessions;
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
        self.run(move |b| b.list_notes(&i), with_open(&id, |o, notes| o.set_notes(notes)));
    }

    // Chat

    pub fn open_session(&self, session_id: String) {
        let Some(o) = self.open.as_ref() else { return };
        let (id, scope) = (o.id.clone(), o.scope.as_ref().map(|(sid, _)| sid.clone()));
        let asked_in = scope.clone();
        self.run(
            move |b| match &scope {
                Some(source) => b.get_source_chat_session(source, &session_id),
                None => b.get_chat_session(&session_id),
            },
            with_open(&id, move |o, s| {
                // Not if the chat has switched to another source, or back to the notebook, since.
                if o.scope.as_ref().map(|(sid, _)| sid) == asked_in.as_ref() {
                    o.session = Some(s);
                    o.sessions_open = false;
                }
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
        let source = scope.as_ref().map(|(sid, _)| sid.clone());
        self.run(
            move |b| match &source {
                Some(source) => b.list_source_chat_sessions(source),
                None => b.list_chat_sessions(&nb),
            },
            move |app: &mut App, r: notebook_engine::Result<Vec<ChatSession>>| match r {
                Ok(sessions) => {
                    let Some(o) = app.open.as_mut().filter(|o| o.id == id && o.scope == scope) else { return };
                    let latest = sessions.first().map(|s| s.id.clone());
                    o.sessions = sessions;
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
        let context = Some(o.context.clone()).filter(|c| !c.sources.is_empty() || !c.notes.is_empty());
        let key = o.override_key();
        let model_override =
            self.prefs.get::<std::collections::BTreeMap<String, String>>(MODELS_KEY).and_then(|m| m.get(&key).cloned()).filter(|m| !m.is_empty());
        let carried = model_override.clone().filter(|_| session_id.is_none());
        let inbox = self.inbox.clone();
        let (id, streaming) = (notebook_id.clone(), cancel.clone());
        let (sent_from, mine) = (notebook_id.clone(), cancel.clone());
        self.run(
            move |b| {
                let mut on_text = |delta: &str| {
                    let delta = delta.to_owned();
                    let (id, request) = (id.clone(), streaming.clone());
                    inbox.send(move |app| {
                        if let Some((_, reply)) = app.open.as_mut().filter(|o| o.id == id && o.is_sending(&request)).and_then(|o| o.pending.as_mut()) {
                            reply.push_str(&delta);
                        }
                    });
                };
                match scope {
                    Some(source_id) => b.send_source_chat_message(notebook_engine::backend::SendSourceChat {
                        source_id,
                        session_id,
                        message,
                        model_override,
                        on_text: Some(&mut on_text),
                        cancel,
                    }),
                    None => b.send_chat_message(notebook_engine::backend::SendChat {
                        notebook_id,
                        session_id,
                        message,
                        context,
                        model_override,
                        on_text: Some(&mut on_text),
                        cancel,
                    }),
                }
            },
            move |app, r| {
                // A model picked for a new chat stays with the conversation it started, wherever the user is now.
                if let (Ok(result), Some(m)) = (&r, carried) {
                    let mut map: std::collections::BTreeMap<String, String> = app.prefs.get(MODELS_KEY).unwrap_or_default();
                    map.remove(&key);
                    map.insert(result.session.id.clone(), m);
                    let _ = app.prefs.set(MODELS_KEY, &map);
                }
                // The user may have opened another notebook, or reopened this one, since: the
                // engine has saved the message, so only the view that sent it is updated.
                let Some(o) = app.open.as_mut().filter(|o| o.id == sent_from && o.is_sending(&mine)) else { return };
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
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        let scope = self.open.as_ref().and_then(|o| o.scope.as_ref().map(|(sid, _)| sid.clone()));
        self.run(
            move |b| match &scope {
                Some(source) => b.delete_source_chat_session(source, &session_id),
                None => b.delete_chat_session(&session_id),
            },
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == nb) {
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
            if o.reading.as_ref().is_none_or(|(d, _)| d.summary.id != source_id) {
                o.insights.clear();
                o.insight_open = None;
                o.saved_insights.clear();
                o.renaming_source = None;
            }
            o.insight_picker = false;
        }
        self.load_insights(source_id.clone());
        self.run(
            move |b| b.get_source(&source_id),
            with_open(&id, move |o, detail: SourceDetail| {
                let passage = passage_query.and_then(|q| notebook_engine::citations::best_passage(&detail.full_text, &q));
                o.scroll_to_passage = passage.is_some();
                o.reading = Some((detail, passage));
            }),
        );
    }

    /// Opens a note in its notebook's editor, from Ask, Search or a notebook search.
    pub fn open_note(&mut self, notebook_id: String, note_id: String) {
        let same = self.open.as_ref().is_some_and(|o| o.id == notebook_id);
        self.open_notebook(notebook_id);
        let Some(o) = self.open.as_mut() else { return };
        o.tab = NotebookTab::Notes;
        match o.notes.iter().find(|n| n.id == note_id).cloned() {
            Some(n) if same => o.edit_note(&n),
            _ => o.pending_note = Some(note_id),
        }
    }

    /// Renames, describes or (un)archives a notebook; closes the details dialog once saved.
    pub fn update_notebook(&mut self, id: String, update: NotebookUpdate) {
        let i = id.clone();
        self.run(
            move |b| b.update_notebook(&i, update),
            move |app, r| match r {
                Ok(nb) => {
                    if let Some(n) = app.notebooks.iter_mut().find(|n| n.id == id) {
                        let counts = (n.source_count, n.note_count);
                        *n = Notebook { source_count: counts.0, note_count: counts.1, ..nb.clone() };
                    }
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == id) {
                        o.notebook = Some(nb);
                        o.details = None;
                    }
                }
                Err(e) => app.fail(e),
            },
        );
    }

    /// The notebook search box changed: search 300 ms after typing stops, from two characters.
    pub fn search_changed(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        o.search_gen += 1;
        o.search_open = true;
        let (generation, query, id) = (o.search_gen, o.search.text.trim().to_owned(), o.id.clone());
        if query.chars().count() < 2 {
            o.hits = None;
            return;
        }
        let inbox = self.inbox.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            inbox.send(move |app| app.search_now(id, generation, query));
        });
    }

    /// Runs the search typed as `generation`, unless more has been typed since.
    pub fn search_now(&mut self, notebook_id: String, generation: u64, query: String) {
        let current = |app: &App| app.open.as_ref().filter(|o| o.id == notebook_id && o.search_gen == generation).is_some();
        if !current(self) {
            return;
        }
        let (nb, q) = (notebook_id.clone(), query.clone());
        self.run(
            move |b| b.search(&q, Some(&nb)),
            move |app, r| {
                if let Some(o) = app.open.as_mut().filter(|o| o.id == notebook_id && o.search_gen == generation) {
                    o.hits = Some((query, r.map_err(|e| e.to_string())));
                }
            },
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
    pub fn load_add_library(&self) {
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

    /// The model chosen for the current conversation, if any.
    pub fn model_override(&self) -> Option<String> {
        let key = self.open.as_ref()?.override_key();
        self.prefs.get::<std::collections::BTreeMap<String, String>>(MODELS_KEY)?.get(&key).cloned().filter(|m| !m.is_empty())
    }

    /// Picks a model for the current conversation; empty goes back to the default.
    pub fn set_model_override(&mut self, value: String) {
        let Some(key) = self.open.as_ref().map(|o| o.override_key()) else { return };
        let mut map: std::collections::BTreeMap<String, String> = self.prefs.get(MODELS_KEY).unwrap_or_default();
        if value.is_empty() {
            map.remove(&key);
        } else {
            map.insert(key, value);
        }
        if let Err(e) = self.prefs.set(MODELS_KEY, &map) {
            self.fail(e);
        }
    }

    /// What the default model is called in the picker.
    pub fn default_model_label(&self) -> String {
        if self.remote.is_some() {
            "Server default model".into()
        } else {
            self.settings.chat.as_ref().map(|m| m.model.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| "No chat model chosen".into())
        }
    }

    /// Loads the models a conversation can use: the server's language models, or the chat
    /// provider's models on device, as "provider::model".
    pub fn load_chat_models(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        if o.models.is_some() {
            return;
        }
        o.models = Some(Vec::new());
        let id = o.id.clone();
        let provider = match &self.remote {
            Some(_) => None,
            None => {
                let Some(kind) = self.settings.chat.as_ref().and_then(|m| ProviderKind::parse(&m.provider)) else { return };
                let key = self.secrets.get(&settings::provider_secret(kind.as_str())).ok().flatten();
                let base = (kind == ProviderKind::OpenAiCompatible).then(|| self.settings.compatible_base_url.clone());
                Some(notebook_engine::providers::ProviderConfig { kind, api_key: key, base_url: base })
            }
        };
        self.run(
            move |b| match provider {
                None => Ok(b.list_models()?.into_iter().filter(|m| m.kind == "language").map(|m| (m.id, format!("{} ({})", m.name, m.provider))).collect()),
                Some(config) => {
                    let kind = config.kind;
                    let models = notebook_engine::providers::create_provider(&config, notebook_engine::net::UreqHttp::shared()).list_models()?;
                    Ok(models
                        .into_iter()
                        .filter(|m| m.capabilities.is_empty() || m.capabilities.contains(&notebook_engine::providers::Capability::Chat))
                        .map(|m| (format!("{}::{}", kind.as_str(), m.id), m.label.unwrap_or(m.id)))
                        .collect::<Vec<_>>())
                }
            },
            with_open(&id, |o, list| o.models = Some(list)),
        );
    }

    /// Renames the open conversation.
    pub fn rename_chat(&mut self, title: String) {
        let Some(o) = self.open.as_mut() else { return };
        o.renaming_chat = None;
        let Some(sid) = o.session.as_ref().map(|s| s.session.id.clone()) else { return };
        let title = if title.trim().is_empty() { "Untitled conversation".to_owned() } else { title.trim().to_owned() };
        let (id, t) = (o.id.clone(), title.clone());
        let s2 = sid.clone();
        self.run(
            move |b| b.rename_chat_session(&s2, &t),
            with_open(&id, move |o, ()| {
                if let Some(s) = o.sessions.iter_mut().find(|s| s.id == sid) {
                    s.title = title.clone();
                }
                if let Some(s) = o.session.as_mut().filter(|s| s.session.id == sid) {
                    s.session.title = title;
                }
            }),
        );
    }

    /// Sets how much of a source chat reads, and remembers it for the notebook.
    pub fn set_source_context(&mut self, source_id: String, mode: SourceContext) {
        let Some(o) = self.open.as_mut() else { return };
        o.context.sources.insert(source_id, mode);
        let (key, value) = (context_key(&o.id), o.context.clone());
        if let Err(e) = self.prefs.set(&key, &value) {
            self.fail(e);
        }
    }

    /// Puts a note in chat or leaves it out, and remembers it for the notebook.
    pub fn set_note_included(&mut self, note_id: String, on: bool) {
        let Some(o) = self.open.as_mut() else { return };
        o.context.notes.insert(note_id, on);
        let (key, value) = (context_key(&o.id), o.context.clone());
        if let Err(e) = self.prefs.set(&key, &value) {
            self.fail(e);
        }
    }

    /// Renames the source in the reader.
    pub fn rename_source(&mut self, source_id: String, title: String) {
        let title = title.trim().to_owned();
        if let Some(o) = self.open.as_mut() {
            o.renaming_source = None;
        }
        if title.is_empty() {
            return;
        }
        let Some(id) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(
            move |b| b.rename_source(&source_id, &title),
            with_open(&id, |o, detail: SourceDetail| {
                if let Some(s) = o.sources.iter_mut().find(|s| s.id == detail.summary.id) {
                    s.title = detail.summary.title.clone();
                }
                if let Some((d, _)) = o.reading.as_mut().filter(|(d, _)| d.summary.id == detail.summary.id) {
                    d.summary.title = detail.summary.title;
                }
            }),
        );
    }

    /// Saves the source's original file to Downloads and shows it in Finder.
    pub fn download_source(&mut self, source_id: String) {
        let Some(o) = self.open.as_mut() else { return };
        if o.downloading {
            return;
        }
        o.downloading = true;
        let id = o.id.clone();
        self.run(
            move |b| {
                let file = b.download_source(&source_id)?.ok_or_else(|| notebook_engine::Error::other("This source has no original file."))?;
                let dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Downloads");
                std::fs::create_dir_all(&dir)?;
                let path = free_path(&dir, &file.name);
                std::fs::write(&path, &file.data)?;
                Ok(path)
            },
            move |app, r| {
                if let Some(o) = app.open.as_mut().filter(|o| o.id == id) {
                    o.downloading = false;
                }
                match r {
                    Ok(path) => {
                        #[cfg(target_os = "macos")]
                        let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        app.toast = Some(format!("Saved {name} to Downloads"));
                    }
                    Err(e) => app.fail(e),
                }
            },
        );
    }

    /// Saves an insight as a note; its button then says so.
    pub fn save_insight(&mut self, insight_id: String) {
        if let Some(o) = self.open.as_mut() {
            o.saved_insights.insert(insight_id.clone());
        }
        self.insight_to_note(insight_id);
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
        let from = nb.clone();
        self.run(
            move |b| if shared { b.unlink_source(&source_id, &nb) } else { b.delete_source(&source_id) },
            move |app, r| match r {
                Ok(()) => {
                    // A shared source taken out of this notebook is still in the others.
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == from) {
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

    /// Saves the note being edited, once it has text and has changed.
    pub fn save_note(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        if !o.note_dirty() || o.note.as_ref().is_none_or(|d| d.content.text.trim().is_empty()) {
            return;
        }
        let Some(draft) = o.note.take() else { return };
        let (title, content, ai) = (draft.title.text.trim().to_owned(), draft.content.text.clone(), o.note_ai);
        let (note_id, base) = (draft.id.clone(), o.note_base.clone());
        let (nb, from) = (o.id.clone(), o.id.clone());
        self.run(
            move |b| {
                let title = (!title.is_empty()).then_some(title.as_str());
                match &note_id {
                    Some(id) => b.update_note(id, title, Some(&content)),
                    None => b.create_note(&nb, title, &content, ai),
                }
            },
            move |app, r| match r {
                Ok(_) => app.refresh_notes(),
                Err(e) => {
                    // Back in the editor, unless another note was opened there since.
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == from && o.note.is_none()) {
                        o.note = Some(draft);
                        o.note_base = base;
                        o.note_ai = ai;
                        o.note_read = false;
                        o.tab = NotebookTab::Notes;
                    }
                    app.fail(e);
                }
            },
        );
    }

    /// Leaves the note editor, asking first when there are unsaved changes.
    pub fn leave_note(&mut self) {
        let Some(o) = self.open.as_mut() else { return };
        let unsaved = o.note_dirty() && o.note.as_ref().is_some_and(|d| !d.content.text.trim().is_empty());
        if unsaved {
            self.ask_to("Discard", "Discard your changes to this note?", |s| {
                if let Some(o) = s.open.as_mut() {
                    o.note = None;
                }
            });
        } else {
            o.note = None;
        }
    }

    pub fn delete_note(&mut self, note_id: String) {
        let gone = note_id.clone();
        let Some(nb) = self.open.as_ref().map(|o| o.id.clone()) else { return };
        self.run(
            move |b| b.delete_note(&note_id),
            move |app, r| match r {
                Ok(()) => {
                    if let Some(o) = app.open.as_mut().filter(|o| o.id == nb) {
                        o.notes.retain(|n| n.id != gone);
                        if o.note.as_ref().is_some_and(|d| d.id.as_deref() == Some(gone.as_str())) {
                            o.note = None;
                        }
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
        // On device with the same mode, the running engine re-reads settings.json in place, so
        // sources and episodes being made keep going. Keys are read on every call. Anything else
        // (a mode or server change) reconnects.
        if !mode_changed && let Some(local) = self.backend.as_ref().and_then(|b| b.local()) {
            local.reload_settings();
            self.refresh_notebooks();
        } else {
            self.connect();
        }
    }
}

/// `name` in `dir`, numbered ("report 2.pdf") when the name is taken.
pub fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let name = if name.trim().is_empty() { "source" } else { name };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (1..1000).map(|i| dir.join(if i == 1 { name.to_owned() } else { format!("{stem} {i}{ext}") })).find(|p| !p.exists()).unwrap_or_else(|| dir.join(name))
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
    fn a_note_opens_in_the_editor_once_notes_load() {
        let dir = scratch();
        let mut app = opened(&dir);
        let b = app.backend.clone().unwrap();
        let nb = app.open.as_ref().unwrap().id.clone();
        let note = b.create_note(&nb, Some("Thursday"), "Ask about **age**", false).unwrap();
        // From another page: the notebook reopens and the note opens when its notes arrive.
        app.open = None;
        app.open_note(nb.clone(), note.id.clone());
        assert_eq!(app.open.as_ref().unwrap().pending_note.as_deref(), Some(note.id.as_str()));
        settle(&mut app, |a| a.open.as_ref().unwrap().note.is_some());
        let o = app.open.as_ref().unwrap();
        assert_eq!(o.tab, NotebookTab::Notes);
        assert_eq!(o.note.as_ref().unwrap().id.as_deref(), Some(note.id.as_str()));
        assert!(o.note_read, "a saved note opens rendered");
        assert!(!o.note_dirty());
        // Within the same notebook it opens straight away.
        app.open.as_mut().unwrap().note = None;
        app.open_note(nb, note.id.clone());
        assert!(app.open.as_ref().unwrap().note.is_some());
    }

    #[test]
    fn search_waits_for_two_characters_and_drops_stale_results() {
        let dir = scratch();
        let mut app = opened(&dir);
        let nb = app.open.as_ref().unwrap().id.clone();
        app.open.as_mut().unwrap().search = TextEdit::new("s");
        app.search_changed();
        assert!(app.open.as_ref().unwrap().hits.is_none());
        app.open.as_mut().unwrap().search = TextEdit::new("spindles");
        app.search_changed();
        let stale = app.open.as_ref().unwrap().search_gen - 1;
        app.search_now(nb, stale, "s".into());
        settle(&mut app, |a| a.open.as_ref().unwrap().hits.is_some());
        let (query, _) = app.open.as_ref().unwrap().hits.clone().expect("the current search ran");
        assert_eq!(query, "spindles");
    }

    #[test]
    fn archiving_keeps_the_notebook_listed() {
        let dir = scratch();
        let mut app = opened(&dir);
        settle(&mut app, |a| !a.notebooks.is_empty());
        let id = app.open.as_ref().unwrap().id.clone();
        app.update_notebook(id.clone(), NotebookUpdate { archived: Some(true), description: Some("For the exam".into()), ..Default::default() });
        settle(&mut app, |a| a.notebooks[0].archived);
        assert!(app.notebooks[0].archived);
        assert_eq!(app.open.as_ref().unwrap().notebook.as_ref().unwrap().description, "For the exam");
    }

    #[test]
    fn chat_context_cycles_and_is_remembered_per_notebook() {
        assert_eq!(next_mode(SourceContext::Full, false), SourceContext::Off);
        assert_eq!(next_mode(SourceContext::Off, false), SourceContext::Full);
        assert_eq!(next_mode(SourceContext::Off, true), SourceContext::Insights);
        assert_eq!(next_mode(SourceContext::Insights, true), SourceContext::Full);

        let dir = scratch();
        let mut app = opened(&dir);
        let id = app.open.as_ref().unwrap().id.clone();
        let with = SourceSummary { id: "s1".into(), insights_count: 2, ..Default::default() };
        assert_eq!(app.open.as_ref().unwrap().source_mode(&with), SourceContext::Insights);
        app.set_source_context("s1".into(), SourceContext::Off);
        app.set_note_included("n1".into(), false);
        app.open = None;
        app.open_notebook(id);
        let o = app.open.as_ref().unwrap();
        assert_eq!(o.source_mode(&with), SourceContext::Off);
        assert!(!o.note_included("n1") && o.note_included("n2"));
    }

    #[test]
    fn downloads_take_a_free_name() {
        let dir = scratch();
        assert_eq!(free_path(&dir, "paper.pdf"), dir.join("paper.pdf"));
        std::fs::write(dir.join("paper.pdf"), b"x").unwrap();
        assert_eq!(free_path(&dir, "paper.pdf"), dir.join("paper 2.pdf"));
        assert_eq!(free_path(&dir, ""), dir.join("source"));
    }

    #[test]
    fn picker_labels_are_unique() {
        let l = unique_labels(["Plan".to_owned(), "Plan".to_owned(), "Other".to_owned(), "Plan".to_owned()]);
        assert_eq!(l, ["Plan", "Plan (2)", "Other", "Plan (3)"]);
    }

    #[test]
    fn a_model_picked_for_a_new_chat_moves_to_its_session() {
        let dir = scratch();
        let mut app = opened(&dir);
        let nb = app.open.as_ref().unwrap().id.clone();
        app.set_model_override("openai::gpt-x".into());
        assert_eq!(app.model_override().as_deref(), Some("openai::gpt-x"));
        assert_eq!(app.open.as_ref().unwrap().override_key(), format!("new:{nb}"));
        app.set_model_override(String::new());
        assert_eq!(app.model_override(), None);
    }

    #[test]
    fn chat_is_ready_once_a_source_is_read() {
        let mut o = Open::new("nb".into());
        assert!(!o.chat_ready());
        o.sources.push(SourceSummary { status: Some("processing".into()), ..Default::default() });
        assert!(!o.chat_ready());
        o.sources.push(SourceSummary { status: Some("completed".into()), ..Default::default() });
        assert!(o.chat_ready());
    }

    #[test]
    fn a_changed_note_asks_before_it_is_dropped() {
        let mut app = App::offline();
        app.open_offline("nb");
        let note = Note { id: "n1".into(), title: Some("T".into()), content: Some("Body".into()), ..Default::default() };
        app.open.as_mut().unwrap().edit_note(&note);
        app.leave_note();
        assert!(app.open.as_ref().unwrap().note.is_none(), "an unchanged note just closes");

        app.open.as_mut().unwrap().edit_note(&note);
        app.open.as_mut().unwrap().note.as_mut().unwrap().content = TextEdit::new("Body, edited");
        assert!(app.open.as_ref().unwrap().note_dirty());
        app.leave_note();
        assert!(app.open.as_ref().unwrap().note.is_some());
        let c = app.confirm.take().expect("asks first");
        assert_eq!(c.verb, "Discard");
        (c.yes)(&mut app);
        assert!(app.open.as_ref().unwrap().note.is_none());
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

    #[test]
    fn removing_a_shared_source_leaves_it_in_the_notebook_opened_next() {
        let dir = scratch();
        let mut app = opened(&dir);
        let b = app.backend.clone().unwrap();
        let here = app.open.as_ref().unwrap().id.clone();
        let other = b.create_notebook("Other", "").unwrap();
        let src = b.add_source(&here, NewSource { input: SourceInput::Text { content: "Shared".into(), title: None }, transformations: Vec::new() }).unwrap();
        b.link_source(&src.id, &other.id).unwrap();
        app.refresh_sources();
        settle(&mut app, |a| a.open.as_ref().unwrap().sources.first().is_some_and(|s| s.notebooks.len() == 2));
        app.delete_source(src.id.clone());
        // The other notebook is open, its sources shown, before the removal's result arrives.
        app.open_offline(&other.id);
        app.open.as_mut().unwrap().sources = b.list_sources(&other.id).unwrap();
        settle(&mut app, |_| false);
        assert!(app.open.as_ref().unwrap().sources.iter().any(|s| s.id == src.id), "still listed where it still is");
        assert!(b.list_sources(&here).unwrap().is_empty());
    }

    #[test]
    fn a_note_that_fails_to_save_comes_back_to_the_editor() {
        let dir = scratch();
        let mut app = opened(&dir);
        // A note the backend doesn't have, so the save fails as an unreachable server's would.
        let note = Note { id: "note:missing".into(), title: Some("Plan".into()), content: Some("Body".into()), ..Default::default() };
        app.open.as_mut().unwrap().edit_note(&note);
        app.open.as_mut().unwrap().note.as_mut().unwrap().content = TextEdit::new("Body, edited");
        app.save_note();
        assert!(app.open.as_ref().unwrap().note.is_none(), "the editor closes while it saves");
        settle(&mut app, |a| a.toast.is_some());
        assert!(app.toast.is_some(), "the failure is shown");
        let o = app.open.as_ref().unwrap();
        let draft = o.note.as_ref().expect("the draft is back");
        assert_eq!(draft.id.as_deref(), Some("note:missing"));
        assert_eq!(draft.content.text, "Body, edited");
        assert_eq!(o.tab, NotebookTab::Notes);
        assert!(o.note_dirty(), "it still counts as unsaved");

        // A new note that fails goes back too, still new.
        let nb = o.id.clone();
        app.open = None;
        app.open_offline("notebook:gone");
        app.open.as_mut().unwrap().new_note(String::new(), "Draft text".into(), false);
        app.save_note();
        settle(&mut app, |a| a.open.as_ref().unwrap().note.is_some());
        let draft = app.open.as_ref().unwrap().note.as_ref().expect("the new draft is back");
        assert!(draft.id.is_none());
        assert_eq!(draft.content.text, "Draft text");
        assert!(app.backend.clone().unwrap().list_notes(&nb).unwrap().is_empty());
    }
}

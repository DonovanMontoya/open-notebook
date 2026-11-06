//! Port of `local.test.ts`, `ask.test.ts`, `studio.test.ts` and `insights.test.ts`:
//! a real database in a temp folder, a fake provider and canned web pages.

mod chat;
mod insights;
mod podcasts;
mod sources;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use time::OffsetDateTime;

use super::{Extracted, LocalBackend, LocalOptions};
use crate::backend::{Backend, Cancel, Result, SendChat, SendSourceChat};
use crate::providers::{Capability, ChatRequest, ChatTurn, ModelInfo, Provider, ProviderKind};
use crate::settings::{AppSettings, ModelSetting};
use crate::types::{NewSource, SourceInput};
use crate::Error;

/// A folder removed when the test ends.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("on-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug)]
pub struct Recorded {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatTurn>,
}

type Reply = Box<dyn Fn(&Recorded) -> String + Send + Sync>;

/// Letter-frequency "embedding": similar text gives similar vectors, deterministically.
pub fn letter_vector(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; 26];
    for c in text.to_lowercase().chars() {
        if c.is_ascii_lowercase() {
            v[(c as u8 - b'a') as usize] += 1.0;
        }
    }
    v
}

/// A deterministic provider that records every call.
pub struct Fake {
    pub chats: Mutex<Vec<Recorded>>,
    pub embeds: Mutex<Vec<Vec<String>>>,
    reply: Mutex<Reply>,
    fail: Mutex<Option<String>>,
    embed_delay: Mutex<Option<Duration>>,
    in_flight: AtomicUsize,
    pub peak: AtomicUsize,
}

impl Fake {
    pub fn new() -> Arc<Fake> {
        Arc::new(Fake {
            chats: Mutex::default(),
            embeds: Mutex::default(),
            reply: Mutex::new(Box::new(|_| "Fake answer [Doc]".into())),
            fail: Mutex::default(),
            embed_delay: Mutex::default(),
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        })
    }
    pub fn reply(&self, f: impl Fn(&Recorded) -> String + Send + Sync + 'static) {
        *self.reply.lock().unwrap() = Box::new(f);
    }
    pub fn reply_text(&self, text: &str) {
        let text = text.to_string();
        self.reply(move |_| text.clone());
    }
    pub fn fail(&self, message: Option<&str>) {
        *self.fail.lock().unwrap() = message.map(String::from);
    }
    pub fn slow_embeddings(&self, delay: Duration) {
        *self.embed_delay.lock().unwrap() = Some(delay);
    }
    pub fn last(&self) -> Recorded {
        self.chats.lock().unwrap().last().cloned().expect("a chat call")
    }
    pub fn embed_count(&self) -> usize {
        self.embeds.lock().unwrap().len()
    }
}

impl Provider for Fake {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAi
    }
    fn supports_embeddings(&self) -> bool {
        true
    }
    fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![ModelInfo { id: "fake-chat".into(), label: None, capabilities: vec![Capability::Chat] }])
    }
    fn chat(&self, mut request: ChatRequest<'_>) -> Result<String> {
        let recorded = Recorded { model: request.model.clone(), system: request.system.clone(), messages: request.messages.clone() };
        self.chats.lock().unwrap().push(recorded.clone());
        if let Some(message) = self.fail.lock().unwrap().clone() {
            return Err(Error::other(message));
        }
        let text = (self.reply.lock().unwrap())(&recorded);
        if let Some(on_text) = request.on_text.as_mut() {
            for word in text.split_inclusive(' ') {
                on_text(word);
            }
        }
        Ok(text)
    }
    fn embed(&self, _model: &str, texts: &[String], _cancel: &Cancel) -> Result<Vec<Vec<f32>>> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        let delay = *self.embed_delay.lock().unwrap();
        if let Some(delay) = delay {
            std::thread::sleep(delay);
        }
        self.embeds.lock().unwrap().push(texts.to_vec());
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(texts.iter().map(|t| letter_vector(t)).collect())
    }
}

pub fn model(name: &str) -> Option<ModelSetting> {
    Some(ModelSetting { provider: "openai".into(), model: name.into() })
}

/// Chat and embedding models set, as most tests use.
pub fn both_models() -> AppSettings {
    AppSettings { chat: model("fake-chat"), embedding: model("fake-embed"), ..Default::default() }
}

pub type Pages = Arc<Mutex<HashMap<String, Extracted>>>;

pub struct Env {
    pub backend: LocalBackend,
    pub fake: Arc<Fake>,
    pub pages: Pages,
    pub dir: TempDir,
}

/// A clock that moves one second per reading, so ordering by time is stable.
pub fn ticking_clock() -> super::Clock {
    let tick = Arc::new(AtomicI64::new(OffsetDateTime::parse("2026-11-01T12:00:00Z", &time::format_description::well_known::Rfc3339).unwrap().unix_timestamp()));
    Arc::new(move || OffsetDateTime::from_unix_timestamp(tick.fetch_add(1, Ordering::SeqCst) + 1).unwrap())
}

pub fn options(settings: AppSettings, fake: &Arc<Fake>, pages: &Pages) -> LocalOptions {
    let provider = fake.clone();
    let pages = pages.clone();
    LocalOptions {
        http: Some(Arc::new(|_r: crate::net::Request| -> std::result::Result<crate::net::Response, crate::net::Transport> {
            Err(crate::net::Transport::Failed("no network in tests".into()))
        })),
        providers: Some(Arc::new(move |_| provider.clone() as Arc<dyn Provider>)),
        extract_url: Some(Arc::new(move |url: &str, _: &Cancel| {
            pages.lock().unwrap().get(url).cloned().ok_or_else(|| Error::other(format!("404 for {url}")))
        })),
        clock: Some(ticking_clock()),
        settings: Some(settings),
        voices: None,
        open_socket: None,
    }
}

pub fn setup_with(settings: AppSettings) -> Env {
    let dir = TempDir::new();
    let fake = Fake::new();
    let pages: Pages = Arc::default();
    let backend = LocalBackend::open_with(dir.path(), options(settings, &fake, &pages)).unwrap();
    Env { backend, fake, pages, dir }
}

pub fn setup() -> Env {
    setup_with(both_models())
}

pub fn text(content: &str, title: &str) -> NewSource {
    NewSource { input: SourceInput::Text { content: content.into(), title: Some(title.into()) }, transformations: vec![] }
}

pub fn link(url: &str) -> NewSource {
    NewSource { input: SourceInput::Link { url: url.into(), title: None }, transformations: vec![] }
}

pub fn chat<'a>(notebook_id: &str, message: &str) -> SendChat<'a> {
    SendChat {
        notebook_id: notebook_id.into(),
        session_id: None,
        message: message.into(),
        context: None,
        model_override: None,
        on_text: None,
        cancel: Cancel::new(),
    }
}

pub fn source_chat<'a>(source_id: &str, message: &str) -> SendSourceChat<'a> {
    SendSourceChat { source_id: source_id.into(), session_id: None, message: message.into(), model_override: None, on_text: None, cancel: Cancel::new() }
}

pub fn count(env: &Env, table: &str) -> i64 {
    env.backend.db.lock().query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
}

#[test]
fn timestamps_and_ids_use_the_typescript_formats() {
    let env = setup();
    let nb = env.backend.create_notebook("N", "").unwrap();
    assert!(nb.id.starts_with("notebook:") && uuid::Uuid::parse_str(&nb.id["notebook:".len()..]).is_ok(), "{}", nb.id);
    assert!(nb.created.starts_with("2026-11-01T12:00:0") && nb.created.ends_with(".000Z") && nb.created.len() == 24, "{}", nb.created);
    let s = env.backend.add_source(&nb.id, text("Hello there.", "Doc")).unwrap();
    env.backend.idle();
    let (chunk_id, embedding): (String, String) =
        env.backend.db.lock().query_row("SELECT id, embedding FROM chunk WHERE source_id = ?1", [&s.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(chunk_id, format!("{}#0", s.id));
    assert_eq!(crate::text::decode_vector(&embedding).unwrap(), letter_vector("Hello there."));
}

#[test]
fn a_database_written_by_the_tauri_app_opens_without_losing_data() {
    let dir = TempDir::new();
    {
        // The Tauri app's schema from before the later columns, with a row in it.
        let conn = rusqlite::Connection::open(dir.path().join(crate::db::FILE_NAME)).unwrap();
        conn.execute_batch(
            "CREATE TABLE notebook (id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', archived INTEGER NOT NULL DEFAULT 0, created TEXT NOT NULL, updated TEXT NOT NULL);
             CREATE TABLE source (id TEXT PRIMARY KEY, notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE, title TEXT, kind TEXT NOT NULL, url TEXT, full_text TEXT NOT NULL DEFAULT '', status TEXT NOT NULL, error TEXT, created TEXT NOT NULL, updated TEXT NOT NULL);
             INSERT INTO notebook VALUES ('notebook:old', 'From Tauri', 'kept', 0, '2026-10-01T10:00:00.000Z', '2026-10-01T10:00:00.000Z');
             INSERT INTO source VALUES ('source:old', 'notebook:old', 'Old doc', 'text', NULL, 'Old text', 'completed', NULL, '2026-10-01T10:00:00.000Z', '2026-10-01T10:00:00.000Z');",
        )
        .unwrap();
    }
    let fake = Fake::new();
    let backend = LocalBackend::open_with(dir.path(), options(both_models(), &fake, &Pages::default())).unwrap();
    let nb = backend.get_notebook("notebook:old").unwrap();
    assert_eq!((nb.name.as_str(), nb.description.as_str(), nb.source_count), ("From Tauri", "kept", 1));
    let s = backend.get_source("source:old").unwrap();
    assert_eq!(s.full_text, "Old text");
    assert_eq!(s.summary.notebooks, ["notebook:old"]);
}

#[test]
fn notebooks_track_source_and_note_counts_and_sort_by_last_update() {
    let env = setup();
    let b = &env.backend;
    let a = b.create_notebook("Alpha", "first").unwrap();
    let beta = b.create_notebook("Beta", "").unwrap();
    b.create_note(&a.id, None, "A note", false).unwrap();
    b.add_source(&a.id, text("Some text", "Doc")).unwrap();
    b.idle();

    let list = Backend::list_notebooks(b).unwrap();
    assert_eq!(list.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta"]);
    assert_eq!((list[0].note_count, list[0].source_count), (1, 1));
    let archived = b.update_notebook(&beta.id, crate::types::NotebookUpdate { archived: Some(true), ..Default::default() }).unwrap();
    assert!(archived.archived);
    assert_eq!(archived.name, "Beta");
}

#[test]
fn notes_are_titled_from_their_first_line_and_can_be_edited() {
    let env = setup();
    let b = &env.backend;
    let nb = b.create_notebook("N", "").unwrap();
    let note = b.create_note(&nb.id, None, "First line\nmore", false).unwrap();
    assert_eq!((note.title.as_deref(), note.note_type.as_deref()), (Some("First line"), Some("human")));
    let edited = b.update_note(&note.id, None, Some("New body")).unwrap();
    assert_eq!((edited.title.as_deref(), edited.content.as_deref()), (Some("First line"), Some("New body")));
    b.delete_note(&note.id).unwrap();
    assert!(b.list_notes(&nb.id).unwrap().is_empty());
    assert!(matches!(b.update_note(&note.id, None, None), Err(Error::NotFound(_))));
}

#[test]
fn the_mac_app_lists_no_server_models() {
    let env = setup();
    assert!(Backend::list_models(&env.backend).unwrap().is_empty());
}

#[test]
fn settings_changes_apply_to_the_next_call() {
    let env = setup_with(AppSettings::default());
    let nb = env.backend.create_notebook("N", "").unwrap();
    assert!(env.backend.send_chat_message(chat(&nb.id, "Hi")).unwrap_err().to_string().contains("Choose a chat model"));
    both_models().save(env.dir.path()).unwrap();
    env.backend.reload_settings();
    env.backend.send_chat_message(chat(&nb.id, "Hi")).unwrap();
    assert_eq!(env.fake.last().model, "fake-chat");
}

#[test]
fn keys_come_from_the_key_file_and_the_compatible_base_url_from_settings() {
    let env = setup();
    crate::settings::Secrets::new(env.dir.path()).set("provider:anthropic", "sk-ant").unwrap();
    let config = env.backend.ai_config().unwrap();
    assert_eq!(config.providers[&ProviderKind::Anthropic].api_key.as_deref(), Some("sk-ant"));
    assert_eq!(config.providers[&ProviderKind::OpenAi].api_key, None);
    assert_eq!(config.providers[&ProviderKind::OpenAiCompatible].base_url.as_deref(), Some("https://openrouter.ai/api/v1"));
    assert_eq!(config.chat.unwrap().model, "fake-chat");
    assert!(config.transformation.is_none());
}

//! The on-device backend: SQLite in the app's data folder, AI calls with the
//! user's own keys. Port of `packages/engine/src/local.ts`.
//!
//! Calls block (the UI runs them on worker threads). Source processing and
//! insights run in the background on a small pool of std threads, two at a
//! time like the TypeScript engine; their status lives in the database, which
//! the UI polls.

mod backend_impl;
mod chat;
mod insights;
mod jobs;
mod notebooks;
mod research;
mod sources;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use time::OffsetDateTime;

use crate::backend::{Cancel, Result};
use crate::db::{self, Db};
use crate::net::{SharedHttp, UreqHttp};
use crate::providers::{ChatRequest, Provider, ProviderConfig, ProviderKind, create_provider};
use crate::settings::{AppSettings, ModelSetting, Secrets, provider_secret};
use crate::Error;

pub use crate::extract::Extracted;

/// Builds a provider for a config. Tests swap in a fake.
pub type ProviderFactory = Arc<dyn Fn(&ProviderConfig) -> Arc<dyn Provider> + Send + Sync>;
/// Downloads a link and returns its readable text.
pub type UrlExtractor = Arc<dyn Fn(&str, &Cancel) -> Result<Extracted> + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;

/// Optional overrides for [`LocalBackend::open_with`]; the defaults are the real network and clock.
#[derive(Default, Clone)]
pub struct LocalOptions {
    pub http: Option<SharedHttp>,
    pub providers: Option<ProviderFactory>,
    pub extract_url: Option<UrlExtractor>,
    pub clock: Option<Clock>,
    /// Settings to start with instead of `settings.json`.
    pub settings: Option<AppSettings>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelChoice {
    pub provider: ProviderConfig,
    pub model: String,
}

/// What a call needs from Settings and the key file, read fresh each time
/// (the TypeScript engine's `getAiConfig`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AiConfig {
    pub chat: Option<ModelChoice>,
    /// Optional: without it, search and retrieval fall back to keyword matching.
    pub embedding: Option<ModelChoice>,
    /// Transformations, insights and study decks; the chat model when unset.
    pub transformation: Option<ModelChoice>,
    /// Speech to text for audio and video sources.
    pub transcription: Option<ModelChoice>,
    /// Every provider, with its saved key if any, so a conversation can switch models.
    pub providers: BTreeMap<ProviderKind, ProviderConfig>,
}

impl AiConfig {
    /// The model for transformations, insights and study decks.
    pub fn writer(&self) -> Option<&ModelChoice> {
        self.transformation.as_ref().or(self.chat.as_ref())
    }
}

/// Sources processed at the same time; the rest wait their turn.
pub(crate) const SOURCE_CONCURRENCY: usize = 2;
/// Notebooks up to this many characters of source text are sent to the model whole (about 40k tokens).
pub const FULL_CONTEXT_CHARS: usize = 160_000;

pub struct Engine {
    data_dir: PathBuf,
    db: Db,
    settings: RwLock<AppSettings>,
    secrets: Secrets,
    http: SharedHttp,
    providers: ProviderFactory,
    extract_url: UrlExtractor,
    clock: Clock,
    /// Cancels for background work, by source or insight id, with a generation so a
    /// finished job never removes the cancel of a newer one.
    running: Mutex<HashMap<String, (u64, Cancel)>>,
    generation: AtomicU64,
    jobs: Arc<jobs::Jobs>,
}

/// The on-device backend. Cheap to clone; clones share one database and job pool.
#[derive(Clone)]
pub struct LocalBackend(Arc<Engine>);

impl std::ops::Deref for LocalBackend {
    type Target = Arc<Engine>;
    fn deref(&self) -> &Arc<Engine> {
        &self.0
    }
}

impl LocalBackend {
    /// Opens `<data_dir>/open-notebook.db` (the Tauri app's file), migrates it and
    /// marks work left unfinished by a previous run as failed.
    pub fn open(data_dir: &Path) -> Result<LocalBackend> {
        Self::open_with(data_dir, LocalOptions::default())
    }

    pub fn open_with(data_dir: &Path, options: LocalOptions) -> Result<LocalBackend> {
        let db = Db::open(&data_dir.join(db::FILE_NAME))?;
        let http = options.http.unwrap_or_else(UreqHttp::shared);
        let providers = options.providers.unwrap_or_else(|| {
            let http = http.clone();
            Arc::new(move |config: &ProviderConfig| create_provider(config, http.clone()))
        });
        let extract_url = options.extract_url.unwrap_or_else(|| {
            let http = http.clone();
            let languages = caption_languages();
            Arc::new(move |url: &str, cancel: &Cancel| crate::extract::extract_url(&*http, url, &languages, cancel))
        });
        let engine = Engine {
            data_dir: data_dir.to_path_buf(),
            db,
            settings: RwLock::new(options.settings.unwrap_or_else(|| AppSettings::load(data_dir))),
            secrets: Secrets::new(data_dir),
            http,
            providers,
            extract_url,
            clock: options.clock.unwrap_or_else(|| Arc::new(OffsetDateTime::now_utc)),
            running: Mutex::default(),
            generation: AtomicU64::new(0),
            jobs: Arc::new(jobs::Jobs::new(SOURCE_CONCURRENCY)),
        };
        engine.recover()?;
        engine.seed_transformations()?;
        Ok(LocalBackend(Arc::new(engine)))
    }
}

impl Engine {
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The HTTP client, for features built on this backend (transcription, YouTube).
    pub fn http(&self) -> &SharedHttp {
        &self.http
    }

    /// Re-reads `settings.json`, after the app saves new model choices.
    pub fn reload_settings(&self) {
        self.set_settings(AppSettings::load(&self.data_dir));
    }

    /// Replaces the settings in memory. Keys are read from `keys.json` on every call.
    pub fn set_settings(&self, settings: AppSettings) {
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = settings;
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Waits until no source or insight is being processed. Used by tests and before quitting.
    pub fn idle(&self) {
        self.jobs.wait_idle();
    }

    fn recover(&self) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE source SET status = 'failed', error = ?1 WHERE status = 'processing'",
            ["Processing was interrupted when the app closed. Retry to continue."],
        )?;
        conn.execute(
            "UPDATE podcast_episode SET status = 'failed', error = ?1 WHERE status NOT IN ('completed', 'failed')",
            ["The app closed before this episode finished. Delete it and create it again."],
        )?;
        conn.execute(
            "UPDATE source_insight SET status = 'failed', error = ?1 WHERE status = 'running'",
            ["The app closed before this insight finished. Delete it and run the transformation again."],
        )?;
        Ok(())
    }

    /// `new Date().toISOString()`.
    pub(crate) fn now(&self) -> String {
        iso((self.clock)())
    }

    pub(crate) fn now_plus_ms(&self, ms: i64) -> String {
        iso((self.clock)() + time::Duration::milliseconds(ms))
    }

    pub(crate) fn id(&self, prefix: &str) -> String {
        format!("{prefix}:{}", uuid::Uuid::new_v4())
    }

    // Models and keys

    pub fn ai_config(&self) -> Result<AiConfig> {
        let settings = self.settings();
        let mut providers = BTreeMap::new();
        for kind in ProviderKind::ALL {
            providers.insert(kind, self.provider_config(kind, &settings)?);
        }
        let choice = |m: &Option<ModelSetting>| -> Option<ModelChoice> {
            let m = m.as_ref().filter(|m| !m.model.trim().is_empty())?;
            let kind = ProviderKind::parse(&m.provider)?;
            Some(ModelChoice { provider: providers[&kind].clone(), model: m.model.clone() })
        };
        Ok(AiConfig {
            chat: choice(&settings.chat),
            embedding: choice(&settings.embedding),
            transformation: choice(&settings.transformation),
            transcription: choice(&settings.transcription),
            providers: providers.clone(),
        })
    }

    fn provider_config(&self, kind: ProviderKind, settings: &AppSettings) -> Result<ProviderConfig> {
        Ok(ProviderConfig {
            kind,
            api_key: self.secrets.get(&provider_secret(kind.as_str()))?,
            base_url: (kind == ProviderKind::OpenAiCompatible).then(|| settings.compatible_base_url.clone()),
        })
    }

    /// A chat call recorded in the usage log under a feature name.
    pub(crate) fn chat_with(&self, choice: &ModelChoice, feature: &str, mut request: ChatRequest<'_>) -> Result<String> {
        request.model = choice.model.clone();
        let input = request.system.chars().count() + request.messages.iter().map(|m| m.content.chars().count()).sum::<usize>();
        let reply = (self.providers)(&choice.provider).chat(request)?;
        self.record_usage(feature, &choice.model, tokens(input), tokens(reply.chars().count()), 0.0);
        Ok(reply)
    }

    /// Embeddings, recorded in the usage log. Checks the provider returned one vector per text.
    pub(crate) fn embed_with(&self, choice: &ModelChoice, feature: &str, texts: &[String], cancel: &Cancel) -> Result<Vec<Vec<f32>>> {
        let vectors = (self.providers)(&choice.provider).embed(&choice.model, texts, cancel)?;
        if vectors.len() != texts.len() {
            return Err(Error::other(format!("The embedding model returned {} vectors for {} texts", vectors.len(), texts.len())));
        }
        self.record_usage(feature, &choice.model, tokens(texts.iter().map(|t| t.chars().count()).sum()), 0, 0.0);
        Ok(vectors)
    }

    /// Logs a call for the Usage view. A failure to log never fails the call.
    pub fn record_usage(&self, feature: &str, model: &str, input_tokens: u64, output_tokens: u64, audio_seconds: f64) {
        let _ = self.db.lock().execute(
            "INSERT INTO usage_event (id, created, feature, model, input_tokens, output_tokens, audio_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![self.id("usage"), self.now(), feature, model, input_tokens as i64, output_tokens as i64, audio_seconds],
        );
    }

    // Background work

    /// Registers a cancel for background work under `key`; returns it with its generation.
    fn start_running(&self, key: &str) -> (u64, Cancel) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let cancel = Cancel::new();
        self.running.lock().unwrap_or_else(|e| e.into_inner()).insert(key.into(), (generation, cancel.clone()));
        (generation, cancel)
    }

    fn finish_running(&self, key: &str, generation: u64) {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.get(key).is_some_and(|(g, _)| *g == generation) {
            running.remove(key);
        }
    }

    fn cancel_running(&self, key: &str) {
        if let Some((_, cancel)) = self.running.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
            cancel.cancel();
        }
    }
}

/// Preferred caption languages for YouTube links: the system language, then English
/// (the Tauri app uses the browser's languages).
fn caption_languages() -> Vec<String> {
    let mut out = Vec::new();
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(var) {
            let tag = value.split(['.', '@']).next().unwrap_or("").replace('_', "-");
            if !tag.is_empty() && tag != "C" && tag != "POSIX" {
                out.push(tag);
                break;
            }
        }
    }
    if !out.iter().any(|l| l == "en") {
        out.push("en".into());
    }
    out
}

/// Text length to estimated tokens, as the TypeScript engine counts them.
fn tokens(chars: usize) -> u64 {
    chars.div_ceil(4) as u64
}

const ISO: &[time::format_description::FormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

/// A time as `Date.toISOString()` writes it.
pub fn iso(t: OffsetDateTime) -> String {
    t.to_offset(time::UtcOffset::UTC).format(ISO).unwrap_or_default()
}

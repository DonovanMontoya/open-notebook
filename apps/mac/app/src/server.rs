//! The server admin page (connected mode): which model the server uses for each job, its
//! models, its API keys and its background jobs. These calls go to `RemoteBackend::client`,
//! which the `Backend` trait doesn't cover.

use std::collections::BTreeMap;
use std::sync::Arc;

use notebook_engine::remote::RemoteBackend;
use notebook_engine::remote::{NewCredential, ServerCredential, ServerJob};
use notebook_engine::types::ServerModel;
use rune::TextEdit;

use crate::state::App;

/// Default model slots: setting key, label, model type.
pub const SLOTS: [(&str, &str, &str); 7] = [
    ("default_chat_model", "Chat", "language"),
    ("default_transformation_model", "Transformations and insights", "language"),
    ("large_context_model", "Large documents", "language"),
    ("default_tools_model", "Tools", "language"),
    ("default_embedding_model", "Embedding", "embedding"),
    ("default_text_to_speech_model", "Text to speech", "text_to_speech"),
    ("default_speech_to_text_model", "Speech to text", "speech_to_text"),
];

/// Model types, in the order the Models section groups them, with their labels.
pub const MODEL_TYPES: [(&str, &str); 4] =
    [("language", "Language"), ("embedding", "Embedding"), ("text_to_speech", "Text to speech"), ("speech_to_text", "Speech to text")];

pub fn type_label(kind: &str) -> &str {
    MODEL_TYPES.iter().find(|(k, _)| *k == kind).map_or(kind, |(_, l)| l)
}

/// Providers a new server key can be for.
pub const KEY_PROVIDERS: [&str; 12] =
    ["openai", "anthropic", "google", "groq", "mistral", "deepseek", "xai", "openrouter", "voyage", "elevenlabs", "ollama", "openai_compatible"];

/// Processing choices: setting key, label and (value, label) options.
/// (value, label) choices for a setting.
pub type Options = &'static [(&'static str, &'static str)];

pub const PROCESSING: [(&str, &str, Options); 4] = [
    ("default_content_processing_engine_doc", "Documents", &[("auto", "Automatic"), ("docling", "Docling"), ("simple", "Simple")]),
    (
        "default_content_processing_engine_url",
        "Web pages",
        &[("auto", "Automatic"), ("firecrawl", "Firecrawl"), ("jina", "Jina"), ("crawl4ai", "Crawl4AI"), ("simple", "Simple")],
    ),
    ("default_embedding_option", "Index new sources", &[("ask", "Ask each time"), ("always", "Always"), ("never", "Never")]),
    ("auto_delete_files", "Delete uploads after reading", &[("yes", "Yes"), ("no", "No")]),
];

/// Processing switches: setting key and label.
pub const PROCESSING_SWITCHES: [(&str, &str); 3] =
    [("docling_ocr", "Read scanned pages (OCR)"), ("docling_formulas", "Read formulas"), ("docling_vision", "Describe images")];

pub const CAPTION_LANGUAGES: &str = "youtube_preferred_languages";

/// Job statuses that can still be cancelled.
pub fn cancellable(status: &str) -> bool {
    matches!(status, "new" | "queued" | "running" | "pending")
}

/// "process_source" or "commands:embed_note" → "process source", "commands embed note".
pub fn job_label(j: &ServerJob) -> String {
    let raw = j.command.as_deref().or(j.name.as_deref()).unwrap_or("Job");
    raw.split(['_', ':']).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// "en, pt-BR , ,de" → ["en", "pt-BR", "de"].
pub fn parse_languages(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

pub struct NewKey {
    pub name: TextEdit,
    pub provider: Option<String>,
    pub api_key: TextEdit,
    pub base_url: TextEdit,
    /// What the key is used for; at least one.
    pub modalities: Vec<String>,
}

impl Default for NewKey {
    fn default() -> Self {
        NewKey {
            name: TextEdit::new(""),
            provider: Some("openai".into()),
            api_key: TextEdit::new(""),
            base_url: TextEdit::new(""),
            modalities: vec!["language".into()],
        }
    }
}

impl NewKey {
    pub fn ready(&self) -> bool {
        self.provider.is_some() && !self.name.text.trim().is_empty() && !self.modalities.is_empty()
    }
}

/// A model added by hand.
pub struct NewModel {
    pub provider: Option<String>,
    pub kind: String,
    pub name: TextEdit,
}

impl Default for NewModel {
    fn default() -> Self {
        NewModel { provider: None, kind: "language".into(), name: TextEdit::new("") }
    }
}

#[derive(Default)]
pub struct ServerPage {
    pub models: Vec<ServerModel>,
    pub defaults: BTreeMap<String, Option<String>>,
    pub credentials: Vec<ServerCredential>,
    pub providers: Vec<String>,
    pub jobs: Vec<ServerJob>,
    /// The Select that is open: a slot key, or "provider" in the new-key form.
    pub open_select: Option<String>,
    pub adding: Option<NewKey>,
    /// Results of tests and actions, by model or credential id ("rebuild" for the index).
    pub notes: BTreeMap<String, String>,
    /// Model types each provider supports, for adding a model by hand.
    pub supported_types: BTreeMap<String, Vec<String>>,
    pub new_model: NewModel,
    /// The server's processing settings, once loaded.
    pub settings: Option<serde_json::Map<String, serde_json::Value>>,
    pub caption_languages: TextEdit,
    /// Bumped on each load, so an older jobs refresh timer stops.
    pub jobs_timer: u64,
}

impl App {
    /// The server client, in connected mode.
    pub fn remote(&self) -> Option<Arc<RemoteBackend>> {
        self.remote.clone()
    }

    fn admin<T: Send + 'static>(
        &self,
        work: impl FnOnce(&RemoteBackend) -> notebook_engine::Result<T> + Send + 'static,
        done: impl FnOnce(&mut ServerPage, T) + Send + 'static,
    ) {
        let Some(remote) = self.remote() else { return };
        let inbox = self.inbox();
        std::thread::spawn(move || {
            let result = work(&remote);
            inbox.send(move |app| match result {
                Ok(v) => {
                    if let Some(p) = app.server.as_mut() {
                        done(p, v);
                    }
                }
                Err(e) => app.fail(e),
            });
        });
    }

    pub fn load_server(&mut self) {
        self.server.get_or_insert_with(ServerPage::default);
        self.admin(|r| r.client().list_models(), |p, m| p.models = m);
        self.admin(|r| r.client().model_defaults(), |p, d| p.defaults = d);
        self.admin(|r| r.client().list_credentials(), |p, c| p.credentials = c);
        self.admin(
            |r| r.client().model_providers(),
            |p, x| {
                p.providers = x.available;
                p.supported_types = x.supported_types;
            },
        );
        self.admin(|r| r.client().settings(), apply_settings);
        self.load_jobs();
        let timer = self.server.as_mut().map(|p| {
            p.jobs_timer += 1;
            p.jobs_timer
        });
        if let Some(t) = timer {
            self.refresh_jobs_after(t);
        }
    }

    /// Reloads the jobs every five seconds while the server page is open.
    fn refresh_jobs_after(&self, timer: u64) {
        let inbox = self.inbox();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            inbox.send(move |app| {
                if app.route == crate::state::Route::Server && app.server.as_ref().is_some_and(|p| p.jobs_timer == timer) {
                    app.load_jobs();
                    app.refresh_jobs_after(timer);
                }
            });
        });
    }

    /// Changes one processing setting and sends them all, as the server expects.
    pub fn set_server_setting(&mut self, key: &str, value: serde_json::Value) {
        let Some(p) = self.server.as_mut() else { return };
        p.open_select = None;
        let Some(all) = p.settings.as_mut() else { return };
        all.insert(key.into(), value);
        let all = all.clone();
        self.admin(move |r| r.client().update_settings(all), apply_settings);
    }

    pub fn save_caption_languages(&mut self) {
        let Some(text) = self.server.as_ref().map(|p| p.caption_languages.text.clone()) else { return };
        let list = parse_languages(&text).into_iter().map(serde_json::Value::String).collect();
        self.set_server_setting(CAPTION_LANGUAGES, serde_json::Value::Array(list));
    }

    pub fn add_model(&mut self) {
        let Some(p) = self.server.as_mut() else { return };
        let name = p.new_model.name.text.trim().to_owned();
        let Some(provider) = p.new_model.provider.clone().filter(|_| !name.is_empty()) else { return };
        let kind = p.new_model.kind.clone();
        p.new_model.name = TextEdit::new("");
        self.admin(move |r| r.client().create_model(&name, &provider, &kind).and_then(|()| r.client().list_models()), |p, m| p.models = m);
    }

    pub fn load_jobs(&self) {
        self.admin(|r| r.client().list_jobs(), |p, j| p.jobs = j);
    }

    pub fn set_default(&mut self, slot: &'static str, model: Option<String>) {
        let Some(p) = self.server.as_mut() else { return };
        p.open_select = None;
        p.defaults.insert(slot.into(), model);
        let all = p.defaults.clone();
        self.admin(move |r| r.client().set_model_defaults(&all), |_, ()| {});
    }

    pub fn test_model(&self, id: String) {
        let key = id.clone();
        self.admin(
            move |r| r.client().test_model(&id),
            move |p, t| {
                let ok = t.success.unwrap_or(false);
                p.notes.insert(key, t.message.unwrap_or_else(|| if ok { "Works".into() } else { "Failed".into() }));
            },
        );
    }

    pub fn delete_model(&self, id: String) {
        self.admin(move |r| r.client().delete_model(&id), |_, ()| {});
        self.admin(|r| r.client().list_models(), |p, m| p.models = m);
    }

    pub fn sync_models(&self) {
        self.admin(|r| r.client().sync_models().and_then(|()| r.client().list_models()), |p, m| p.models = m);
    }

    pub fn add_credential(&mut self) {
        if !self.server.as_ref().and_then(|p| p.adding.as_ref()).is_some_and(NewKey::ready) {
            return;
        }
        let Some(k) = self.server.as_mut().and_then(|p| p.adding.take()) else { return };
        let Some(provider) = k.provider else { return };
        let data = NewCredential {
            name: k.name.text.trim().to_owned(),
            provider,
            modalities: k.modalities,
            api_key: Some(k.api_key.text.trim().to_owned()).filter(|v| !v.is_empty()),
            base_url: Some(k.base_url.text.trim().to_owned()).filter(|v| !v.is_empty()),
        };
        self.admin(move |r| r.client().create_credential(&data).and_then(|_| r.client().list_credentials()), |p, c| p.credentials = c);
    }

    pub fn test_credential(&self, id: String) {
        let key = id.clone();
        self.admin(
            move |r| r.client().test_credential(&id),
            move |p, t| {
                let ok = t.success.unwrap_or(false);
                p.notes.insert(key, t.message.unwrap_or_else(|| if ok { "Key works".into() } else { "Key failed".into() }));
            },
        );
    }

    pub fn register_models(&self, id: String) {
        let key = id.clone();
        self.admin(
            move |r| r.client().register_credential_models(&id).and_then(|()| r.client().list_models()),
            move |p, m| {
                p.models = m;
                p.notes.insert(key, "Models added".into());
            },
        );
    }

    pub fn delete_credential(&self, id: String) {
        self.admin(move |r| r.client().delete_credential(&id).and_then(|()| r.client().list_credentials()), |p, c| p.credentials = c);
    }

    pub fn cancel_job(&self, id: String) {
        self.admin(move |r| r.client().cancel_job(&id).and_then(|()| r.client().list_jobs()), |p, j| p.jobs = j);
    }

    pub fn rebuild_index(&self) {
        self.admin(
            |r| r.client().rebuild_embeddings(true),
            |p, started| {
                p.notes.insert("rebuild".into(), started.message.unwrap_or_else(|| "Started".into()));
            },
        );
    }
}

/// Shows processing settings the server sent.
pub fn apply_settings(p: &mut ServerPage, settings: serde_json::Map<String, serde_json::Value>) {
    let langs: Vec<&str> =
        settings.get(CAPTION_LANGUAGES).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    p.caption_languages = TextEdit::new(langs.join(", "));
    p.settings = Some(settings);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_labels_are_tidied_and_prefer_the_command() {
        let job = |command: Option<&str>, name: Option<&str>| ServerJob {
            job_id: None,
            id: None,
            command: command.map(Into::into),
            name: name.map(Into::into),
            status: "running".into(),
            created: None,
            updated: None,
            error_message: None,
        };
        assert_eq!(job_label(&job(Some("open_notebook:process_source"), Some("x"))), "open notebook process source");
        assert_eq!(job_label(&job(None, Some("embed__note"))), "embed note");
        assert_eq!(job_label(&job(None, None)), "Job");
        assert!(cancellable("queued") && !cancellable("completed"));
    }

    #[test]
    fn caption_languages_parse_and_settings_fill_the_field() {
        assert_eq!(parse_languages("en, pt-BR , ,de"), ["en", "pt-BR", "de"]);
        let mut p = ServerPage::default();
        apply_settings(&mut p, serde_json::from_value(serde_json::json!({"youtube_preferred_languages": ["en", "es"]})).unwrap());
        assert_eq!(p.caption_languages.text, "en, es");
    }

    #[test]
    fn a_new_key_needs_a_name_and_a_use() {
        let mut k = NewKey::default();
        assert!(!k.ready());
        k.name = TextEdit::new("Work");
        assert!(k.ready());
        k.modalities.clear();
        assert!(!k.ready());
    }
}

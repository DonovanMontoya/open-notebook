//! Settings operations and catalog-backed model choices.
use std::{collections::BTreeMap, sync::Arc};

use notebook_engine::{
    net::{SharedHttp, UreqHttp},
    providers::{Capability, ModelInfo, ProviderKind, create_provider},
    remote::RemoteBackend,
    settings::{self, Mode},
};
use rune::TextEdit;

use crate::state::{App, Check, MODEL_ROLES, ModelList};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    Connection,
    Keys,
    Models,
    Preferences,
}
impl SettingsTab {
    pub const ALL: [Self; 4] = [Self::Connection, Self::Keys, Self::Models, Self::Preferences];
    pub fn label(self) -> &'static str {
        match self {
            Self::Connection => "Connection",
            Self::Keys => "API keys",
            Self::Models => "Models",
            Self::Preferences => "Preferences",
        }
    }
}

fn preferred(role: &str, kind: ProviderKind) -> &'static [&'static str] {
    use ProviderKind::*;
    match (role, kind) {
        ("chat" | "transformation", Anthropic) => &["claude-sonnet-5-5", "claude-sonnet-4-6", "claude-sonnet-4-5"],
        ("chat" | "transformation", OpenAi) => &["gpt-6.1-terra", "gpt-6.1-sol", "gpt-6-terra", "gpt-6-sol", "gpt-5.4", "gpt-4.1"],
        ("chat" | "transformation" | "transcription", Gemini) => &["gemini-3.8-flash", "gemini-3.6-flash", "gemini-2.5-flash"],
        ("embedding", OpenAi | OpenAiCompatible) => &["text-embedding-3-small", "text-embedding-3-large"],
        ("embedding", Gemini) => &["gemini-embedding-001", "gemini-embedding-2"],
        ("transcription", OpenAi | OpenAiCompatible) => &["gpt-transcribe", "gpt-4o-transcribe", "gpt-4o-mini-transcribe", "whisper-1"],
        ("chat" | "transformation", OpenAiCompatible) => &["claude-sonnet-5-5", "gpt-6.1-terra", "gpt-6.1-sol", "gemini-3.8-flash"],
        _ => &[],
    }
}

fn supports(role: &str, kind: ProviderKind, model: &ModelInfo) -> bool {
    let id = model.id.to_ascii_lowercase();
    if ["preview", "experimental", "deprecated", "-exp", ":free"].iter().any(|s| id.contains(s)) {
        return false;
    }
    match role {
        "embedding" => model.capabilities.contains(&Capability::Embedding),
        "transcription" if kind != ProviderKind::Gemini => id.contains("transcribe") || id.contains("whisper"),
        _ => {
            model.capabilities.contains(&Capability::Chat)
                && !["image", "tts", "audio", "live", "realtime", "transcrib", "embedding", "codex", "search"].iter().any(|s| id.contains(s))
        }
    }
}

fn model_for(role: &str, kind: ProviderKind, models: &[ModelInfo]) -> Option<String> {
    let eligible: Vec<_> = models.iter().filter(|m| supports(role, kind, m)).collect();
    for preferred in preferred(role, kind) {
        if let Some(model) =
            eligible.iter().find(|m| m.id == *preferred || m.id.strip_prefix("models/") == Some(preferred) || m.id.rsplit('/').next() == Some(preferred))
        {
            return Some(model.id.clone());
        }
    }
    // A local endpoint often exposes just its one loaded model.
    (kind == ProviderKind::OpenAiCompatible && eligible.len() == 1).then(|| eligible[0].id.clone())
}

pub fn recommendations(catalogs: &BTreeMap<ProviderKind, Vec<ModelInfo>>) -> Vec<(&'static str, ProviderKind, String)> {
    MODEL_ROLES
        .into_iter()
        .filter_map(|role| {
            let order = if matches!(role, "chat" | "transformation") {
                [ProviderKind::Anthropic, ProviderKind::OpenAi, ProviderKind::Gemini, ProviderKind::OpenAiCompatible]
            } else {
                ProviderKind::ALL
            };
            order.into_iter().find_map(|kind| model_for(role, kind, catalogs.get(&kind)?).map(|id| (role, kind, id)))
        })
        .collect()
}

impl App {
    pub(crate) fn run_settings<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> notebook_engine::Result<T> + Send + 'static,
        done: impl FnOnce(&mut App, notebook_engine::Result<T>) + Send + 'static,
    ) {
        let inbox = self.inbox();
        std::thread::spawn(move || {
            let result = work();
            inbox.send(move |app| done(app, result));
        });
    }

    pub fn check_saved_keys(&mut self) {
        let Some(form) = &self.form else { return };
        let kinds: Vec<_> = ProviderKind::ALL.into_iter().filter(|&k| form.has_provider_key(k)).collect();
        let elevenlabs = form.has_key(&settings::provider_secret("elevenlabs"));
        for kind in kinds {
            self.load_models(kind, true);
        }
        if elevenlabs {
            self.save_and_check_elevenlabs();
        }
    }

    pub fn recommend_models(&mut self) {
        self.recommend_models_with(UreqHttp::shared());
    }

    pub(crate) fn recommend_models_with(&mut self, http: SharedHttp) {
        let compatible_url = self.settings.compatible_base_url.clone();
        let Some(form) = self.form.as_mut() else { return };
        if form.recommendation == Some(Check::Checking) {
            return;
        }
        let kinds: Vec<_> = ProviderKind::ALL
            .into_iter()
            .filter(|&k| {
                (k != ProviderKind::OpenAiCompatible || form.compatible_base_url.text.trim() == compatible_url)
                    && (form.has_provider_key(k) || (k == ProviderKind::OpenAiCompatible && matches!(form.checks.get(k.as_str()), Some(Check::Working(_)))))
            })
            .collect();
        if kinds.is_empty() {
            form.recommendation = Some(Check::Failed("Save a provider key on the API keys tab first. For a local model, check its connection there.".into()));
            return;
        }
        let ticket = form.begin("recommendations");
        form.recommendation = Some(Check::Checking);
        let configs: Vec<_> = kinds.into_iter().map(|k| self.provider_config(k)).collect();
        self.run_settings(
            move || {
                Ok(std::thread::scope(|scope| {
                    let jobs: Vec<_> = configs
                        .into_iter()
                        .map(|config| {
                            let http = http.clone();
                            scope.spawn(move || (config.kind, create_provider(&config, http).list_models()))
                        })
                        .collect();
                    jobs.into_iter().filter_map(|job| job.join().ok()).collect::<Vec<_>>()
                }))
            },
            move |app, result| {
                let Some(form) = app.form.as_mut().filter(|f| f.owns("recommendations", &ticket)) else { return };
                let mut catalogs = BTreeMap::new();
                for (kind, result) in result.unwrap_or_default() {
                    match result {
                        Ok(models) => {
                            form.checks.insert(kind.as_str().into(), Check::Working(format!("Verified · {} models available", models.len())));
                            form.models.insert(kind, ModelList::Ready(models.clone()));
                            catalogs.insert(kind, models);
                        }
                        Err(error) => {
                            form.checks.insert(kind.as_str().into(), Check::Failed(error.to_string()));
                            form.models.insert(kind, ModelList::Failed(error.to_string()));
                        }
                    }
                }
                let choices = recommendations(&catalogs);
                if choices.is_empty() {
                    form.recommendation =
                        Some(Check::Failed("No recommended models are available. Check your provider status on API keys, or choose a model manually.".into()));
                    return;
                }
                let count = choices.len();
                for (role, kind, name) in choices {
                    let (provider, model) = form.slot(role);
                    *provider = kind;
                    *model = TextEdit::new(name);
                }
                if app.persist_model_settings() {
                    let message = if count == MODEL_ROLES.len() {
                        "Recommended models selected and saved.".into()
                    } else {
                        format!("Saved {count} recommended models. Roles without a supported model were kept as they were.")
                    };
                    app.form.as_mut().unwrap().recommendation = Some(Check::Working(message));
                    app.toast = Some("Recommended models saved".into());
                    app.toast_since = None;
                } else {
                    app.form.as_mut().unwrap().recommendation =
                        Some(Check::Failed("Could not save the selections. See the save error below and try again.".into()));
                }
            },
        );
    }

    pub fn use_local_mode(&mut self) {
        let mut next = self.settings.clone();
        next.mode = Mode::Local;
        if let Err(error) = next.save(&self.data_dir) {
            if let Some(f) = &mut self.form {
                f.save = Some(Check::Failed(error.to_string()));
            }
            return;
        }
        let reconnect = self.settings.mode != Mode::Local;
        self.settings = next;
        if reconnect {
            self.open = None;
            self.library = None;
            self.ask = None;
            self.usage = None;
            self.transformations = None;
            self.notebooks.clear();
            self.connect();
        }
        if let Some(error) = self.backend_error.clone() {
            if let Some(f) = &mut self.form {
                f.save = Some(Check::Failed(error));
            }
            return;
        }
        if let Some(f) = &mut self.form {
            f.mode = Mode::Local;
            f.save = Some(Check::Working("On-device notebooks are active".into()));
        }
        self.toast = Some("Using on-device notebooks".into());
        self.toast_since = None;
    }

    pub fn save_and_test_server(&mut self) {
        self.save_and_test_server_with(UreqHttp::shared());
    }

    pub(crate) fn save_and_test_server_with(&mut self, http: SharedHttp) {
        let Some(form) = self.form.as_mut() else { return };
        if form.checks.get("server") == Some(&Check::Checking) {
            return;
        }
        let url = form.server_url.text.trim().to_owned();
        if url.is_empty() {
            form.checks.insert("server".into(), Check::Failed("Enter your Open Notebook server address.".into()));
            return;
        }
        if !form.server_password.text.trim().is_empty() {
            if let Err(error) = self.secrets.put(settings::SERVER_PASSWORD, &form.server_password.text, self.settings.sync_keys) {
                form.checks.insert("server".into(), Check::Failed(error.to_string()));
                return;
            }
            form.server_password = TextEdit::default();
            form.saved_keys.insert(settings::SERVER_PASSWORD.into());
        }
        let password = match self.secrets.get(settings::SERVER_PASSWORD) {
            Ok(password) => password,
            Err(error) => {
                form.checks.insert("server".into(), Check::Failed(error.to_string()));
                return;
            }
        };
        let ticket = form.begin("server");
        form.checks.insert("server".into(), Check::Checking);
        let remote = Arc::new(RemoteBackend::with_http(&url, password.as_deref(), http));
        self.run_settings(
            move || {
                remote.client().auth_status()?;
                let notebooks = remote.client().list_notebooks(None)?;
                Ok((remote, notebooks))
            },
            move |app, result| {
                if app.form.as_ref().is_none_or(|f| !f.owns("server", &ticket)) {
                    return;
                }
                let status = match result {
                    Ok((remote, notebooks)) => {
                        let mut next = app.settings.clone();
                        next.mode = Mode::Remote;
                        next.server_url = url;
                        match next.save(&app.data_dir) {
                            Err(error) => Check::Failed(format!("Server reached, but settings could not be saved: {error}")),
                            Ok(()) => {
                                let n = notebooks.len();
                                app.settings = next;
                                app.open = None;
                                app.library = None;
                                app.server = None;
                                app.ask = None;
                                app.usage = None;
                                app.transformations = None;
                                app.backend = Some(remote.clone());
                                app.remote = Some(remote);
                                app.backend_error = None;
                                app.notebooks = notebooks;
                                app.loaded = true;
                                app.toast = Some("Connected to server".into());
                                app.toast_since = None;
                                Check::Working(format!("Connected · {n} {}", if n == 1 { "notebook" } else { "notebooks" }))
                            }
                        }
                    }
                    Err(error) => Check::Failed(error.to_string()),
                };
                app.form.as_mut().unwrap().checks.insert("server".into(), status);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SettingsForm;
    use notebook_engine::{
        net::{Request, Response, Transport},
        providers::Capability,
        settings::{AppSettings, Secrets},
    };

    fn setup(keys: &[&str]) -> App {
        let mut app = App::offline();
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        app.data_dir =
            std::env::temp_dir().join(format!("notebook-settings-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&app.data_dir).unwrap();
        app.secrets = Secrets::new(&app.data_dir);
        app.settings.save(&app.data_dir).unwrap();
        for key in keys {
            app.secrets.set(&settings::provider_secret(key), "test-key").unwrap();
        }
        app.form = Some(SettingsForm::from(&app.settings, &app.secrets));
        app
    }
    fn settle(app: &mut App, done: impl Fn(&App) -> bool) {
        let start = std::time::Instant::now();
        loop {
            app.drain();
            if done(app) {
                return;
            }
            assert!(start.elapsed().as_secs() < 5, "settings operation did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn model(id: &str, capabilities: Vec<Capability>) -> ModelInfo {
        ModelInfo { id: id.into(), label: None, capabilities }
    }

    #[test]
    fn recommendations_use_supported_catalog_models_and_pair_providers_by_task() {
        let catalogs = BTreeMap::from([
            (ProviderKind::Anthropic, vec![model("claude-opus-5-5", vec![Capability::Chat]), model("claude-sonnet-5-5", vec![Capability::Chat])]),
            (
                ProviderKind::OpenAi,
                vec![
                    model("text-embedding-3-small", vec![Capability::Embedding]),
                    model("gpt-transcribe", vec![]),
                    model("gpt-6.1-terra", vec![Capability::Chat]),
                ],
            ),
        ]);
        let choices = recommendations(&catalogs);
        assert_eq!(choices.len(), 4);
        assert!(choices.contains(&("chat", ProviderKind::Anthropic, "claude-sonnet-5-5".into())));
        assert!(choices.contains(&("embedding", ProviderKind::OpenAi, "text-embedding-3-small".into())));
        assert!(choices.contains(&("transcription", ProviderKind::OpenAi, "gpt-transcribe".into())));
        let bad = BTreeMap::from([(
            ProviderKind::OpenAi,
            vec![model("gpt-6.1-terra-preview", vec![Capability::Chat]), model("image-3", vec![]), model("text-embedding-3-small", vec![])],
        )]);
        assert!(recommendations(&bad).is_empty());
    }

    #[test]
    fn recommendation_button_checks_saved_keys_and_saves_without_a_backend() {
        let mut app = setup(&["openai", "anthropic"]);
        let http = |request: Request| -> Result<Response, Transport> {
            assert_eq!(request.method, "GET");
            let value = if request.url.contains("anthropic.com") {
                serde_json::json!({"data":[{"id":"claude-sonnet-5-5"}],"has_more":false})
            } else {
                assert!(request.url.contains("api.openai.com"));
                serde_json::json!({"data":[{"id":"gpt-6.1-terra"},{"id":"text-embedding-3-small"},{"id":"gpt-transcribe"}]})
            };
            Ok(Response::new(200, value.to_string()))
        };
        app.recommend_models_with(Arc::new(http));
        assert_eq!(app.form.as_ref().unwrap().recommendation, Some(Check::Checking));
        settle(&mut app, |a| a.form.as_ref().unwrap().recommendation != Some(Check::Checking));
        let form = app.form.as_ref().unwrap();
        assert!(matches!(form.recommendation, Some(Check::Working(_))));
        assert!(matches!(form.checks.get("openai"), Some(Check::Working(_))));
        assert!(form.keys.iter().all(|(_, edit)| edit.text.is_empty()));
        let saved = AppSettings::load(&app.data_dir);
        assert_eq!(saved.chat.unwrap().model, "claude-sonnet-5-5");
        assert_eq!(saved.embedding.unwrap().model, "text-embedding-3-small");
        assert_eq!(saved.transcription.unwrap().model, "gpt-transcribe");
        assert_eq!(app.toast.as_deref(), Some("Recommended models saved"));
    }

    #[test]
    fn rejected_keys_do_not_produce_model_choices() {
        let mut app = setup(&["openai"]);
        app.recommend_models_with(Arc::new(|_: Request| Ok(Response::new(401, "{\"error\":{\"message\":\"Key rejected\"}}"))));
        settle(&mut app, |a| a.form.as_ref().unwrap().recommendation != Some(Check::Checking));
        assert!(matches!(app.form.as_ref().unwrap().recommendation, Some(Check::Failed(_))));
        assert!(app.settings.chat.is_none());
        assert!(app.form.as_ref().unwrap().has_provider_key(ProviderKind::OpenAi));
    }

    #[test]
    fn connect_verifies_before_switching_and_retains_the_connection_on_failure() {
        let mut app = setup(&[]);
        let form = app.form.as_mut().unwrap();
        form.mode = Mode::Remote;
        form.server_url = TextEdit::new("https://example.test");
        let http = |request: Request| -> Result<Response, Transport> {
            Ok(Response::new(
                200,
                if request.url.contains("/auth/status") {
                    "{\"auth_enabled\":false}"
                } else {
                    "[{\"id\":\"n\",\"name\":\"Server notebook\",\"created\":\"c\",\"updated\":\"u\"}]"
                },
            ))
        };
        app.save_and_test_server_with(Arc::new(http));
        assert_eq!(app.settings.mode, Mode::Local);
        assert_eq!(app.form.as_ref().unwrap().checks.get("server"), Some(&Check::Checking));
        settle(&mut app, |a| a.form.as_ref().unwrap().checks.get("server") != Some(&Check::Checking));
        assert_eq!(app.settings.mode, Mode::Remote);
        assert_eq!(AppSettings::load(&app.data_dir).server_url, "https://example.test");
        assert_eq!(app.notebooks[0].name, "Server notebook");
        let previous = app.backend.clone().unwrap();
        let form = app.form.as_mut().unwrap();
        form.connection_edited();
        form.server_url = TextEdit::new("https://bad.test");
        app.save_and_test_server_with(Arc::new(|_: Request| Ok(Response::new(401, "{\"detail\":\"Password rejected\"}"))));
        settle(&mut app, |a| a.form.as_ref().unwrap().checks.get("server") != Some(&Check::Checking));
        assert!(matches!(app.form.as_ref().unwrap().checks.get("server"), Some(Check::Failed(_))));
        assert!(Arc::ptr_eq(&previous, app.backend.as_ref().unwrap()));
        assert_eq!(app.settings.server_url, "https://example.test");
    }

    #[test]
    fn an_edited_address_ignores_the_previous_connection_result() {
        let mut app = setup(&[]);
        app.form.as_mut().unwrap().server_url = TextEdit::new("https://old.test");
        let (release, gate) = std::sync::mpsc::channel();
        let gate = std::sync::Mutex::new(gate);
        app.save_and_test_server_with(Arc::new(move |request: Request| {
            if request.url.contains("/auth/status") {
                gate.lock().unwrap().recv().unwrap();
            }
            Ok(Response::new(200, if request.url.contains("/auth/status") { "{\"auth_enabled\":false}" } else { "[]" }))
        }));
        let form = app.form.as_mut().unwrap();
        form.connection_edited();
        form.server_url = TextEdit::new("https://new.test");
        release.send(()).unwrap();
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.drain();
        }
        assert_eq!(app.settings.mode, Mode::Local);
        assert!(!app.form.as_ref().unwrap().checks.contains_key("server"));
    }

    #[test]
    fn save_failure_shows_an_error_instead_of_success() {
        let mut app = setup(&[]);
        app.data_dir = app.data_dir.join("missing-parent").join("missing-child");
        std::fs::write(app.data_dir.parent().unwrap(), b"file blocks directory").unwrap();
        app.form.as_mut().unwrap().chat_model = TextEdit::new("gpt-6.1-terra");
        app.save_settings();
        assert!(matches!(app.form.as_ref().unwrap().save, Some(Check::Failed(_))));
        assert!(app.settings.chat.is_none());
        assert!(app.toast.is_none());
    }
}

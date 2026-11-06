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

#[derive(Default)]
pub struct NewKey {
    pub name: TextEdit,
    pub provider: Option<String>,
    pub api_key: TextEdit,
    pub base_url: TextEdit,
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
        self.admin(|r| r.client().model_providers(), |p, x| p.providers = x.available);
        self.load_jobs();
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
        let Some(k) = self.server.as_mut().and_then(|p| p.adding.take()) else { return };
        let Some(provider) = k.provider else { return };
        let data = NewCredential {
            name: Some(k.name.text.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| provider.clone()),
            provider,
            modalities: Vec::new(),
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

//! App settings and secrets, in the app's data folder.
//!
//! The data folder is the Tauri app's (`~/Library/Application Support/com.donovanmontoya.opennotebook`),
//! so the Rune app opens the same database and `keys.json`. Settings were in the webview's
//! localStorage, which the Rune app can't read, so they start from the defaults in `settings.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backend::Result;

pub const APP_ID: &str = "com.donovanmontoya.opennotebook";

/// The app's data folder. `OPEN_NOTEBOOK_DATA_DIR` overrides it (tests, headless snapshots).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OPEN_NOTEBOOK_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(APP_ID)
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")).join(APP_ID)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSetting {
    /// "openai", "anthropic", "gemini" or "openai-compatible".
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Everything on this device with the user's API keys.
    #[default]
    Local,
    /// An Open Notebook server.
    Remote,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub mode: Mode,
    pub server_url: String,
    /// Base URL for the OpenAI-compatible provider (OpenRouter by default, or LM Studio, Ollama...).
    pub compatible_base_url: String,
    pub chat: Option<ModelSetting>,
    pub embedding: Option<ModelSetting>,
    /// Speech to text for audio and video sources.
    pub transcription: Option<ModelSetting>,
    /// Transformations, insights and study decks; the chat model when unset.
    pub transformation: Option<ModelSetting>,
    /// "system", "light" or "dark".
    pub appearance: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            mode: Mode::Local,
            server_url: "https://razer-blade.codlet-butterfly.ts.net:8502".into(),
            compatible_base_url: "https://openrouter.ai/api/v1".into(),
            chat: None,
            embedding: None,
            transcription: None,
            transformation: None,
            appearance: "system".into(),
        }
    }
}

impl AppSettings {
    pub fn load(dir: &Path) -> AppSettings {
        std::fs::read(dir.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        write_private(&dir.join("settings.json"), &serde_json::to_vec_pretty(self)?)
    }
}

/// Secret names, as the Tauri app wrote them.
pub fn provider_secret(kind: &str) -> String {
    format!("provider:{kind}")
}
pub const SERVER_PASSWORD: &str = "server:password";

/// Provider keys and the server password in `keys.json` (mode 0600), the file the Tauri app uses.
#[derive(Clone, Debug)]
pub struct Secrets {
    path: PathBuf,
}

impl Secrets {
    pub fn new(dir: &Path) -> Self {
        Secrets { path: dir.join("keys.json") }
    }

    fn load(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.load()?.get(name).cloned())
    }

    /// An empty value deletes the secret.
    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        let mut keys = self.load()?;
        let value = value.trim();
        if value.is_empty() {
            keys.remove(name);
        } else {
            keys.insert(name.into(), value.into());
        }
        write_private(&self.path, &serde_json::to_vec_pretty(&keys)?)
    }
}

/// Writes a temporary file readable only by this user and renames it over the old one.
fn write_private(target: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = target.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(&temp, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_round_trip_and_empty_deletes() {
        let dir = std::env::temp_dir().join(format!("on-secrets-{}", uuid::Uuid::new_v4()));
        let s = Secrets::new(&dir);
        assert_eq!(s.get("provider:openai").unwrap(), None);
        s.set("provider:openai", " sk-test ").unwrap();
        assert_eq!(s.get("provider:openai").unwrap().as_deref(), Some("sk-test"));
        s.set("provider:openai", "").unwrap();
        assert_eq!(s.get("provider:openai").unwrap(), None);
        std::fs::remove_dir_all(dir).ok();
    }
}

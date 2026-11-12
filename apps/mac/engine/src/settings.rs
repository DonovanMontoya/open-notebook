//! App settings and secrets, in the app's data folder.
//!
//! The data folder is the Tauri app's (`~/Library/Application Support/com.donovanmontoya.opennotebook`),
//! so the Rune app opens the same database and `keys.json`. Settings were in the webview's
//! localStorage, which the Rune app can't read, so they start from the defaults in `settings.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::backend::Result;
use crate::pricing::PriceOverrides;

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
    /// Keep API keys in iCloud Keychain so they follow the user to their other devices.
    pub sync_keys: bool,
    /// The user's own model prices, which win over the list in `pricing`.
    pub prices: PriceOverrides,
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
            sync_keys: false,
            prices: PriceOverrides::new(),
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

/// The model filled in when the user picks a provider for a job ("chat", "transformation",
/// "embedding" or "transcription"); empty when there is no obvious one.
pub fn default_model(role: &str, kind: crate::providers::ProviderKind) -> &'static str {
    use crate::providers::{DEFAULT_ANTHROPIC_MODEL, ProviderKind as K};
    match (role, kind) {
        ("chat" | "transformation", K::Anthropic) => DEFAULT_ANTHROPIC_MODEL,
        ("embedding", K::OpenAi) => "text-embedding-3-small",
        ("embedding", K::Gemini) => "gemini-embedding-001",
        ("transcription", K::OpenAi) => "gpt-4o-transcribe",
        ("transcription", K::Gemini) => "gemini-2.5-flash",
        ("transcription", K::OpenAiCompatible) => "whisper-1",
        _ => "",
    }
}

/// Every secret name the app uses: the server password, the four providers and ElevenLabs.
pub fn all_secret_names() -> Vec<String> {
    let mut names = vec![SERVER_PASSWORD.to_string()];
    names.extend(["openai", "anthropic", "gemini", "openai-compatible", "elevenlabs"].map(provider_secret));
    names
}

/// One place secrets can live: `keys.json` on this device, or iCloud Keychain.
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    /// Deleting a secret that isn't there is not an error.
    fn delete(&self, name: &str) -> Result<()>;
}

/// `keys.json` in the data folder, mode 0600, the file the Tauri app uses.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(dir: &Path) -> Self {
        FileStore { path: dir.join("keys.json") }
    }

    fn load(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }
}

impl SecretStore for FileStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.load()?.get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        let mut keys = self.load()?;
        keys.insert(name.into(), value.into());
        write_private(&self.path, &serde_json::to_vec_pretty(&keys)?)
    }

    fn delete(&self, name: &str) -> Result<()> {
        let mut keys = self.load()?;
        if keys.remove(name).is_some() {
            write_private(&self.path, &serde_json::to_vec_pretty(&keys)?)?;
        }
        Ok(())
    }
}

/// The error text a keychain call gives when the app isn't signed for iCloud Keychain.
pub const MISSING_ENTITLEMENT: &str = "missing-entitlement";

/// The synced half of the data protection keychain: iCloud Keychain. It never touches the
/// login keychain, so it never asks for permission. Writing needs an app signed with a
/// keychain-access-groups entitlement; ad-hoc builds get [`MISSING_ENTITLEMENT`].
pub struct Keychain;

#[cfg(all(target_os = "macos", not(test)))]
mod keychain {
    use security_framework::base::Error as SecError;
    use security_framework::passwords::{delete_generic_password_options, generic_password, set_generic_password_options};
    use security_framework::passwords_options::PasswordOptions;

    use crate::Error;
    use crate::backend::Result;

    const ERR_NOT_FOUND: i32 = -25300; // errSecItemNotFound
    const ERR_MISSING_ENTITLEMENT: i32 = -34018; // errSecMissingEntitlement

    fn options(name: &str) -> PasswordOptions {
        let mut o = PasswordOptions::new_generic_password(super::APP_ID, name);
        o.use_protected_keychain();
        o.set_access_synchronized(Some(true));
        o
    }

    fn describe(e: SecError) -> Error {
        if e.code() == ERR_MISSING_ENTITLEMENT { Error::other(super::MISSING_ENTITLEMENT) } else { Error::other(e.to_string()) }
    }

    fn ignorable(e: &SecError) -> bool {
        e.code() == ERR_NOT_FOUND || e.code() == ERR_MISSING_ENTITLEMENT
    }

    pub fn get(name: &str) -> Result<Option<String>> {
        match generic_password(options(name)) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if ignorable(&e) => Ok(None),
            Err(e) => Err(describe(e)),
        }
    }

    pub fn set(name: &str, value: &str) -> Result<()> {
        set_generic_password_options(value.as_bytes(), options(name)).map_err(describe)
    }

    pub fn delete(name: &str) -> Result<()> {
        match delete_generic_password_options(options(name)) {
            Err(e) if !ignorable(&e) => Err(describe(e)),
            _ => Ok(()),
        }
    }

    pub fn available() -> bool {
        let probe = "sync-probe";
        let ok = set_generic_password_options(b"1", options(probe)).is_ok();
        let _ = delete_generic_password_options(options(probe));
        ok
    }
}

/// Other platforms and tests: no iCloud Keychain, as an unsigned build sees it.
#[cfg(not(all(target_os = "macos", not(test))))]
mod keychain {
    use crate::Error;
    use crate::backend::Result;

    pub fn get(_name: &str) -> Result<Option<String>> {
        Ok(None)
    }
    pub fn set(_name: &str, _value: &str) -> Result<()> {
        Err(Error::other(super::MISSING_ENTITLEMENT))
    }
    pub fn delete(_name: &str) -> Result<()> {
        Ok(())
    }
    pub fn available() -> bool {
        false
    }
}

impl Keychain {
    /// Whether this build can write to iCloud Keychain (it is signed for it).
    pub fn available() -> bool {
        keychain::available()
    }
}

impl SecretStore for Keychain {
    fn get(&self, name: &str) -> Result<Option<String>> {
        keychain::get(name)
    }
    fn set(&self, name: &str, value: &str) -> Result<()> {
        keychain::set(name, value)
    }
    fn delete(&self, name: &str) -> Result<()> {
        keychain::delete(name)
    }
}

/// Why keys couldn't move between stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncError {
    /// The build isn't signed for iCloud Keychain.
    Unsigned,
    Failed(String),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::Unsigned => f.write_str(
                "This build is not signed for iCloud Keychain yet, so keys stay in the app on this Mac. Sync turns on once the app is signed with your developer team.",
            ),
            SyncError::Failed(message) => write!(f, "Keys could not be moved: {message}"),
        }
    }
}

/// Copies each named secret from `from` to `to`, then deletes it from `from`. A key is only
/// deleted once its new copy is written and reads back the same, so a failure part way
/// leaves every key in at least one store. Returns how many moved.
pub fn move_secrets(names: &[String], from: &dyn SecretStore, to: &dyn SecretStore) -> std::result::Result<usize, SyncError> {
    let fail = |e: crate::Error| {
        let message = e.to_string();
        if message.contains(MISSING_ENTITLEMENT) { SyncError::Unsigned } else { SyncError::Failed(message) }
    };
    let mut moved = 0;
    for name in names {
        let Some(value) = from.get(name).map_err(fail)? else { continue };
        to.set(name, &value).map_err(fail)?;
        if to.get(name).map_err(fail)?.as_deref() != Some(value.as_str()) {
            return Err(SyncError::Failed(format!("{name} did not read back after writing")));
        }
        from.delete(name).map_err(fail)?;
        moved += 1;
    }
    Ok(moved)
}

/// Provider keys and the server password. They live in `keys.json` on this device, or in
/// iCloud Keychain when the user turns sync on (`AppSettings::sync_keys`). Reads look on
/// the device first, then in the keychain, as the Tauri app does.
#[derive(Clone)]
pub struct Secrets {
    dir: PathBuf,
    device: Arc<dyn SecretStore>,
    synced: Arc<dyn SecretStore>,
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets").field("dir", &self.dir).finish_non_exhaustive()
    }
}

impl Secrets {
    pub fn new(dir: &Path) -> Self {
        Self::with_stores(dir, Arc::new(FileStore::new(dir)), Arc::new(Keychain))
    }

    /// Secrets over given stores (tests). `dir` holds `settings.json`, which says whether sync is on.
    pub fn with_stores(dir: &Path, device: Arc<dyn SecretStore>, synced: Arc<dyn SecretStore>) -> Self {
        Secrets { dir: dir.to_path_buf(), device, synced }
    }

    pub fn get(&self, name: &str) -> Result<Option<String>> {
        match self.device.get(name)? {
            Some(v) => Ok(Some(v)),
            None => self.synced.get(name),
        }
    }

    /// Saves to the store the saved settings pick. An empty value deletes the secret.
    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        self.put(name, value, AppSettings::load(&self.dir).sync_keys)
    }

    /// Saves to iCloud Keychain (`sync`) or this device, then removes the copy in the other
    /// store so a key lives in one place. An empty value deletes the secret from both.
    pub fn put(&self, name: &str, value: &str, sync: bool) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            return self.delete(name);
        }
        let (to, other) = if sync { (&self.synced, &self.device) } else { (&self.device, &self.synced) };
        to.set(name, value)?;
        other.delete(name)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        self.device.delete(name)?;
        self.synced.delete(name)
    }

    /// Moves every key to iCloud Keychain (`enabled`) or back to this device. The caller
    /// saves `sync_keys` only when this succeeds.
    pub fn set_sync(&self, enabled: bool, keychain_available: bool) -> std::result::Result<usize, SyncError> {
        if enabled && !keychain_available {
            return Err(SyncError::Unsigned);
        }
        let names = all_secret_names();
        if enabled { move_secrets(&names, &*self.device, &*self.synced) } else { move_secrets(&names, &*self.synced, &*self.device) }
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

    /// A store in memory that can be told to fail writes or deletes.
    #[derive(Default)]
    struct Memory {
        keys: std::sync::Mutex<BTreeMap<String, String>>,
        fail_set: Option<&'static str>,
        fail_delete: bool,
    }

    impl SecretStore for Memory {
        fn get(&self, name: &str) -> Result<Option<String>> {
            Ok(self.keys.lock().unwrap().get(name).cloned())
        }
        fn set(&self, name: &str, value: &str) -> Result<()> {
            if let Some(e) = self.fail_set {
                return Err(crate::Error::other(e));
            }
            self.keys.lock().unwrap().insert(name.into(), value.into());
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<()> {
            if self.fail_delete {
                return Err(crate::Error::other("locked"));
            }
            self.keys.lock().unwrap().remove(name);
            Ok(())
        }
    }

    fn memory(keys: &[(&str, &str)]) -> Memory {
        Memory { keys: std::sync::Mutex::new(keys.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()), ..Default::default() }
    }

    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!("on-sync-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn turning_sync_on_moves_every_key_to_the_keychain_and_back() {
        let (device, synced) = (Arc::new(memory(&[("provider:openai", "sk-1"), (SERVER_PASSWORD, "pw")])), Arc::new(memory(&[])));
        let s = Secrets::with_stores(&temp(), device.clone(), synced.clone());
        assert_eq!(s.set_sync(true, true), Ok(2));
        assert!(device.keys.lock().unwrap().is_empty());
        assert_eq!(synced.get("provider:openai").unwrap().as_deref(), Some("sk-1"));
        assert_eq!(s.get(SERVER_PASSWORD).unwrap().as_deref(), Some("pw"));
        assert_eq!(s.set_sync(false, true), Ok(2));
        assert!(synced.keys.lock().unwrap().is_empty());
        assert_eq!(device.get("provider:openai").unwrap().as_deref(), Some("sk-1"));
    }

    #[test]
    fn an_unsigned_build_keeps_its_keys_on_the_device() {
        let device = Arc::new(memory(&[("provider:openai", "sk-1")]));
        let s = Secrets::with_stores(&temp(), device.clone(), Arc::new(memory(&[])));
        assert_eq!(s.set_sync(true, false), Err(SyncError::Unsigned));
        let unsigned = Arc::new(Memory { fail_set: Some(MISSING_ENTITLEMENT), ..Default::default() });
        let s = Secrets::with_stores(&temp(), device.clone(), unsigned);
        assert_eq!(s.set_sync(true, true), Err(SyncError::Unsigned));
        assert_eq!(device.get("provider:openai").unwrap().as_deref(), Some("sk-1"));
    }

    #[test]
    fn a_failed_move_never_loses_a_key() {
        // Writes work, deletes fail: the key is in both stores, never in neither.
        let device = Arc::new(Memory { fail_delete: true, ..memory(&[("provider:gemini", "g")]) });
        let synced = Arc::new(memory(&[]));
        let s = Secrets::with_stores(&temp(), device.clone(), synced.clone());
        assert!(matches!(s.set_sync(true, true), Err(SyncError::Failed(_))));
        assert_eq!(device.get("provider:gemini").unwrap().as_deref(), Some("g"));
        assert_eq!(synced.get("provider:gemini").unwrap().as_deref(), Some("g"));
        // Writes fail: the key stays where it was.
        let device = Arc::new(memory(&[("provider:gemini", "g")]));
        let s = Secrets::with_stores(&temp(), device.clone(), Arc::new(Memory { fail_set: Some("disk full"), ..Default::default() }));
        assert_eq!(s.set_sync(true, true), Err(SyncError::Failed("disk full".into())));
        assert_eq!(s.get("provider:gemini").unwrap().as_deref(), Some("g"));
    }

    #[test]
    fn put_keeps_one_copy_in_the_chosen_store() {
        let (device, synced) = (Arc::new(memory(&[("provider:openai", "old")])), Arc::new(memory(&[])));
        let s = Secrets::with_stores(&temp(), device.clone(), synced.clone());
        s.put("provider:openai", " new ", true).unwrap();
        assert_eq!(device.get("provider:openai").unwrap(), None);
        assert_eq!(s.get("provider:openai").unwrap().as_deref(), Some("new"));
        s.put("provider:openai", "", true).unwrap();
        assert_eq!(s.get("provider:openai").unwrap(), None);
    }

    #[test]
    fn prices_and_sync_survive_a_save() {
        let dir = temp();
        let mut s = AppSettings { sync_keys: true, ..Default::default() };
        s.prices.insert("my-model".into(), crate::pricing::Price { input: Some(1.0), output: Some(2.0), per_minute: None });
        s.save(&dir).unwrap();
        assert_eq!(AppSettings::load(&dir), s);
        std::fs::remove_dir_all(dir).ok();
    }
}

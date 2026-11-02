//! Provider API keys and the server password.
//!
//! On a Mac, keys are kept in a file in the app's data folder
//! (~/Library/Application Support/<id>/keys.json), readable only by this user.
//! That folder is not touched when the app is replaced or reinstalled, and
//! using it avoids the login keychain, which asks for permission again every
//! time an unsigned build changes. On iOS the app's folder is erased when the
//! app is deleted, so keys go in the device keychain instead (not synced),
//! which iOS keeps across a reinstall and never prompts for.
//!
//! When the user turns on sync, a key moves to iCloud Keychain (the data
//! protection keychain with kSecAttrSynchronizable). That needs the app to be
//! signed with a keychain-access-groups entitlement; unsigned builds get a
//! "missing-entitlement" error and keep the key on the device.

use tauri::AppHandle;

const SERVICE: &str = "com.donovanmontoya.opennotebook";
const MISSING_ENTITLEMENT: &str = "missing-entitlement";

fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("Invalid secret name".into());
    }
    Ok(())
}

/// Keys stored in the app data folder (every platform except iOS).
#[cfg(not(target_os = "ios"))]
mod device {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use tauri::{AppHandle, Manager};

    fn path(app: &AppHandle) -> Result<PathBuf, String> {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir.join("keys.json"))
    }

    fn load(app: &AppHandle) -> Result<BTreeMap<String, String>, String> {
        match std::fs::read(path(app)?) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Writes a temporary file and renames it over the old one, so a crash never leaves half a file.
    fn save(app: &AppHandle, keys: &BTreeMap<String, String>) -> Result<(), String> {
        let target = path(app)?;
        let temp = target.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(keys).map_err(|e| e.to_string())?;
        {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f = options.open(&temp).map_err(|e| e.to_string())?;
            f.write_all(&bytes).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
        }
        std::fs::rename(&temp, &target).map_err(|e| e.to_string())
    }

    pub fn get(app: &AppHandle, name: &str) -> Result<Option<String>, String> {
        Ok(load(app)?.get(name).cloned())
    }

    pub fn set(app: &AppHandle, name: &str, value: &str) -> Result<(), String> {
        let mut keys = load(app)?;
        keys.insert(name.into(), value.into());
        save(app, &keys)
    }

    pub fn delete(app: &AppHandle, name: &str) -> Result<(), String> {
        let mut keys = load(app)?;
        if keys.remove(name).is_some() {
            save(app, &keys)?;
        }
        Ok(())
    }
}

/// Keys stored in the iOS keychain on this device only, which outlives the app.
#[cfg(target_os = "ios")]
mod device {
    use super::{keychain, AppHandle};

    pub fn get(_app: &AppHandle, name: &str) -> Result<Option<String>, String> {
        keychain::get(name, false)
    }

    pub fn set(_app: &AppHandle, name: &str, value: &str) -> Result<(), String> {
        keychain::set(name, value, false)
    }

    pub fn delete(_app: &AppHandle, name: &str) -> Result<(), String> {
        keychain::delete(name, false)
    }
}

/// The data protection keychain: the synced half is iCloud Keychain, the other
/// half stays on this device (used on iOS). Never touches the login keychain,
/// so it never shows a permission prompt.
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod keychain {
    use security_framework::base::Error;
    use security_framework::passwords::{
        delete_generic_password_options, generic_password, set_generic_password_options,
    };
    use security_framework::passwords_options::PasswordOptions;

    const ERR_NOT_FOUND: i32 = -25300; // errSecItemNotFound
    const ERR_MISSING_ENTITLEMENT: i32 = -34018; // errSecMissingEntitlement

    /// The data protection keychain, either the iCloud copy (synced) or this device's.
    fn options(name: &str, synced: bool) -> PasswordOptions {
        let mut o = PasswordOptions::new_generic_password(super::SERVICE, name);
        o.use_protected_keychain();
        o.set_access_synchronized(Some(synced));
        o
    }

    fn describe(e: Error) -> String {
        if e.code() == ERR_MISSING_ENTITLEMENT {
            super::MISSING_ENTITLEMENT.into()
        } else {
            e.to_string()
        }
    }

    fn ignorable(e: &Error) -> bool {
        e.code() == ERR_NOT_FOUND || e.code() == ERR_MISSING_ENTITLEMENT
    }

    pub fn get(name: &str, synced: bool) -> Result<Option<String>, String> {
        match generic_password(options(name, synced)) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if ignorable(&e) => Ok(None),
            Err(e) => Err(describe(e)),
        }
    }

    pub fn set(name: &str, value: &str, synced: bool) -> Result<(), String> {
        set_generic_password_options(value.as_bytes(), options(name, synced)).map_err(describe)
    }

    pub fn delete(name: &str, synced: bool) -> Result<(), String> {
        match delete_generic_password_options(options(name, synced)) {
            Ok(()) => Ok(()),
            Err(e) if ignorable(&e) => Ok(()),
            Err(e) => Err(describe(e)),
        }
    }

    /// Whether this build can write to iCloud Keychain (it is signed for it).
    pub fn available() -> bool {
        let probe = "sync-probe";
        let ok = set_generic_password_options(b"1", options(probe, true)).is_ok();
        let _ = delete_generic_password_options(options(probe, true));
        ok
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod keychain {
    pub fn get(_name: &str, _synced: bool) -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn set(_name: &str, _value: &str, _synced: bool) -> Result<(), String> {
        Err(super::MISSING_ENTITLEMENT.into())
    }

    pub fn delete(_name: &str, _synced: bool) -> Result<(), String> {
        Ok(())
    }

    pub fn available() -> bool {
        false
    }
}

/// This device first, then iCloud Keychain.
#[tauri::command]
pub fn secret_get(app: AppHandle, name: String) -> Result<Option<String>, String> {
    check_name(&name)?;
    match device::get(&app, &name)? {
        Some(value) => Ok(Some(value)),
        None => keychain::get(&name, true),
    }
}

/// Writes to one store, then removes the copy in the other so a key lives in one place.
#[tauri::command]
pub fn secret_set(app: AppHandle, name: String, value: String, sync: Option<bool>) -> Result<(), String> {
    check_name(&name)?;
    if sync.unwrap_or(false) {
        keychain::set(&name, &value, true)?;
        device::delete(&app, &name)
    } else {
        device::set(&app, &name, &value)?;
        keychain::delete(&name, true)
    }
}

#[tauri::command]
pub fn secret_delete(app: AppHandle, name: String) -> Result<(), String> {
    check_name(&name)?;
    device::delete(&app, &name)?;
    keychain::delete(&name, true)
}

#[tauri::command]
pub fn secret_sync_available() -> bool {
    keychain::available()
}

//! Provider API keys and the server password.
//!
//! On Apple platforms each secret is kept either in this device's keychain or,
//! when the user turns on sync, in iCloud Keychain (the data protection
//! keychain with kSecAttrSynchronizable). iCloud Keychain needs the app to be
//! signed with a keychain-access-groups entitlement; unsigned builds get a
//! "missing-entitlement" error and keep using the device keychain.

const SERVICE: &str = "com.donovanmontoya.opennotebook";
const MISSING_ENTITLEMENT: &str = "missing-entitlement";

fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("Invalid secret name".into());
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod store {
    use security_framework::base::Error;
    use security_framework::passwords::{
        delete_generic_password_options, generic_password, set_generic_password_options,
    };
    use security_framework::passwords_options::PasswordOptions;

    const ERR_NOT_FOUND: i32 = -25300; // errSecItemNotFound
    const ERR_MISSING_ENTITLEMENT: i32 = -34018; // errSecMissingEntitlement

    fn options(name: &str, synced: bool) -> PasswordOptions {
        let mut o = PasswordOptions::new_generic_password(super::SERVICE, name);
        if synced {
            o.use_protected_keychain();
            o.set_access_synchronized(Some(true));
        }
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

    /// iCloud copy first, then the device keychain.
    pub fn get(name: &str) -> Result<Option<String>, String> {
        for synced in [true, false] {
            match generic_password(options(name, synced)) {
                Ok(bytes) => return Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
                Err(e) if ignorable(&e) => continue,
                Err(e) => return Err(describe(e)),
            }
        }
        Ok(None)
    }

    /// Writes to one store, then removes the copy in the other so a key lives in one place.
    pub fn set(name: &str, value: &str, sync: bool) -> Result<(), String> {
        set_generic_password_options(value.as_bytes(), options(name, sync)).map_err(describe)?;
        match delete_generic_password_options(options(name, !sync)) {
            Ok(()) => Ok(()),
            Err(e) if ignorable(&e) => Ok(()),
            Err(e) => Err(describe(e)),
        }
    }

    pub fn delete(name: &str) -> Result<(), String> {
        for synced in [true, false] {
            match delete_generic_password_options(options(name, synced)) {
                Ok(()) => {}
                Err(e) if ignorable(&e) => {}
                Err(e) => return Err(describe(e)),
            }
        }
        Ok(())
    }

    /// Whether this build can write to iCloud Keychain (it is signed for it).
    pub fn sync_available() -> bool {
        let probe = "sync-probe";
        let ok = set_generic_password_options(b"1", options(probe, true)).is_ok();
        let _ = delete_generic_password_options(options(probe, true));
        ok
    }
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod store {
    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(super::SERVICE, name).map_err(|e| e.to_string())
    }

    pub fn get(name: &str) -> Result<Option<String>, String> {
        match entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(name: &str, value: &str, sync: bool) -> Result<(), String> {
        if sync {
            return Err(super::MISSING_ENTITLEMENT.into());
        }
        entry(name)?.set_password(value).map_err(|e| e.to_string())
    }

    pub fn delete(name: &str) -> Result<(), String> {
        match entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn sync_available() -> bool {
        false
    }
}

#[tauri::command]
pub fn secret_get(name: String) -> Result<Option<String>, String> {
    check_name(&name)?;
    store::get(&name)
}

#[tauri::command]
pub fn secret_set(name: String, value: String, sync: Option<bool>) -> Result<(), String> {
    check_name(&name)?;
    store::set(&name, &value, sync.unwrap_or(false))
}

#[tauri::command]
pub fn secret_delete(name: String) -> Result<(), String> {
    check_name(&name)?;
    store::delete(&name)
}

#[tauri::command]
pub fn secret_sync_available() -> bool {
    store::sync_available()
}

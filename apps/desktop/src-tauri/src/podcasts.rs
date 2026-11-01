//! Episode audio files. The app sends a finished WAV recording; on macOS it is
//! converted to AAC with the system's afconvert, which makes files about a
//! tenth of the size. Files live in the app data folder under podcasts/.

use std::path::PathBuf;

use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager};

fn podcast_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("podcasts");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn safe_name(id: &str) -> Result<String, String> {
    let name: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if name.is_empty() || name.len() > 128 {
        return Err("Invalid episode id".into());
    }
    Ok(name)
}

/// Saves a WAV recording (raw request body, episode id in the `x-episode-id`
/// header) and returns the path of the stored file.
#[tauri::command]
pub async fn podcast_save(app: AppHandle, request: Request<'_>) -> Result<String, String> {
    let id = request
        .headers()
        .get("x-episode-id")
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing episode id")?;
    let name = safe_name(id)?;
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("Expected audio bytes".into());
    };
    let dir = podcast_dir(&app)?;
    let wav = dir.join(format!("{name}.wav"));
    std::fs::write(&wav, bytes).map_err(|e| e.to_string())?;

    #[cfg(target_os = "macos")]
    {
        let m4a = dir.join(format!("{name}.m4a"));
        let (src, dst) = (wav.clone(), m4a.clone());
        let converted = tauri::async_runtime::spawn_blocking(move || {
            std::process::Command::new("/usr/bin/afconvert")
                .args(["-f", "m4af", "-d", "aac", "-b", "96000"])
                .arg(&src)
                .arg(&dst)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);
        if converted && m4a.exists() {
            let _ = std::fs::remove_file(&wav);
            return Ok(m4a.to_string_lossy().into_owned());
        }
    }
    Ok(wav.to_string_lossy().into_owned())
}

/// Deletes a stored episode file. Only paths inside the podcasts folder are accepted.
#[tauri::command]
pub fn podcast_remove(app: AppHandle, path: String) -> Result<(), String> {
    let dir = podcast_dir(&app)?.canonicalize().map_err(|e| e.to_string())?;
    let target = match PathBuf::from(&path).canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(()), // already gone
    };
    if !target.starts_with(&dir) {
        return Err("Refusing to delete outside the podcasts folder".into());
    }
    std::fs::remove_file(target).map_err(|e| e.to_string())
}

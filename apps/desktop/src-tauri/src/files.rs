//! Original files of uploaded sources (kept under the app data folder) and
//! saving downloads to the user's Downloads folder.

use std::path::{Path, PathBuf};

use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager};

fn originals_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("originals");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Keeps letters, digits and a few separators; everything else becomes `_`.
fn clean(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { "file".into() } else { trimmed.chars().take(120).collect() }
}

fn header(request: &Request<'_>, name: &str) -> Result<String, String> {
    let raw = request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .ok_or(format!("Missing {name}"))?;
    // Header values are percent-encoded by the app so non-ASCII names survive.
    Ok(percent_decode(raw))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn body<'a>(request: &'a Request<'_>) -> Result<&'a [u8], String> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(bytes),
        _ => Err("Expected file bytes".into()),
    }
}

fn inside(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let dir = dir.canonicalize().map_err(|e| e.to_string())?;
    let target = PathBuf::from(path).canonicalize().map_err(|e| e.to_string())?;
    if target.starts_with(&dir) { Ok(target) } else { Err("Path is outside the app's files".into()) }
}

/// Saves an original file. Headers: x-source-id, x-file-name. Returns its path.
#[tauri::command]
pub fn file_save(app: AppHandle, request: Request<'_>) -> Result<String, String> {
    let id = clean(&header(&request, "x-source-id")?);
    let name = clean(&header(&request, "x-file-name")?);
    let path = originals_dir(&app)?.join(format!("{id}__{name}"));
    std::fs::write(&path, body(&request)?).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn file_read(app: AppHandle, path: String) -> Result<Response, String> {
    let target = inside(&originals_dir(&app)?, &path)?;
    Ok(Response::new(std::fs::read(target).map_err(|e| e.to_string())?))
}

#[tauri::command]
pub fn file_remove(app: AppHandle, path: String) -> Result<(), String> {
    match inside(&originals_dir(&app)?, &path) {
        Ok(target) => std::fs::remove_file(target).map_err(|e| e.to_string()),
        Err(_) => Ok(()), // already gone
    }
}

/// Writes bytes to ~/Downloads under a free name and returns the path. Header: x-file-name.
#[tauri::command]
pub fn download_save(app: AppHandle, request: Request<'_>) -> Result<String, String> {
    let name = clean(&header(&request, "x-file-name")?);
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let mut path = dir.join(&name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    std::fs::write(&path, body(&request)?).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

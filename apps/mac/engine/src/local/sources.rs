//! Sources: adding, linking between notebooks, original files, and the
//! background pipeline (read the link, chunk, embed, run chosen transformations).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{OptionalExtension, Row, params};

use super::Engine;
use crate::backend::{Cancel, Result};
use crate::db::int;
use crate::text::{chunk_source_text, encode_vector, title_from};
use crate::transcribe::{Recording, SpeechProvider, is_media, transcribe};
use crate::types::{NewSource, SourceAsset, SourceDetail, SourceFile, SourceInput, SourceSummary};
use crate::Error;

const SOURCE_SELECT: &str = "SELECT s.*,
        (SELECT COUNT(*) FROM chunk c WHERE c.source_id = s.id) AS chunk_count,
        (SELECT COUNT(*) FROM chunk c WHERE c.source_id = s.id AND c.embedding IS NOT NULL) AS embedded_count,
        (SELECT COUNT(*) FROM source_insight i WHERE i.source_id = s.id AND i.status = 'completed') AS insights_count
   FROM source s";

/// Where original files are kept: the Tauri app's folder, so either app can read them.
const ORIGINALS: &str = "originals";

struct SourceRow {
    summary: SourceSummary,
    notebook_id: String,
    full_text: String,
}

fn to_source(r: &Row<'_>) -> rusqlite::Result<SourceRow> {
    let url: Option<String> = r.get("url")?;
    let file_name: Option<String> = r.get("file_name")?;
    let file_ref: Option<String> = r.get("file_ref")?;
    let embedded = int(r, "embedded_count")?;
    let asset = match (url, file_name) {
        (Some(url), _) => Some(SourceAsset { url: Some(url), file_path: None }),
        (None, Some(name)) => Some(SourceAsset { file_path: Some(name), url: None }),
        _ => None,
    };
    Ok(SourceRow {
        summary: SourceSummary {
            id: r.get("id")?,
            title: r.get("title")?,
            topics: Vec::new(),
            asset,
            embedded: embedded > 0,
            embedded_chunks: embedded as u32,
            insights_count: int(r, "insights_count")? as u32,
            created: r.get("created")?,
            updated: r.get("updated")?,
            file_available: file_ref.is_some(),
            status: r.get("status")?,
            error: r.get("error")?,
            notebooks: Vec::new(),
        },
        notebook_id: r.get("notebook_id")?,
        full_text: r.get::<_, Option<String>>("full_text")?.unwrap_or_default(),
    })
}

/// Keeps letters, digits and a few separators, as `files.rs` names saved files.
fn clean_file_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' }).collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { "file".into() } else { trimmed.chars().take(120).collect() }
}

impl Engine {
    fn source_rows(&self, filter: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<SourceRow>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!("{SOURCE_SELECT} WHERE {filter}"))?;
        let rows = stmt.query_map(params, to_source)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_sources(&self, notebook_id: &str) -> Result<Vec<SourceSummary>> {
        let rows = self.source_rows(
            "s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = ?1) ORDER BY s.updated DESC",
            &[&notebook_id],
        )?;
        Ok(rows.into_iter().map(|r| r.summary).collect())
    }

    pub fn get_source(&self, id: &str) -> Result<SourceDetail> {
        let row = self.source_rows("s.id = ?1", &[&id])?.into_iter().next().ok_or_else(|| Error::NotFound("Source".into()))?;
        let links: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT notebook_id FROM notebook_source WHERE source_id = ?1")?;
            stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        let mut summary = row.summary;
        summary.notebooks = std::iter::once(row.notebook_id.clone()).chain(links.into_iter().filter(|n| *n != row.notebook_id)).collect();
        Ok(SourceDetail { summary, full_text: row.full_text })
    }

    /// Every source in every notebook, newest first, with the notebooks each is in.
    pub fn list_all_sources(&self) -> Result<Vec<SourceSummary>> {
        let rows = self.source_rows("1 = 1 ORDER BY s.updated DESC", &[])?;
        let mut by_source: HashMap<String, Vec<String>> = HashMap::new();
        {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT source_id, notebook_id FROM notebook_source")?;
            for link in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (s, n) = link?;
                by_source.entry(s).or_default().push(n);
            }
        }
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut s = r.summary;
                s.notebooks = by_source.remove(&s.id).unwrap_or_else(|| vec![r.notebook_id]);
                s
            })
            .collect())
    }

    pub fn add_source(self: &Arc<Self>, notebook_id: &str, source: NewSource) -> Result<SourceSummary> {
        self.get_notebook(notebook_id)?;
        let id = self.id("source");
        let (kind, url, text, title) = match &source.input {
            SourceInput::Link { url, title } => ("link", Some(url.trim().to_string()), String::new(), title.clone()),
            SourceInput::Text { content, title } => ("text", None, content.clone(), title.clone()),
            SourceInput::Document { title, content, .. } => ("document", None, content.clone(), Some(title.clone())),
        };
        let explicit = title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(String::from);
        let fallback = match &url {
            Some(u) => u.clone(),
            None => title_from(&text, 60),
        };
        let shown = explicit.clone().unwrap_or(fallback);
        let shown = if shown.is_empty() { "Untitled source".to_string() } else { shown };
        let pending = (!source.transformations.is_empty()).then(|| serde_json::to_string(&source.transformations)).transpose()?;
        {
            let conn = self.db.lock();
            conn.execute(
                "INSERT INTO source (id, notebook_id, title, kind, url, full_text, status, pending_transformations, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'processing', ?7, ?8, ?8)",
                params![id, notebook_id, shown, kind, url, text, pending, self.now()],
            )?;
            conn.execute("INSERT INTO notebook_source (source_id, notebook_id) VALUES (?1, ?2)", params![id, notebook_id])?;
        }
        if let SourceInput::Document { title, file_name, file: Some(data), mime, .. } = &source.input {
            let name = file_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| if title.is_empty() { "source".into() } else { title.clone() });
            let mime = mime.clone().unwrap_or_default();
            let path = self.save_file(&id, &name, data)?;
            self.db.lock().execute(
                "UPDATE source SET file_ref = ?1, file_name = ?2, file_type = ?3, kind = ?4 WHERE id = ?5",
                params![path, name, mime, if is_media(&mime, &name) { "media" } else { "document" }, id],
            )?;
        }
        self.touch_notebook(notebook_id)?;
        self.process(&id, explicit);
        let row = self.source_rows("s.id = ?1", &[&id])?.into_iter().next().ok_or_else(|| Error::NotFound("Source".into()))?;
        Ok(row.summary)
    }

    pub fn retry_source(self: &Arc<Self>, id: &str) -> Result<()> {
        self.get_source(id)?;
        self.db
            .lock()
            .execute("UPDATE source SET status = 'processing', error = NULL, updated = ?1 WHERE id = ?2", params![self.now(), id])?;
        self.process(id, None);
        Ok(())
    }

    pub fn delete_source(&self, id: &str) -> Result<()> {
        self.cancel_running(id);
        let chats: Vec<String> = {
            let conn = self.db.lock();
            conn.execute("DELETE FROM chunk WHERE source_id = ?1", [id])?;
            conn.execute("DELETE FROM source_insight WHERE source_id = ?1", [id])?;
            let mut stmt = conn.prepare("SELECT id FROM chat_session WHERE source_id = ?1")?;
            stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for c in chats {
            self.delete_chat_session(&c)?;
        }
        let file: Option<String> = self.db.lock().query_row("SELECT file_ref FROM source WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
        if let Some(path) = file.and_then(|f| self.original(&f)) {
            let _ = std::fs::remove_file(path);
        }
        let conn = self.db.lock();
        conn.execute("DELETE FROM notebook_source WHERE source_id = ?1", [id])?;
        conn.execute("DELETE FROM source WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn link_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        self.get_source(source_id)?;
        self.get_notebook(notebook_id)?;
        self.db
            .lock()
            .execute("INSERT OR IGNORE INTO notebook_source (source_id, notebook_id) VALUES (?1, ?2)", params![source_id, notebook_id])?;
        self.touch_notebook(notebook_id)
    }

    pub fn unlink_source(&self, source_id: &str, notebook_id: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute("DELETE FROM notebook_source WHERE source_id = ?1 AND notebook_id = ?2", params![source_id, notebook_id])?;
        // Keep the home notebook pointing at a notebook the source is still in.
        let left: Option<String> =
            conn.query_row("SELECT notebook_id FROM notebook_source WHERE source_id = ?1", [source_id], |r| r.get(0)).optional()?;
        if let Some(left) = left {
            conn.execute("UPDATE source SET notebook_id = ?1 WHERE id = ?2 AND notebook_id = ?3", params![left, source_id, notebook_id])?;
        }
        Ok(())
    }

    pub fn rename_source(&self, id: &str, title: &str) -> Result<SourceDetail> {
        let title = if title.trim().is_empty() { "Untitled source" } else { title.trim() };
        self.db.lock().execute("UPDATE source SET title = ?1, updated = ?2 WHERE id = ?3", params![title, self.now(), id])?;
        self.get_source(id)
    }

    pub fn download_source(&self, id: &str) -> Result<Option<SourceFile>> {
        let row: Option<(Option<String>, Option<String>, Option<String>)> = self
            .db
            .lock()
            .query_row("SELECT file_ref, file_name, file_type FROM source WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let Some((Some(file_ref), name, mime)) = row else { return Ok(None) };
        let path = self.original(&file_ref).ok_or_else(|| Error::other("The original file is missing or outside the app's files"))?;
        Ok(Some(SourceFile { data: std::fs::read(path)?, name: name.unwrap_or_else(|| "source".into()), mime: mime.unwrap_or_default() }))
    }

    /// Saves an original file as the Tauri app does: `<data_dir>/originals/<id>__<name>`, by absolute path.
    fn save_file(&self, source_id: &str, name: &str, data: &[u8]) -> Result<String> {
        let dir = self.data_dir.join(ORIGINALS);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}__{}", clean_file_name(source_id), clean_file_name(name)));
        std::fs::write(&path, data)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// A saved file's path, only if it is inside the originals folder.
    pub(crate) fn original(&self, file_ref: &str) -> Option<PathBuf> {
        let dir = self.data_dir.join(ORIGINALS).canonicalize().ok()?;
        let target = Path::new(file_ref).canonicalize().ok()?;
        target.starts_with(&dir).then_some(target)
    }

    // Processing

    /// Queues a source for processing; at most two run at once.
    fn process(self: &Arc<Self>, id: &str, explicit_title: Option<String>) {
        let (generation, cancel) = self.start_running(id);
        let engine = self.clone();
        let id = id.to_string();
        self.jobs.push(move || {
            if !cancel.is_cancelled()
                && let Err(error) = engine.run_pipeline(&id, &cancel, explicit_title.as_deref())
                && !cancel.is_cancelled()
            {
                let _ = engine.db.lock().execute(
                    "UPDATE source SET status = 'failed', error = ?1, updated = ?2 WHERE id = ?3",
                    params![error.to_string(), engine.now(), id],
                );
            }
            engine.finish_running(&id, generation);
        });
    }

    /// Read (links), chunk, embed (when an embedding model is set), then mark completed.
    fn run_pipeline(self: &Arc<Self>, id: &str, cancel: &Cancel, explicit_title: Option<&str>) -> Result<()> {
        let source = self.get_source(id)?;
        let mut text = source.full_text.clone();

        if let Some(url) = source.summary.asset.as_ref().and_then(|a| a.url.clone()).filter(|_| text.is_empty()) {
            let extracted = (self.extract_url)(&url, cancel)?;
            text = extracted.text;
            let title = match explicit_title {
                None if !extracted.title.is_empty() => Some(extracted.title),
                _ => source.summary.title.clone(),
            };
            if text.trim().is_empty() {
                return Err(Error::other("No readable text was found at that address."));
            }
            self.db
                .lock()
                .execute("UPDATE source SET full_text = ?1, title = ?2, updated = ?3 WHERE id = ?4", params![text, title, self.now(), id])?;
        }

        if text.trim().is_empty() {
            let row: Option<(String, Option<String>)> =
                self.db.lock().query_row("SELECT kind, file_ref FROM source WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            if let Some(("media", Some(_))) = row.as_ref().map(|(k, f)| (k.as_str(), f.as_ref())) {
                text = self.transcribe_source(id, cancel)?;
                self.db.lock().execute("UPDATE source SET full_text = ?1, updated = ?2 WHERE id = ?3", params![text, self.now(), id])?;
            }
        }

        let pieces = chunk_source_text(&text);
        let config = self.ai_config()?;
        let vectors = match &config.embedding {
            Some(choice) if !pieces.is_empty() => Some(self.embed_with(choice, "embedding", &pieces, cancel)?),
            _ => None,
        };
        if cancel.is_cancelled() {
            return Ok(());
        }

        let pending: Option<String> = {
            let mut conn = self.db.lock();
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM chunk WHERE source_id = ?1", [id])?;
            for (i, piece) in pieces.iter().enumerate() {
                let (embedding, model) = match (&vectors, &config.embedding) {
                    (Some(v), Some(c)) => (Some(encode_vector(&v[i])), Some(c.model.clone())),
                    _ => (None, None),
                };
                tx.execute(
                    "INSERT INTO chunk (id, source_id, position, text, embedding, embedding_model) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![format!("{id}#{i}"), id, i as i64, piece, embedding, model],
                )?;
            }
            tx.execute("UPDATE source SET status = 'completed', error = NULL, updated = ?1 WHERE id = ?2", params![self.now(), id])?;
            let pending = tx.query_row("SELECT pending_transformations FROM source WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten();
            if pending.is_some() {
                tx.execute("UPDATE source SET pending_transformations = NULL WHERE id = ?1", [id])?;
            }
            tx.commit()?;
            pending
        };
        // Transformations chosen when the source was added run once its text is ready.
        if let Some(pending) = pending {
            for tid in serde_json::from_str::<Vec<String>>(&pending).unwrap_or_default() {
                let _ = self.create_insight(id, &tid);
            }
        }
        Ok(())
    }

    /// Text for an audio or video source with no text yet, from the speech to text model in Settings.
    fn transcribe_source(&self, id: &str, cancel: &Cancel) -> Result<String> {
        let (file_ref, name, mime): (Option<String>, Option<String>, Option<String>) =
            self.db.lock().query_row("SELECT file_ref, file_name, file_type FROM source WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let config = self.ai_config()?;
        let Some(choice) = config.transcription else {
            return Err(Error::Setup("Choose a speech to text model in Settings to transcribe audio and video.".into()));
        };
        let path = file_ref
            .and_then(|f| self.original(&f))
            .ok_or_else(|| Error::other("The original file is missing or outside the app's files"))?;
        let data = std::fs::read(path)?;
        let provider = SpeechProvider {
            kind: choice.provider.kind.as_str(),
            base_url: choice.provider.base_url.as_deref(),
            api_key: choice.provider.api_key.as_deref(),
        };
        let file = Recording { data: &data, name: name.as_deref().unwrap_or("recording"), mime: mime.as_deref().unwrap_or_default() };
        let text = transcribe(&*self.http, provider, &choice.model, file, cancel)?;
        if text.is_empty() {
            return Err(Error::other("No speech was found in this recording."));
        }
        self.record_usage("transcription", &choice.model, 0, super::tokens(text.chars().count()), 0.0);
        Ok(text)
    }
}

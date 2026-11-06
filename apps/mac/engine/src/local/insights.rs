//! Transformations (reusable prompts), insights made from them in the
//! background, the usage report and rebuilding the search index.

use std::sync::Arc;

use rusqlite::{OptionalExtension, Row, params};
use time::OffsetDateTime;

use super::{Engine, iso};
use crate::backend::{Cancel, Result};
use crate::db::{int, num};
use crate::insights::{DEFAULT_TRANSFORMATIONS, DEFAULT_TRANSFORMATION_INSTRUCTIONS};
use crate::providers::{ChatRequest, ChatTurn};
use crate::text::{char_len, take_chars};
use crate::types::{Insight, Note, Transformation, UsageGroup, UsageReport};
use crate::Error;

/// Source text sent to a transformation, about 100k tokens.
const TRANSFORMATION_CHARS: usize = 400_000;

fn to_transformation(r: &Row<'_>) -> rusqlite::Result<Transformation> {
    Ok(Transformation {
        id: r.get("id")?,
        name: r.get("name")?,
        title: r.get("title")?,
        description: r.get::<_, Option<String>>("description")?.unwrap_or_default(),
        prompt: r.get("prompt")?,
        apply_default: int(r, "apply_default")? != 0,
    })
}

fn to_insight(r: &Row<'_>) -> rusqlite::Result<Insight> {
    Ok(Insight {
        id: r.get("id")?,
        source_id: r.get("source_id")?,
        kind: r.get("insight_type")?,
        content: r.get::<_, Option<String>>("content")?.unwrap_or_default(),
        created: r.get("created")?,
        status: r.get("status")?,
        error: r.get("error")?,
    })
}

impl Engine {
    /// Adds the server's default transformations the first time the app runs.
    pub fn seed_transformations(&self) -> Result<()> {
        let mut conn = self.db.lock();
        let seeded: Option<String> =
            conn.query_row("SELECT value FROM app_setting WHERE key = 'transformations_seeded'", [], |r| r.get(0)).optional()?;
        if seeded.is_some() {
            return Ok(());
        }
        let now = self.now();
        let tx = conn.transaction()?;
        for t in DEFAULT_TRANSFORMATIONS {
            tx.execute(
                "INSERT INTO transformation (id, name, title, description, prompt, apply_default, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![self.id("transformation"), t.name, t.title, t.description, t.prompt, t.apply_default as i64, now],
            )?;
        }
        tx.execute("INSERT INTO app_setting (key, value) VALUES ('transformations_seeded', '1')", [])?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_transformations(&self) -> Result<Vec<Transformation>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT * FROM transformation ORDER BY apply_default DESC, title")?;
        let rows = stmt.query_map([], to_transformation)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn get_transformation(&self, id: &str) -> Result<Transformation> {
        self.db
            .lock()
            .query_row("SELECT * FROM transformation WHERE id = ?1", [id], to_transformation)
            .optional()?
            .ok_or_else(|| Error::NotFound("Transformation".into()))
    }

    /// Creates the transformation when `id` is empty.
    pub fn save_transformation(&self, t: Transformation) -> Result<Transformation> {
        let name = t.name.trim().to_string();
        let title = if t.title.trim().is_empty() { name.clone() } else { t.title.trim().to_string() };
        let now = self.now();
        if !t.id.is_empty() {
            self.db.lock().execute(
                "UPDATE transformation SET name = ?1, title = ?2, description = ?3, prompt = ?4, apply_default = ?5, updated = ?6 WHERE id = ?7",
                params![name, title, t.description, t.prompt, t.apply_default as i64, now, t.id],
            )?;
            return self.get_transformation(&t.id);
        }
        let id = self.id("transformation");
        self.db.lock().execute(
            "INSERT INTO transformation (id, name, title, description, prompt, apply_default, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![id, name, title, t.description, t.prompt, t.apply_default as i64, now],
        )?;
        self.get_transformation(&id)
    }

    pub fn delete_transformation(&self, id: &str) -> Result<()> {
        self.db.lock().execute("DELETE FROM transformation WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The instructions placed before every transformation prompt.
    pub fn transformation_instructions(&self) -> Result<String> {
        let value: Option<String> = self
            .db
            .lock()
            .query_row("SELECT value FROM app_setting WHERE key = 'transformation_instructions'", [], |r| r.get(0))
            .optional()?;
        Ok(value.unwrap_or_else(|| DEFAULT_TRANSFORMATION_INSTRUCTIONS.into()))
    }

    pub fn set_transformation_instructions(&self, text: &str) -> Result<()> {
        self.db.lock().execute(
            "INSERT INTO app_setting (key, value) VALUES ('transformation_instructions', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [text],
        )?;
        Ok(())
    }

    /// Runs a transformation on some text and returns the result.
    pub fn run_transformation(&self, transformation_id: &str, text: &str, cancel: &Cancel) -> Result<String> {
        let config = self.ai_config()?;
        let model = config.writer().cloned().ok_or_else(|| Error::Setup("Choose a chat model in Settings to run transformations.".into()))?;
        let t = self.get_transformation(transformation_id)?;
        let instructions = self.transformation_instructions()?;
        let input = if char_len(text) > TRANSFORMATION_CHARS {
            format!("{}\n\n[Text shortened to fit]", take_chars(text, TRANSFORMATION_CHARS))
        } else {
            text.to_string()
        };
        let request = ChatRequest::new(&model.model, &format!("{instructions}\n\n{}", t.prompt), vec![ChatTurn::user(input)]).cancel(cancel);
        self.chat_with(&model, "transformation", request)
    }

    pub fn list_insights(&self, source_id: &str) -> Result<Vec<Insight>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT * FROM source_insight WHERE source_id = ?1 ORDER BY created")?;
        let rows = stmt.query_map([source_id], to_insight)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Starts a transformation on a source in the background; the insight shows as running until done.
    pub fn create_insight(self: &Arc<Self>, source_id: &str, transformation_id: &str) -> Result<()> {
        let source = self.get_source(source_id)?;
        if source.full_text.trim().is_empty() {
            return Err(Error::Setup("This source has no text to transform yet.".into()));
        }
        let t = self.get_transformation(transformation_id)?;
        let id = self.id("insight");
        self.db.lock().execute(
            "INSERT INTO source_insight (id, source_id, insight_type, content, status, created, updated) VALUES (?1, ?2, ?3, '', 'running', ?4, ?4)",
            params![id, source_id, t.title, self.now()],
        )?;
        let (generation, cancel) = self.start_running(&id);
        let engine = self.clone();
        let tid = transformation_id.to_string();
        self.jobs.push(move || {
            let result = engine.run_transformation(&tid, &source.full_text, &cancel);
            if !cancel.is_cancelled() {
                let now = engine.now();
                let _ = match result {
                    Ok(content) => engine.db.lock().execute(
                        "UPDATE source_insight SET content = ?1, status = 'completed', updated = ?2 WHERE id = ?3",
                        params![content.trim(), now, id],
                    ),
                    Err(e) => engine.db.lock().execute(
                        "UPDATE source_insight SET status = 'failed', error = ?1, updated = ?2 WHERE id = ?3",
                        params![e.to_string(), now, id],
                    ),
                };
            }
            engine.finish_running(&id, generation);
        });
        Ok(())
    }

    pub fn delete_insight(&self, id: &str) -> Result<()> {
        self.cancel_running(id);
        self.db.lock().execute("DELETE FROM source_insight WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn save_insight_as_note(&self, insight_id: &str, notebook_id: &str) -> Result<Note> {
        let row: Option<(String, String, Option<String>)> = self
            .db
            .lock()
            .query_row(
                "SELECT i.insight_type, i.content, s.title AS source_title FROM source_insight i JOIN source s ON s.id = i.source_id WHERE i.id = ?1",
                [insight_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (kind, content, source_title) = row.ok_or_else(|| Error::NotFound("Insight".into()))?;
        let title = format!("{kind}: {}", source_title.unwrap_or_else(|| "Source".into()));
        self.create_note(notebook_id, Some(&title), &content, true)
    }

    // Usage and the index

    /// Calls in the last `days` days by model and feature. Token counts are estimated from text length.
    pub fn usage(&self, days: u32) -> Result<UsageReport> {
        let since = iso((self.clock)().checked_sub(time::Duration::days(days as i64)).unwrap_or(OffsetDateTime::UNIX_EPOCH));
        let conn = self.db.lock();
        let mut stmt = conn.prepare(
            "SELECT model, feature, COUNT(*) AS calls, SUM(input_tokens) AS input, SUM(output_tokens) AS output, SUM(audio_seconds) AS audio
               FROM usage_event WHERE created >= ?1 GROUP BY model, feature ORDER BY calls DESC",
        )?;
        let groups = stmt
            .query_map([since], |r| {
                Ok(UsageGroup {
                    model: r.get("model")?,
                    feature: r.get("feature")?,
                    calls: int(r, "calls")? as u64,
                    input_tokens: int(r, "input")? as u64,
                    output_tokens: int(r, "output")? as u64,
                    audio_seconds: num(r, "audio")?,
                    estimated_usd: None,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(UsageReport { days, calls: groups.iter().map(|g| g.calls).sum(), estimated_usd: None, groups, estimated: true })
    }

    /// Re-processes every finished source with the current embedding model. Returns how many were queued.
    pub fn rebuild_index(self: &Arc<Self>) -> Result<u32> {
        let ids: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT id FROM source WHERE status IN ('completed', 'failed')")?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for id in &ids {
            self.retry_source(id)?;
        }
        Ok(ids.len() as u32)
    }
}

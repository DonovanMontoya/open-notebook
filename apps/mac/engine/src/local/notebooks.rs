//! Notebooks and notes.

use rusqlite::{OptionalExtension, Row, params};

use super::Engine;
use crate::Error;
use crate::backend::Result;
use crate::db::int;
use crate::text::title_from;
use crate::types::{Note, Notebook, NotebookUpdate};

const NOTEBOOK_SELECT: &str = "SELECT n.*,
        (SELECT COUNT(*) FROM notebook_source l WHERE l.notebook_id = n.id) AS source_count,
        (SELECT COUNT(*) FROM note t WHERE t.notebook_id = n.id) AS note_count
   FROM notebook n";

fn to_notebook(r: &Row<'_>) -> rusqlite::Result<Notebook> {
    Ok(Notebook {
        id: r.get("id")?,
        name: r.get("name")?,
        description: r.get::<_, Option<String>>("description")?.unwrap_or_default(),
        archived: int(r, "archived")? != 0,
        created: r.get("created")?,
        updated: r.get("updated")?,
        source_count: int(r, "source_count")? as u32,
        note_count: int(r, "note_count")? as u32,
    })
}

pub(super) fn to_note(r: &Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: r.get("id")?,
        title: r.get("title")?,
        content: Some(r.get::<_, Option<String>>("content")?.unwrap_or_default()),
        note_type: r.get("note_type")?,
        created: r.get("created")?,
        updated: r.get("updated")?,
    })
}

impl Engine {
    pub fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare(&format!("{NOTEBOOK_SELECT} ORDER BY n.updated DESC"))?;
        let rows = stmt.query_map([], to_notebook)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_notebook(&self, id: &str) -> Result<Notebook> {
        let conn = self.db.lock();
        conn.query_row(&format!("{NOTEBOOK_SELECT} WHERE n.id = ?1"), [id], to_notebook).optional()?.ok_or_else(|| Error::NotFound("Notebook".into()))
    }

    pub fn create_notebook(&self, name: &str, description: &str) -> Result<Notebook> {
        let id = self.id("notebook");
        self.db.lock().execute(
            "INSERT INTO notebook (id, name, description, archived, created, updated) VALUES (?1, ?2, ?3, 0, ?4, ?4)",
            params![id, name.trim(), description.trim(), self.now()],
        )?;
        self.get_notebook(&id)
    }

    pub fn update_notebook(&self, id: &str, update: NotebookUpdate) -> Result<Notebook> {
        let current = self.get_notebook(id)?;
        let name = update.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&current.name).to_string();
        self.db.lock().execute(
            "UPDATE notebook SET name = ?1, description = ?2, archived = ?3, updated = ?4 WHERE id = ?5",
            params![name, update.description.unwrap_or(current.description), update.archived.unwrap_or(current.archived) as i64, self.now(), id],
        )?;
        self.get_notebook(id)
    }

    pub fn delete_notebook(&self, id: &str) -> Result<()> {
        // Episodes first, so their jobs stop and their audio files are deleted too.
        let episodes: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT id FROM podcast_episode WHERE notebook_id = ?1")?;
            stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for e in episodes {
            self.delete_episode(&e)?;
        }
        // Sources only in this notebook go with it; shared ones just lose the link, like on the server.
        let sources: Vec<(String, i64)> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare(
                "SELECT l.source_id AS id, (SELECT COUNT(*) FROM notebook_source o WHERE o.source_id = l.source_id) AS links
                   FROM notebook_source l WHERE l.notebook_id = ?1",
            )?;
            stmt.query_map([id], |r| Ok((r.get(0)?, int(r, "links")?)))?.collect::<rusqlite::Result<_>>()?
        };
        for (source, links) in sources {
            if links <= 1 { self.delete_source(&source)? } else { self.unlink_source(&source, id)? }
        }
        let sessions: Vec<String> = {
            let conn = self.db.lock();
            let mut stmt = conn.prepare("SELECT id FROM chat_session WHERE notebook_id = ?1")?;
            stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        for s in sessions {
            self.delete_chat_session(&s)?;
        }
        let conn = self.db.lock();
        conn.execute("DELETE FROM podcast_episode WHERE notebook_id = ?1", [id])?;
        conn.execute("DELETE FROM note WHERE notebook_id = ?1", [id])?;
        conn.execute("DELETE FROM notebook WHERE id = ?1", [id])?;
        Ok(())
    }

    pub(crate) fn touch_notebook(&self, id: &str) -> Result<()> {
        self.db.lock().execute("UPDATE notebook SET updated = ?1 WHERE id = ?2", params![self.now(), id])?;
        Ok(())
    }

    // Notes

    pub fn list_notes(&self, notebook_id: &str) -> Result<Vec<Note>> {
        let conn = self.db.lock();
        let mut stmt = conn.prepare("SELECT * FROM note WHERE notebook_id = ?1 ORDER BY updated DESC")?;
        let rows = stmt.query_map([notebook_id], to_note)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The note and the notebook it is in.
    fn get_note(&self, id: &str) -> Result<(Note, String)> {
        let conn = self.db.lock();
        conn.query_row("SELECT * FROM note WHERE id = ?1", [id], |r| Ok((to_note(r)?, r.get("notebook_id")?)))
            .optional()?
            .ok_or_else(|| Error::NotFound("Note".into()))
    }

    pub fn create_note(&self, notebook_id: &str, title: Option<&str>, content: &str, ai: bool) -> Result<Note> {
        self.get_notebook(notebook_id)?;
        let id = self.id("note");
        let title = title.map(str::trim).filter(|t| !t.is_empty()).map(String::from).unwrap_or_else(|| title_from(content, 60));
        let title = if title.is_empty() { "Untitled note".to_string() } else { title };
        self.db.lock().execute(
            "INSERT INTO note (id, notebook_id, title, content, note_type, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, notebook_id, title, content, if ai { "ai" } else { "human" }, self.now()],
        )?;
        self.touch_notebook(notebook_id)?;
        Ok(self.get_note(&id)?.0)
    }

    pub fn update_note(&self, id: &str, title: Option<&str>, content: Option<&str>) -> Result<Note> {
        let (current, notebook_id) = self.get_note(id)?;
        self.db.lock().execute(
            "UPDATE note SET title = ?1, content = ?2, updated = ?3 WHERE id = ?4",
            params![title.map(String::from).or(current.title), content.map(String::from).or(current.content).unwrap_or_default(), self.now(), id],
        )?;
        self.touch_notebook(&notebook_id)?;
        Ok(self.get_note(id)?.0)
    }

    pub fn delete_note(&self, id: &str) -> Result<()> {
        self.db.lock().execute("DELETE FROM note WHERE id = ?1", [id])?;
        Ok(())
    }
}

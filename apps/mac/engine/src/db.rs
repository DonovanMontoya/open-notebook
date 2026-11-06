//! The on-device SQLite database: the same schema, ids, timestamps and JSON
//! columns as the Tauri app (`packages/engine/src/db.ts`), so both apps open the
//! same `open-notebook.db`.
//!
//! Formats the TypeScript engine writes, kept here:
//! - ids are `<table>:<uuid v4>` (chunks are `<source id>#<position>`);
//! - timestamps are `Date.toISOString()`: `2026-11-01T12:00:00.000Z`;
//! - embeddings are base64 little-endian float32 (see [`crate::text::encode_vector`]);
//! - lists and objects (pending transformations, transcripts, speakers) are JSON text;
//! - booleans are 0 and 1.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::backend::Result;

pub const FILE_NAME: &str = "open-notebook.db";

const MIGRATIONS: &[&str] = &[
    r"CREATE TABLE IF NOT EXISTS notebook (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     archived INTEGER NOT NULL DEFAULT 0,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE TABLE IF NOT EXISTS source (
     id TEXT PRIMARY KEY,
     notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE,
     title TEXT,
     kind TEXT NOT NULL,
     url TEXT,
     full_text TEXT NOT NULL DEFAULT '',
     status TEXT NOT NULL,
     error TEXT,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE INDEX IF NOT EXISTS source_notebook ON source(notebook_id)",
    r"CREATE TABLE IF NOT EXISTS chunk (
     id TEXT PRIMARY KEY,
     source_id TEXT NOT NULL REFERENCES source(id) ON DELETE CASCADE,
     position INTEGER NOT NULL,
     text TEXT NOT NULL,
     embedding TEXT,
     embedding_model TEXT
   )",
    r"CREATE INDEX IF NOT EXISTS chunk_source ON chunk(source_id)",
    r"CREATE TABLE IF NOT EXISTS note (
     id TEXT PRIMARY KEY,
     notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE,
     title TEXT,
     content TEXT NOT NULL DEFAULT '',
     note_type TEXT NOT NULL DEFAULT 'human',
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE INDEX IF NOT EXISTS note_notebook ON note(notebook_id)",
    r"CREATE TABLE IF NOT EXISTS chat_session (
     id TEXT PRIMARY KEY,
     notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE,
     title TEXT NOT NULL,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE TABLE IF NOT EXISTS chat_message (
     id TEXT PRIMARY KEY,
     session_id TEXT NOT NULL REFERENCES chat_session(id) ON DELETE CASCADE,
     type TEXT NOT NULL,
     content TEXT NOT NULL,
     timestamp TEXT NOT NULL
   )",
    r"CREATE INDEX IF NOT EXISTS chat_message_session ON chat_message(session_id, timestamp)",
    r"CREATE TABLE IF NOT EXISTS podcast_episode (
     id TEXT PRIMARY KEY,
     notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE,
     name TEXT NOT NULL,
     summary TEXT,
     focus TEXT,
     format TEXT NOT NULL,
     length TEXT NOT NULL,
     status TEXT NOT NULL,
     progress_done INTEGER NOT NULL DEFAULT 0,
     progress_total INTEGER NOT NULL DEFAULT 0,
     error TEXT,
     script TEXT,
     transcript TEXT,
     chapters TEXT,
     exact_timing INTEGER NOT NULL DEFAULT 1,
     duration REAL,
     audio_ref TEXT,
     voice_label TEXT,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE INDEX IF NOT EXISTS podcast_episode_notebook ON podcast_episode(notebook_id, created)",
    r"CREATE TABLE IF NOT EXISTS podcast_check (
     episode_id TEXT PRIMARY KEY REFERENCES podcast_episode(id) ON DELETE CASCADE,
     status TEXT NOT NULL,
     model TEXT NOT NULL,
     result TEXT,
     error TEXT,
     created TEXT NOT NULL
   )",
    r"CREATE TABLE IF NOT EXISTS podcast_prompt (
     episode_id TEXT PRIMARY KEY REFERENCES podcast_episode(id) ON DELETE CASCADE,
     model TEXT NOT NULL,
     system TEXT NOT NULL,
     prompt TEXT NOT NULL,
     created TEXT NOT NULL
   )",
    r"CREATE TABLE IF NOT EXISTS transformation (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     title TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     prompt TEXT NOT NULL,
     apply_default INTEGER NOT NULL DEFAULT 0,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE TABLE IF NOT EXISTS source_insight (
     id TEXT PRIMARY KEY,
     source_id TEXT NOT NULL REFERENCES source(id) ON DELETE CASCADE,
     insight_type TEXT NOT NULL,
     content TEXT NOT NULL DEFAULT '',
     status TEXT NOT NULL DEFAULT 'completed',
     error TEXT,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )",
    r"CREATE INDEX IF NOT EXISTS source_insight_source ON source_insight(source_id)",
    r"CREATE TABLE IF NOT EXISTS app_setting (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    r"CREATE TABLE IF NOT EXISTS notebook_source (
     source_id TEXT NOT NULL,
     notebook_id TEXT NOT NULL,
     PRIMARY KEY (source_id, notebook_id)
   )",
    r"CREATE INDEX IF NOT EXISTS notebook_notebook_source ON notebook_source(notebook_id)",
    r"INSERT OR IGNORE INTO notebook_source (source_id, notebook_id) SELECT id, notebook_id FROM source",
    r"CREATE TABLE IF NOT EXISTS usage_event (
     id TEXT PRIMARY KEY,
     created TEXT NOT NULL,
     feature TEXT NOT NULL,
     model TEXT NOT NULL,
     input_tokens INTEGER NOT NULL DEFAULT 0,
     output_tokens INTEGER NOT NULL DEFAULT 0,
     audio_seconds REAL NOT NULL DEFAULT 0
   )",
    r"CREATE INDEX IF NOT EXISTS usage_event_created ON usage_event(created)",
    r"CREATE TABLE IF NOT EXISTS speaker_profile (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     voice_kind TEXT NOT NULL,
     voice_model TEXT NOT NULL,
     speakers TEXT NOT NULL
   )",
    r"CREATE TABLE IF NOT EXISTS episode_profile (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     speaker_profile_id TEXT NOT NULL,
     language TEXT,
     briefing TEXT NOT NULL,
     segments INTEGER NOT NULL DEFAULT 5
   )",
];

/// Columns added after the first release, applied when missing.
const COLUMNS: &[(&str, &str, &str)] = &[
    ("source", "pending_transformations", "TEXT"),
    ("chat_session", "source_id", "TEXT"),
    ("source", "file_ref", "TEXT"),
    ("source", "file_name", "TEXT"),
    ("source", "file_type", "TEXT"),
    ("podcast_episode", "episode_profile_id", "TEXT"),
    ("podcast_episode", "speaker_profile_id", "TEXT"),
    ("podcast_episode", "effort", "TEXT"),
    ("podcast_prompt", "material", "TEXT"),
];

/// Creates the schema. Safe to run on every launch.
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON")?;
    for statement in MIGRATIONS {
        conn.execute_batch(statement)?;
    }
    for (table, column, definition) in COLUMNS {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if !names.iter().any(|n| n == column) {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
        }
    }
    Ok(())
}

/// One connection shared by the UI's worker threads and background jobs. Hold it
/// only for queries, never across a network call.
pub struct Db(Mutex<Connection>);

impl Db {
    /// Opens (or creates) the database file and migrates it. The Tauri app's sqlx
    /// pool uses WAL and a 5 second busy timeout; so does this.
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Safe with WAL: a crash can lose the last commits but never corrupts the file.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::setup(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(conn: Connection) -> Result<Db> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&conn)?;
        Ok(Db(Mutex::new(conn)))
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// An integer column that may have been written as a float.
pub fn int(row: &rusqlite::Row<'_>, idx: &str) -> rusqlite::Result<i64> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(i) => i,
        ValueRef::Real(f) => f as i64,
        ValueRef::Text(t) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        _ => 0,
    })
}

/// A number column that may be stored as an integer or a float.
pub fn num(row: &rusqlite::Row<'_>, idx: &str) -> rusqlite::Result<f64> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(i) => i as f64,
        ValueRef::Real(f) => f,
        ValueRef::Text(t) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0),
        _ => 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_run_twice_and_add_missing_columns() {
        let conn = Connection::open_in_memory().unwrap();
        // A database from before the added columns.
        conn.execute_batch(MIGRATIONS[1].replace("REFERENCES notebook(id) ON DELETE CASCADE", "").as_str()).unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let mut stmt = conn.prepare("PRAGMA table_info(source)").unwrap();
        let names: Vec<String> = stmt.query_map([], |r| r.get(1)).unwrap().map(|n| n.unwrap()).collect();
        for c in ["pending_transformations", "file_ref", "file_name", "file_type"] {
            assert!(names.iter().any(|n| n == c), "{c} added");
        }
    }

    #[test]
    fn opening_a_file_keeps_its_rows() {
        let dir = std::env::temp_dir().join(format!("on-db-{}", uuid::Uuid::new_v4()));
        let path = dir.join(FILE_NAME);
        {
            let db = Db::open(&path).unwrap();
            db.lock()
                .execute(
                    "INSERT INTO notebook (id, name, created, updated) VALUES ('notebook:1', 'Kept', '2026-11-01T12:00:00.000Z', '2026-11-01T12:00:00.000Z')",
                    [],
                )
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let name: String = db.lock().query_row("SELECT name FROM notebook", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Kept");
        std::fs::remove_dir_all(dir).ok();
    }
}

/**
 * Minimal SQL driver the engine needs. Parameters are positional and written
 * as $1, $2, ... in the order they first appear, which the Tauri SQL plugin
 * (sqlx), node:sqlite and sql.js all bind the same way.
 */
export interface SqlDriver {
  execute(sql: string, params?: unknown[]): Promise<{ rowsAffected: number }>
  select<T>(sql: string, params?: unknown[]): Promise<T[]>
}

const MIGRATIONS: string[] = [
  `CREATE TABLE IF NOT EXISTS notebook (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     archived INTEGER NOT NULL DEFAULT 0,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS source (
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
   )`,
  `CREATE INDEX IF NOT EXISTS source_notebook ON source(notebook_id)`,
  `CREATE TABLE IF NOT EXISTS chunk (
     id TEXT PRIMARY KEY,
     source_id TEXT NOT NULL REFERENCES source(id) ON DELETE CASCADE,
     position INTEGER NOT NULL,
     text TEXT NOT NULL,
     embedding TEXT,
     embedding_model TEXT
   )`,
  `CREATE INDEX IF NOT EXISTS chunk_source ON chunk(source_id)`,
  `CREATE TABLE IF NOT EXISTS note (
     id TEXT PRIMARY KEY,
     notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE,
     title TEXT,
     content TEXT NOT NULL DEFAULT '',
     note_type TEXT NOT NULL DEFAULT 'human',
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )`,
  `CREATE INDEX IF NOT EXISTS note_notebook ON note(notebook_id)`,
  `CREATE TABLE IF NOT EXISTS chat_session (
     id TEXT PRIMARY KEY,
     notebook_id TEXT NOT NULL REFERENCES notebook(id) ON DELETE CASCADE,
     title TEXT NOT NULL,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS chat_message (
     id TEXT PRIMARY KEY,
     session_id TEXT NOT NULL REFERENCES chat_session(id) ON DELETE CASCADE,
     type TEXT NOT NULL,
     content TEXT NOT NULL,
     timestamp TEXT NOT NULL
   )`,
  `CREATE INDEX IF NOT EXISTS chat_message_session ON chat_message(session_id, timestamp)`,
  `CREATE TABLE IF NOT EXISTS podcast_episode (
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
   )`,
  `CREATE INDEX IF NOT EXISTS podcast_episode_notebook ON podcast_episode(notebook_id, created)`,
  `CREATE TABLE IF NOT EXISTS transformation (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     title TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     prompt TEXT NOT NULL,
     apply_default INTEGER NOT NULL DEFAULT 0,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS source_insight (
     id TEXT PRIMARY KEY,
     source_id TEXT NOT NULL REFERENCES source(id) ON DELETE CASCADE,
     insight_type TEXT NOT NULL,
     content TEXT NOT NULL DEFAULT '',
     status TEXT NOT NULL DEFAULT 'completed',
     error TEXT,
     created TEXT NOT NULL,
     updated TEXT NOT NULL
   )`,
  `CREATE INDEX IF NOT EXISTS source_insight_source ON source_insight(source_id)`,
  `CREATE TABLE IF NOT EXISTS app_setting (key TEXT PRIMARY KEY, value TEXT NOT NULL)`,
  // A source can belong to several notebooks, as on the server. source.notebook_id stays as the one it was added to.
  `CREATE TABLE IF NOT EXISTS notebook_source (
     source_id TEXT NOT NULL,
     notebook_id TEXT NOT NULL,
     PRIMARY KEY (source_id, notebook_id)
   )`,
  `CREATE INDEX IF NOT EXISTS notebook_notebook_source ON notebook_source(notebook_id)`,
  `INSERT OR IGNORE INTO notebook_source (source_id, notebook_id) SELECT id, notebook_id FROM source`,
  `CREATE TABLE IF NOT EXISTS usage_event (
     id TEXT PRIMARY KEY,
     created TEXT NOT NULL,
     feature TEXT NOT NULL,
     model TEXT NOT NULL,
     input_tokens INTEGER NOT NULL DEFAULT 0,
     output_tokens INTEGER NOT NULL DEFAULT 0,
     audio_seconds REAL NOT NULL DEFAULT 0
   )`,
  `CREATE INDEX IF NOT EXISTS usage_event_created ON usage_event(created)`,
  `CREATE TABLE IF NOT EXISTS speaker_profile (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     voice_kind TEXT NOT NULL,
     voice_model TEXT NOT NULL,
     speakers TEXT NOT NULL
   )`,
  `CREATE TABLE IF NOT EXISTS episode_profile (
     id TEXT PRIMARY KEY,
     name TEXT NOT NULL,
     description TEXT NOT NULL DEFAULT '',
     speaker_profile_id TEXT NOT NULL,
     language TEXT,
     briefing TEXT NOT NULL,
     segments INTEGER NOT NULL DEFAULT 5
   )`,
]

/** Columns added after the first release, applied when missing. */
const COLUMNS: Array<[table: string, column: string, definition: string]> = [
  ['source', 'pending_transformations', 'TEXT'],
  ['chat_session', 'source_id', 'TEXT'],
  ['source', 'file_ref', 'TEXT'],
  ['source', 'file_name', 'TEXT'],
  ['source', 'file_type', 'TEXT'],
  ['podcast_episode', 'episode_profile_id', 'TEXT'],
  ['podcast_episode', 'speaker_profile_id', 'TEXT'],
]

/** Creates the schema. Safe to run on every launch. */
export async function migrate(db: SqlDriver): Promise<void> {
  await db.execute('PRAGMA foreign_keys = ON')
  for (const statement of MIGRATIONS) await db.execute(statement)
  for (const [table, column, definition] of COLUMNS) {
    const existing = await db.select<{ name: string }>(`PRAGMA table_info(${table})`)
    if (!existing.some((c) => c.name === column)) await db.execute(`ALTER TABLE ${table} ADD COLUMN ${column} ${definition}`)
  }
}

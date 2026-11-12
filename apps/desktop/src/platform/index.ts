// Platform services. Inside the Tauri app these use native plugins (SQLite,
// a CORS-free HTTP client, a key file in the app data folder). In a plain browser, which is
// only used for UI development, they fall back to sql.js, window.fetch and
// localStorage so the interface can still be exercised.
import type { SqlDriver } from '@open-notebook/engine'

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

let fetchPromise: Promise<typeof fetch> | undefined

/** fetch for provider and server calls. */
export function platformFetch(): Promise<typeof fetch> {
  fetchPromise ??= isTauri
    ? import('@tauri-apps/plugin-http').then((m) => m.fetch as typeof fetch)
    : Promise.resolve(window.fetch.bind(window))
  return fetchPromise
}

// SQLite numbers ?1, ?2 exactly; rewriting $1, $2 avoids depending on the
// order in which named parameters first appear in a statement.
const numbered = (sql: string) => sql.replace(/\$(\d+)/g, '?$1')

let dbPromise: Promise<SqlDriver> | undefined

export function openDatabase(): Promise<SqlDriver> {
  dbPromise ??= isTauri ? openTauriDatabase() : import('./browser-db.ts').then((m) => m.openBrowserDatabase())
  return dbPromise
}

async function openTauriDatabase(): Promise<SqlDriver> {
  const { default: Database } = await import('@tauri-apps/plugin-sql')
  const db = await Database.load('sqlite:open-notebook.db')
  return {
    async execute(sql, params) {
      const result = await db.execute(numbered(sql), params ?? [])
      return { rowsAffected: result.rowsAffected }
    },
    async select<T>(sql: string, params?: unknown[]) {
      return db.select<T[]>(numbered(sql), params ?? [])
    },
  }
}

const BROWSER_SECRET_PREFIX = 'open-notebook-dev-secret:'

/** API keys and server passwords. A file in the app data folder (or iCloud Keychain with sync); localStorage in browser dev only. */
export const secrets = {
  async get(name: string): Promise<string | null> {
    if (!isTauri) return localStorage.getItem(BROWSER_SECRET_PREFIX + name)
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<string | null>('secret_get', { name })
  },
  /** With sync, the key goes to iCloud Keychain; throws MISSING_ENTITLEMENT on unsigned builds. */
  async set(name: string, value: string, sync = false): Promise<void> {
    if (!isTauri) {
      if (sync) throw new Error(MISSING_ENTITLEMENT)
      return localStorage.setItem(BROWSER_SECRET_PREFIX + name, value)
    }
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('secret_set', { name, value, sync })
  },
  async delete(name: string): Promise<void> {
    if (!isTauri) return localStorage.removeItem(BROWSER_SECRET_PREFIX + name)
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('secret_delete', { name })
  },
  /** Whether this build can write to iCloud Keychain (signed with a keychain access group). */
  async syncAvailable(): Promise<boolean> {
    if (!isTauri) return false
    const { invoke } = await import('@tauri-apps/api/core')
    return invoke<boolean>('secret_sync_available')
  },
}

export const MISSING_ENTITLEMENT = 'missing-entitlement'

export { numbered as numberedParams }

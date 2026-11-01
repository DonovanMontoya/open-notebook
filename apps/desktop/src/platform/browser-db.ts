// Browser-only development database: sql.js in memory, saved to IndexedDB
// after writes so a page reload keeps your test data.
import type { SqlDriver } from '@open-notebook/engine'
import initSqlJs from 'sql.js'
import wasmUrl from 'sql.js/dist/sql-wasm.wasm?url'

import { numberedParams } from './index.ts'

const IDB_NAME = 'open-notebook-dev'
const IDB_STORE = 'db'
const IDB_KEY = 'sqlite'

function idb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(IDB_NAME, 1)
    req.onupgradeneeded = () => req.result.createObjectStore(IDB_STORE)
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error)
  })
}

async function load(): Promise<Uint8Array | undefined> {
  const db = await idb()
  return new Promise((resolve, reject) => {
    const req = db.transaction(IDB_STORE).objectStore(IDB_STORE).get(IDB_KEY)
    req.onsuccess = () => resolve(req.result as Uint8Array | undefined)
    req.onerror = () => reject(req.error)
  })
}

async function save(bytes: Uint8Array): Promise<void> {
  const db = await idb()
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readwrite')
    tx.objectStore(IDB_STORE).put(bytes, IDB_KEY)
    tx.oncomplete = () => resolve()
    tx.onerror = () => reject(tx.error)
  })
}

type Bindable = string | number | null | Uint8Array

const bind = (params: unknown[] = []): Bindable[] =>
  params.map((p) => (p === undefined ? null : typeof p === 'boolean' ? Number(p) : (p as Bindable)))

export async function openBrowserDatabase(): Promise<SqlDriver> {
  const SQL = await initSqlJs({ locateFile: () => wasmUrl })
  const db = new SQL.Database(await load())
  let timer: ReturnType<typeof setTimeout> | undefined
  const scheduleSave = () => {
    clearTimeout(timer)
    timer = setTimeout(() => void save(db.export()), 400)
  }

  return {
    async execute(sql, params) {
      db.run(numberedParams(sql), bind(params))
      const changed = db.getRowsModified()
      if (!/^\s*(PRAGMA|SELECT)/i.test(sql)) scheduleSave()
      return { rowsAffected: changed }
    },
    async select<T>(sql: string, params?: unknown[]) {
      const stmt = db.prepare(numberedParams(sql))
      try {
        stmt.bind(bind(params))
        const rows: T[] = []
        while (stmt.step()) rows.push(stmt.getAsObject() as T)
        return rows
      } finally {
        stmt.free()
      }
    },
  }
}

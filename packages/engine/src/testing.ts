// Test helpers: a node:sqlite driver and a deterministic fake provider.
import { DatabaseSync } from 'node:sqlite'

import type { SqlDriver } from './db.ts'
import type { ChatRequest, Provider } from './providers/types.ts'

export function memoryDb(): SqlDriver & { raw: DatabaseSync } {
  const raw = new DatabaseSync(':memory:')
  const bind = (params: unknown[] = []) =>
    params.map((p) => (p === undefined ? null : typeof p === 'boolean' ? Number(p) : p)) as Array<string | number | null>
  // SQLite numbers ?1, ?2 the same way sqlx numbers $1, $2; node:sqlite only binds the former positionally.
  const prepare = (sql: string) => raw.prepare(sql.replace(/\$(\d+)/g, '?$1'))
  return {
    raw,
    async execute(sql, params) {
      const result = prepare(sql).run(...bind(params))
      return { rowsAffected: Number(result.changes) }
    },
    async select<T>(sql: string, params?: unknown[]) {
      return prepare(sql).all(...bind(params)) as T[]
    },
  }
}

const VOCAB = 'abcdefghijklmnopqrstuvwxyz'

/** Letter-frequency "embedding": similar text gives similar vectors, deterministically. */
export function letterVector(text: string): number[] {
  const v = new Array(VOCAB.length).fill(0)
  for (const ch of text.toLowerCase()) {
    const i = VOCAB.indexOf(ch)
    if (i >= 0) v[i] += 1
  }
  return v
}

export interface FakeProvider extends Provider {
  chats: ChatRequest[]
  embedCalls: string[][]
  reply: string | ((request: ChatRequest) => string)
  failChat?: Error
}

export function fakeProvider(): FakeProvider {
  const provider: FakeProvider = {
    kind: 'openai',
    supportsEmbeddings: true,
    chats: [],
    embedCalls: [],
    reply: 'Fake answer [Doc]',
    async listModels() {
      return [{ id: 'fake-chat', capabilities: ['chat'] }]
    },
    async chat(request) {
      provider.chats.push(request)
      if (provider.failChat) throw provider.failChat
      const text = typeof provider.reply === 'function' ? provider.reply(request) : provider.reply
      for (const word of text.split(/(?<= )/)) request.onText?.(word)
      return text
    },
    async embed(_model, texts) {
      provider.embedCalls.push(texts)
      return texts.map(letterVector)
    },
  }
  return provider
}

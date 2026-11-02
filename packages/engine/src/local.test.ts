import assert from 'node:assert/strict'
import { test } from 'node:test'

import { FULL_CONTEXT_CHARS, LocalBackend, type AiConfig } from './local.ts'
import { fakeProvider, memoryDb } from './testing.ts'

const PROVIDER = { kind: 'openai' as const, apiKey: 'test' }

async function setup(config: AiConfig = { chat: { provider: PROVIDER, model: 'fake-chat' }, embedding: { provider: PROVIDER, model: 'fake-embed' } }) {
  const db = memoryDb()
  const provider = fakeProvider()
  const pages = new Map<string, { title: string; text: string }>()
  let tick = Date.parse('2026-11-01T12:00:00Z')
  const backend = await LocalBackend.open({
    db,
    fetch: (() => {
      throw new Error('no network in tests')
    }) as unknown as typeof fetch,
    extractUrl: async (url) => {
      const page = pages.get(url)
      if (!page) throw new Error(`404 for ${url}`)
      return page
    },
    getAiConfig: async () => config,
    now: () => new Date((tick += 1000)),
    createProvider: () => provider,
  })
  return { backend, db, provider, pages }
}

test('notebooks track source and note counts and sort by last update', async () => {
  const { backend } = await setup()
  const a = await backend.createNotebook('Alpha', 'first')
  const b = await backend.createNotebook('Beta')
  await backend.createNote(a.id, { content: 'A note' })
  await backend.addSource(a.id, { type: 'text', content: 'Some text', title: 'Doc' })
  await backend.idle()

  const list = await backend.listNotebooks()
  assert.deepEqual(list.map((n) => n.name), ['Alpha', 'Beta'])
  assert.equal(list[0].note_count, 1)
  assert.equal(list[0].source_count, 1)
  assert.equal((await backend.updateNotebook(b.id, { archived: true })).archived, true)
})

test('text sources are chunked, embedded and searchable', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('Research')
  const s = await backend.addSource(nb.id, { type: 'text', content: 'Zebras zigzag across the zoo.', title: 'Zoo facts' })
  assert.equal(s.status, 'processing')
  await backend.idle()

  const source = await backend.getSource(s.id)
  assert.equal(source.status, 'completed')
  assert.equal(source.embedded_chunks, 1)
  assert.equal(provider.embedCalls.length, 1)

  const hits = await backend.search('zebra zoo', nb.id)
  assert.equal(hits[0].id, s.id)
  assert.equal(hits[0].kind, 'source')
})

test('link sources take their text and title from the page', async () => {
  const { backend, pages } = await setup()
  pages.set('https://example.com/a', { title: 'Example article', text: 'Body of the article.' })
  const nb = await backend.createNotebook('Links')
  const s = await backend.addSource(nb.id, { type: 'link', url: 'https://example.com/a' })
  await backend.idle()

  const source = await backend.getSource(s.id)
  assert.equal(source.title, 'Example article')
  assert.equal(source.full_text, 'Body of the article.')
  assert.equal(source.asset?.url, 'https://example.com/a')
})

test('a failed source records the error and can be retried', async () => {
  const { backend, pages } = await setup()
  const nb = await backend.createNotebook('Links')
  const s = await backend.addSource(nb.id, { type: 'link', url: 'https://example.com/missing' })
  await backend.idle()
  let source = await backend.getSource(s.id)
  assert.equal(source.status, 'failed')
  assert.match(String(source.processing_info?.error), /404/)

  pages.set('https://example.com/missing', { title: 'Back', text: 'Now it exists.' })
  await backend.retrySource(s.id)
  await backend.idle()
  source = await backend.getSource(s.id)
  assert.equal(source.status, 'completed')
  assert.equal(source.processing_info, undefined)
})

test('sources still processing when the app closed are marked failed on next open', async () => {
  const { db } = await setup()
  await db.execute(`INSERT INTO notebook (id, name, created, updated) VALUES ('nb', 'x', 'now', 'now')`)
  await db.execute(
    `INSERT INTO source (id, notebook_id, kind, status, created, updated) VALUES ('s', 'nb', 'text', 'processing', 'now', 'now')`,
  )
  const reopened = await LocalBackend.open({
    db,
    fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({}),
  })
  const source = await reopened.getSource('s')
  assert.equal(source.status, 'failed')
  assert.match(String(source.processing_info?.error), /interrupted/)
})

test('chat sends small notebooks whole, streams the reply and saves the session', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('Chat')
  await backend.addSource(nb.id, { type: 'text', content: 'The launch date is 3 March.', title: 'Plan' })
  await backend.createNote(nb.id, { title: 'Reminder', content: 'Ask about budget.' })
  await backend.idle()

  const deltas: string[] = []
  const first = await backend.sendChatMessage({ notebookId: nb.id, message: 'When is launch?', onText: (d) => deltas.push(d) })
  assert.equal(deltas.join(''), 'Fake answer [Doc]')
  assert.match(provider.chats[0].system, /<source title="Plan">\nThe launch date is 3 March\./)
  assert.match(provider.chats[0].system, /<note title="Reminder">/)
  assert.deepEqual(first.messages.map((m) => m.type), ['human', 'ai'])
  assert.equal(first.session.title, 'When is launch?')

  await backend.sendChatMessage({ notebookId: nb.id, sessionId: first.session.id, message: 'And budget?' })
  assert.deepEqual(
    provider.chats[1].messages.map((m) => m.role),
    ['user', 'assistant', 'user'],
    'earlier turns are sent as history',
  )
  const sessions = await backend.listChatSessions(nb.id)
  assert.equal(sessions.length, 1)
  assert.equal(sessions[0].message_count, 4)
})

test('chat on a large notebook sends short sources whole and excerpts of long ones', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('Big')
  const filler = 'lorem ipsum dolor sit amet '.repeat(Math.ceil(FULL_CONTEXT_CHARS / 27) + 10)
  await backend.addSource(nb.id, { type: 'text', content: filler, title: 'Filler' })
  await backend.addSource(nb.id, { type: 'text', content: 'Xylophones quickly vex wizards.', title: 'Needle' })
  await backend.idle()

  await backend.sendChatMessage({ notebookId: nb.id, message: 'xylophone wizards' })
  const system = provider.chats[0].system
  assert.match(system, /<source title="Needle">\nXylophones quickly vex wizards\./)
  assert.match(system, /<source title="Filler" excerpts="true">/)
  assert.ok(system.length < FULL_CONTEXT_CHARS + 2_000, 'prompt stays near the full-context budget')
})

test('chat on a large notebook includes every source, not only the closest matches', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('Eight')
  const long = (n: number) => `Source ${n} opening line.\n\n` + `paragraph about topic ${n} with details. `.repeat(Math.ceil(FULL_CONTEXT_CHARS / 8 / 40) + 200)
  for (let n = 1; n <= 8; n++) await backend.addSource(nb.id, { type: 'text', content: long(n), title: `Paper ${n}` })
  await backend.idle()

  await backend.sendChatMessage({ notebookId: nb.id, message: 'Give me a two-sentence summary of each source' })
  const system = provider.chats[0].system
  for (let n = 1; n <= 8; n++) {
    assert.match(system, new RegExp(`<source title="Paper ${n}" excerpts="true">\\nSource ${n} opening line`), `Paper ${n} is represented by its opening`)
  }
  assert.ok(system.length < FULL_CONTEXT_CHARS + 4_000, 'prompt stays near the full-context budget')
})

test('a failed chat request saves nothing', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('Chat')
  provider.failChat = new Error('API key rejected')
  await assert.rejects(backend.sendChatMessage({ notebookId: nb.id, message: 'Hi' }), /API key rejected/)
  assert.equal((await backend.listChatSessions(nb.id)).length, 0)
})

test('chat without a configured model explains what to do', async () => {
  const { backend } = await setup({})
  const nb = await backend.createNotebook('Chat')
  await assert.rejects(backend.sendChatMessage({ notebookId: nb.id, message: 'Hi' }), /Choose a chat model/)
})

test('without an embedding model, sources still complete and search falls back to keywords', async () => {
  const { backend, provider } = await setup({ chat: { provider: PROVIDER, model: 'fake-chat' } })
  const nb = await backend.createNotebook('Keywords')
  const s = await backend.addSource(nb.id, { type: 'text', content: 'Photosynthesis converts light.', title: 'Bio' })
  await backend.createNote(nb.id, { title: 'Idea', content: 'Light and plants' })
  await backend.idle()

  assert.equal((await backend.getSource(s.id)).status, 'completed')
  assert.equal(provider.embedCalls.length, 0)
  const hits = await backend.search('light')
  assert.deepEqual(hits.map((h) => h.kind).sort(), ['note', 'source'])
})

test('deleting a notebook removes its sources, chunks, notes and chats', async () => {
  const { backend, db } = await setup()
  const nb = await backend.createNotebook('Temp')
  await backend.addSource(nb.id, { type: 'text', content: 'Text', title: 'T' })
  await backend.createNote(nb.id, { content: 'n' })
  await backend.idle()
  await backend.sendChatMessage({ notebookId: nb.id, message: 'hi' })

  await backend.deleteNotebook(nb.id)
  for (const table of ['notebook', 'source', 'chunk', 'note', 'chat_session', 'chat_message']) {
    const [row] = await db.select<{ n: number }>(`SELECT COUNT(*) AS n FROM ${table}`)
    assert.equal(row.n, 0, `${table} is empty`)
  }
})

test('a batch of sources is processed two at a time and all complete', async () => {
  const { backend, provider } = await setup()
  let inFlight = 0
  let peak = 0
  const embed = provider.embed.bind(provider)
  provider.embed = async (model, texts) => {
    inFlight++
    peak = Math.max(peak, inFlight)
    await new Promise((r) => setTimeout(r, 5))
    inFlight--
    return embed(model, texts)
  }
  const nb = await backend.createNotebook('Batch')
  for (let i = 0; i < 6; i++) await backend.addSource(nb.id, { type: 'text', content: `Document ${i}`, title: `Doc ${i}` })
  await backend.idle()
  const sources = await backend.listSources(nb.id)
  assert.equal(sources.length, 6)
  assert.ok(sources.every((s) => s.status === 'completed'))
  assert.equal(peak, 2)
})

test('a source can be in several notebooks; deleting one notebook keeps shared sources', async () => {
  const { backend } = await setup()
  const a = await backend.createNotebook('A')
  const b = await backend.createNotebook('B')
  const shared = await backend.addSource(a.id, { type: 'text', content: 'Shared text', title: 'Shared' })
  const only = await backend.addSource(a.id, { type: 'text', content: 'Only in A', title: 'Only' })
  await backend.idle()

  await backend.linkSource(shared.id, b.id)
  assert.deepEqual((await backend.listSources(b.id)).map((s) => s.title), ['Shared'])
  assert.deepEqual((await backend.getSource(shared.id)).notebooks, [a.id, b.id])
  assert.equal((await backend.getNotebook(b.id)).source_count, 1)
  assert.equal((await backend.listAllSources()).find((s) => s.id === shared.id)?.notebooks?.length, 2)

  await backend.updateSource(shared.id, { title: 'Renamed' })
  await backend.deleteNotebook(a.id)
  assert.equal((await backend.getSource(shared.id)).title, 'Renamed')
  assert.deepEqual((await backend.getSource(shared.id)).notebooks, [b.id])
  await assert.rejects(backend.getSource(only.id), /not found/)

  await backend.unlinkSource(shared.id, b.id)
  assert.equal((await backend.listSources(b.id)).length, 0)
})

test('audio files are kept, transcribed with the speech to text model, and downloadable', async () => {
  const files = new Map<string, Uint8Array>()
  const calls: string[] = []
  const transcriber = (async (url: string, init: RequestInit) => {
    calls.push(url)
    const form = init.body as FormData
    assert.equal(form.get('model'), 'gpt-4o-transcribe')
    return Response.json({ text: 'Welcome to the interview about dark patterns.' })
  }) as unknown as typeof fetch
  const backend = await LocalBackend.open({
    db: memoryDb(),
    fetch: transcriber,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({ transcription: { provider: { kind: 'openai', apiKey: 'k' }, model: 'gpt-4o-transcribe' } }),
    files: {
      async save(id, data) {
        files.set(id, data)
        return id
      },
      async read(ref) {
        return files.get(ref)!.slice().buffer
      },
      async remove(ref) {
        files.delete(ref)
      },
    },
  })
  const nb = await backend.createNotebook('Interviews')
  const audio = new Blob([new Uint8Array([1, 2, 3, 4])], { type: 'audio/mpeg' })
  const s = await backend.addSource(nb.id, { type: 'document', title: 'Interview', content: '', fileName: 'interview.mp3', file: audio })
  await backend.idle()

  const source = await backend.getSource(s.id)
  assert.equal(source.status, 'completed', String(source.processing_info?.error))
  assert.equal(source.full_text, 'Welcome to the interview about dark patterns.')
  assert.equal(source.asset?.file_path, 'interview.mp3')
  assert.match(calls[0], /\/audio\/transcriptions$/)

  const original = await backend.downloadSource(s.id)
  assert.deepEqual([...new Uint8Array(original!.data)], [1, 2, 3, 4])
  assert.equal(original!.name, 'interview.mp3')

  await backend.deleteSource(s.id)
  assert.equal(files.size, 0, 'deleting the source deletes the file')
})

test('audio without a speech to text model fails with a clear reason', async () => {
  const backend = await LocalBackend.open({
    db: memoryDb(),
    fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({}),
    files: { save: async (id) => id, read: async () => new ArrayBuffer(4), remove: async () => undefined },
  })
  const nb = await backend.createNotebook('N')
  const s = await backend.addSource(nb.id, { type: 'document', title: 'Clip', content: '', fileName: 'clip.m4a', file: new Blob([new Uint8Array(4)]) })
  await backend.idle()
  assert.match(String((await backend.getSource(s.id)).processing_info?.error), /speech to text model/)
})

test('calls are recorded in the usage log and summarized by feature and model', async () => {
  const { backend } = await setup()
  const nb = await backend.createNotebook('N')
  await backend.addSource(nb.id, { type: 'text', content: 'Text to embed', title: 'T' })
  await backend.idle()
  await backend.sendChatMessage({ notebookId: nb.id, message: 'Hello there' })
  const report = await backend.usage(30)
  assert.ok(report.estimated)
  const chat = report.groups.find((g) => g.feature === 'chat')!
  assert.equal(chat.model, 'fake-chat')
  assert.equal(chat.calls, 1)
  assert.ok(chat.inputTokens > 0 && chat.outputTokens > 0)
  assert.ok(report.groups.some((g) => g.feature === 'embedding' && g.model === 'fake-embed'))
})

test('rebuilding the index re-processes every source', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('N')
  await backend.addSource(nb.id, { type: 'text', content: 'One', title: 'A' })
  await backend.addSource(nb.id, { type: 'text', content: 'Two', title: 'B' })
  await backend.idle()
  const before = provider.embedCalls.length
  assert.equal(await backend.rebuildIndex(), 2)
  await backend.idle()
  assert.equal(provider.embedCalls.length, before + 2)
  assert.ok((await backend.listSources(nb.id)).every((s) => s.status === 'completed'))
})

test('a Live talk session is created with the server’s settings and an OpenAI backend model', async () => {
  const calls: Array<{ url: string; body: { session: { model: string; delegation: { responses: { model: string } } }; transport: { sdp: string } } }> = []
  const backend = await LocalBackend.open({
    db: memoryDb(),
    fetch: (async (url: string, init: RequestInit) => {
      calls.push({ url, body: JSON.parse(init.body as string) })
      return Response.json({ session: { id: 's' }, transport: { type: 'webrtc', sdp: 'v=0 answer' } })
    }) as unknown as typeof fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({
      chat: { provider: { kind: 'openai', apiKey: 'sk' }, model: 'gpt-6.1-sol' },
      providers: { openai: { kind: 'openai', apiKey: 'sk' } },
    }),
  })
  assert.equal(await backend.createLiveSession('v=0 offer', 'cedar'), 'v=0 answer')
  assert.equal(calls[0].url, 'https://api.openai.com/v1/live/sessions')
  assert.equal(calls[0].body.session.model, 'gpt-live-1')
  assert.equal(calls[0].body.session.delegation.responses.model, 'gpt-6.1-sol')
  assert.equal(calls[0].body.transport.sdp, 'v=0 offer')
  await assert.rejects(backend.createLiveSession('nope', 'marin'), /Invalid WebRTC offer/)
})

test('a Live talk session started from a notebook knows its sources and notes', async () => {
  const calls: Array<{ session: { instructions: string; delegation: { responses: { instructions: string } } } }> = []
  const db = memoryDb()
  const provider = fakeProvider()
  const backend = await LocalBackend.open({
    db,
    fetch: (async (_url: string, init: RequestInit) => {
      calls.push(JSON.parse(init.body as string))
      return Response.json({ transport: { type: 'webrtc', sdp: 'v=0 answer' } })
    }) as unknown as typeof fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    createProvider: () => provider,
    getAiConfig: async () => ({
      chat: { provider: { kind: 'openai', apiKey: 'sk' }, model: 'gpt-6.1-sol' },
      providers: { openai: { kind: 'openai', apiKey: 'sk' } },
    }),
  })
  const nb = await backend.createNotebook('Dark patterns')
  await backend.addSource(nb.id, { type: 'text', title: 'FTC report', content: 'Roach motels make leaving hard.' })
  await backend.createNote(nb.id, { title: 'My take', content: 'Cancellation is the worst offender.' })
  await backend.idle()

  await backend.createLiveSession('v=0 offer', 'marin', nb.id)
  const session = calls.at(-1)!.session
  assert.match(session.instructions, /notebook "Dark patterns"/)
  assert.match(session.instructions, /Source: FTC report/)
  assert.doesNotMatch(session.instructions, /no access to notebook sources/)
  assert.match(session.delegation.responses.instructions, /Roach motels make leaving hard\./)
  assert.match(session.delegation.responses.instructions, /Cancellation is the worst offender\./)
})

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { LocalBackend } from '../local.ts'
import { fakeProvider, memoryDb } from '../testing.ts'
import { DEFAULT_TRANSFORMATIONS } from './seeds.ts'

async function setup() {
  const db = memoryDb()
  const provider = fakeProvider()
  provider.reply = (req) => (req.system.includes('SUMMARY-PROMPT') ? 'A short summary.' : 'Chat answer')
  const backend = await LocalBackend.open({
    db,
    fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({ chat: { provider: { kind: 'openai' }, model: 'm' } }),
    createProvider: () => provider,
  })
  return { backend, db, provider }
}

test('the server’s default transformations are added once', async () => {
  const { backend, db } = await setup()
  const list = await backend.listTransformations()
  assert.equal(list.length, DEFAULT_TRANSFORMATIONS.length)
  assert.equal(list[0].name, 'Dense Summary', 'the default-on transformation sorts first')
  await backend.deleteTransformation(list[1].id)
  await backend.seedTransformations()
  assert.equal((await backend.listTransformations()).length, DEFAULT_TRANSFORMATIONS.length - 1, 'deleted ones stay deleted')
  assert.ok((await db.select('SELECT * FROM app_setting')).length >= 1)
})

test('transformations can be created, edited and run with the shared instructions', async () => {
  const { backend, provider } = await setup()
  const t = await backend.saveTransformation({ name: 'tldr', title: 'TL;DR', description: 'd', prompt: 'SUMMARY-PROMPT', applyDefault: false })
  const edited = await backend.saveTransformation({ ...t, title: 'Too long' })
  assert.equal(edited.title, 'Too long')
  await backend.setTransformationInstructions('HOUSE RULES')
  assert.equal(await backend.runTransformation(t.id, 'Some text'), 'A short summary.')
  const call = provider.chats.at(-1)!
  assert.match(call.system, /^HOUSE RULES\n\nSUMMARY-PROMPT$/)
  assert.equal(call.messages[0].content, 'Some text')
})

test('transformations chosen when adding a source become insights, which can be saved as notes', async () => {
  const { backend } = await setup()
  const t = await backend.saveTransformation({ name: 's', title: 'Summary', description: '', prompt: 'SUMMARY-PROMPT', applyDefault: true })
  const nb = await backend.createNotebook('N')
  const s = await backend.addSource(nb.id, { type: 'text', content: 'Long text', title: 'Doc', transformations: [t.id] })
  await backend.idle()
  const insights = await backend.listInsights(s.id)
  assert.deepEqual(insights.map((i) => [i.type, i.content, i.status]), [['Summary', 'A short summary.', 'completed']])
  assert.equal((await backend.getSource(s.id)).insights_count, 1)

  const note = await backend.saveInsightAsNote(insights[0].id, nb.id)
  assert.equal(note.title, 'Summary: Doc')
  assert.equal(note.note_type, 'ai')

  await backend.deleteInsight(insights[0].id)
  assert.equal((await backend.listInsights(s.id)).length, 0)
})

test('a failing transformation leaves a failed insight with the reason', async () => {
  const { backend, provider } = await setup()
  const t = (await backend.listTransformations())[0]
  const nb = await backend.createNotebook('N')
  const s = await backend.addSource(nb.id, { type: 'text', content: 'Text', title: 'Doc' })
  await backend.idle()
  provider.failChat = new Error('Rate limited')
  await backend.createInsight(s.id, t.id)
  await backend.idle()
  const [insight] = await backend.listInsights(s.id)
  assert.equal(insight.status, 'failed')
  assert.match(insight.error!, /Rate limited/)
})

test('chat context follows the chosen modes for sources and notes', async () => {
  const { backend, provider } = await setup()
  const t = await backend.saveTransformation({ name: 's', title: 'Summary', description: '', prompt: 'SUMMARY-PROMPT', applyDefault: false })
  const nb = await backend.createNotebook('N')
  const a = await backend.addSource(nb.id, { type: 'text', content: 'ALPHA FULL TEXT', title: 'Alpha', transformations: [t.id] })
  const b = await backend.addSource(nb.id, { type: 'text', content: 'BETA FULL TEXT', title: 'Beta' })
  const note = await backend.createNote(nb.id, { title: 'Private', content: 'SECRET NOTE' })
  await backend.idle()

  await backend.sendChatMessage({ notebookId: nb.id, message: 'q' })
  let system = provider.chats.at(-1)!.system
  assert.match(system, /A short summary\./, 'a source with insights sends its insights by default')
  assert.doesNotMatch(system, /ALPHA FULL TEXT/)
  assert.match(system, /BETA FULL TEXT/, 'a source without insights sends its full text')
  assert.match(system, /SECRET NOTE/)

  await backend.sendChatMessage({
    notebookId: nb.id,
    message: 'q',
    context: { sources: { [a.id]: 'full', [b.id]: 'off' }, notes: { [note.id]: false } },
  })
  system = provider.chats.at(-1)!.system
  assert.match(system, /ALPHA FULL TEXT/)
  assert.doesNotMatch(system, /BETA FULL TEXT/)
  assert.doesNotMatch(system, /SECRET NOTE/)
})

test('source chat talks about one source and keeps its own sessions', async () => {
  const { backend, provider } = await setup()
  const nb = await backend.createNotebook('N')
  const a = await backend.addSource(nb.id, { type: 'text', content: 'ALPHA TEXT', title: 'Alpha' })
  await backend.addSource(nb.id, { type: 'text', content: 'BETA TEXT', title: 'Beta' })
  await backend.idle()

  const first = await backend.sendSourceChatMessage({ sourceId: a.id, message: 'What is this?' })
  const system = provider.chats.at(-1)!.system
  assert.match(system, /ALPHA TEXT/)
  assert.doesNotMatch(system, /BETA TEXT/, 'other sources stay out')
  await backend.sendSourceChatMessage({ sourceId: a.id, sessionId: first.session.id, message: 'More?' })
  assert.equal(provider.chats.at(-1)!.messages.length, 3)

  assert.equal((await backend.listSourceChatSessions(a.id)).length, 1)
  assert.equal((await backend.listChatSessions(nb.id)).length, 0, 'source chats are not notebook chats')

  await backend.renameChatSession(first.session.id, 'Alpha questions')
  assert.equal((await backend.getSourceChatSession(a.id, first.session.id)).title, 'Alpha questions')

  await backend.deleteSource(a.id)
  assert.equal((await backend.listSourceChatSessions(a.id)).length, 0)
})

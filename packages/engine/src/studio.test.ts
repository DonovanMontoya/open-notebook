import assert from 'node:assert/strict'
import { test } from 'node:test'

import { LocalBackend } from './local.ts'
import { parseDeck, STUDY_PREFIX } from './studio.ts'
import { fakeProvider, memoryDb } from './testing.ts'

const quiz = (sourceId: string, count = 2) =>
  JSON.stringify({
    version: 1,
    kind: 'quiz',
    title: 'Dark patterns quiz',
    items: Array.from({ length: count }, (_, i) => ({
      question: `Q${i}`,
      answer: 'Because the report says so.',
      options: ['a', 'b', 'c', 'd'],
      correct_index: 1,
      source_ids: [sourceId],
    })),
  })

test('parseDeck rejects decks that break the server’s rules', () => {
  const req = { kind: 'quiz' as const, count: 2, sourceIds: ['s1'] }
  assert.equal(parseDeck('```json\n' + quiz('s1') + '\n```', req).items.length, 2)
  assert.throws(() => parseDeck(quiz('s1', 3), req), /invalid study deck/, 'wrong count')
  assert.throws(() => parseDeck(quiz('other'), req), /invalid study deck/, 'cites an unknown source')
  const dupes = JSON.parse(quiz('s1'))
  dupes.items[0].options = ['a', 'a', 'b', 'c']
  assert.throws(() => parseDeck(JSON.stringify(dupes), req), /invalid study deck/, 'repeated choices')
  const flash = { version: 1, kind: 'flashcards', title: 'Cards', items: [{ question: 'q', answer: 'a', options: [], correct_index: null, source_ids: ['s1'] }] }
  assert.equal(parseDeck(JSON.stringify(flash), { kind: 'flashcards', count: 1, sourceIds: ['s1'] }).kind, 'flashcards')
})

test('a generated deck is saved as a note the server would recognize and listed back', async () => {
  const provider = fakeProvider()
  const backend = await LocalBackend.open({
    db: memoryDb(),
    fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({ chat: { provider: { kind: 'openai' }, model: 'm' } }),
    createProvider: () => provider,
  })
  const nb = await backend.createNotebook('N')
  const s = await backend.addSource(nb.id, { type: 'text', content: 'The FTC names four tactics.', title: 'FTC' })
  await backend.idle()
  provider.reply = quiz(s.id)

  const deck = await backend.generateStudyDeck(nb.id, { kind: 'quiz', sourceIds: [s.id], count: 2, difficulty: 'medium' })
  assert.equal(deck.title, 'Dark patterns quiz')
  assert.match(provider.chats[0].system, /Exactly 2 items/)
  assert.match(provider.chats[0].messages[0].content, /"source_id":/)

  const notes = await backend.listNotes(nb.id)
  assert.ok(notes[0].content!.startsWith(STUDY_PREFIX))
  const decks = await backend.listStudyDecks(nb.id)
  assert.equal(decks.length, 1)
  assert.equal(decks[0].noteId, notes[0].id)

  await assert.rejects(backend.generateStudyDeck(nb.id, { kind: 'quiz', sourceIds: ['source:elsewhere'], count: 2, difficulty: 'easy' }), /from this notebook/)
})

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { parseStrategy } from './ask.ts'
import { LocalBackend } from './local.ts'
import { fakeProvider, memoryDb } from './testing.ts'

test('parseStrategy reads the plan and caps the searches', () => {
  const plan = parseStrategy('```json\n{"reasoning":"r","searches":[{"term":"a","instructions":"x"},{"term":""},{"term":"b"},{"term":"c"},{"term":"d"},{"term":"e"}]}\n```')
  assert.equal(plan.reasoning, 'r')
  assert.deepEqual(plan.searches.map((s) => s.term), ['a', 'b', 'c', 'd'])
  assert.deepEqual(parseStrategy('not json').searches, [])
})

test('ask plans searches, answers each from matching documents, then writes a cited final answer', async () => {
  const provider = fakeProvider()
  provider.reply = (req) => {
    const msg = req.messages[0].content
    if (msg.includes('plan how to find')) return JSON.stringify({ reasoning: 'Look for launch info', searches: [{ term: 'launch date', instructions: 'Find the date' }] })
    if (msg.includes('# Search')) {
      const id = /<document id="([^"]+)"/.exec(msg)?.[1]
      return `It launches in March [${id}].`
    }
    if (msg.includes('Partial answers')) return msg.match(/It launches in March \[[^\]]+\]\./)![0]
    return '?'
  }
  const backend = await LocalBackend.open({
    db: memoryDb(),
    fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({ chat: { provider: { kind: 'openai' }, model: 'm' } }),
    createProvider: () => provider,
  })
  const nb = await backend.createNotebook('Plans')
  const src = await backend.addSource(nb.id, { type: 'text', content: 'The launch date is in March.', title: 'Launch plan' })
  await backend.createNote(nb.id, { title: 'Launch worries', content: 'What if the launch date slips?' })
  await backend.idle()

  const steps: string[] = []
  const result = await backend.ask('When is the launch?', { onStep: (s) => steps.push(s.kind) })
  assert.deepEqual(steps, ['strategy', 'answer', 'final'])
  assert.equal(result.answer, `It launches in March [${src.id}].`)
  assert.ok(result.documents.some((d) => d.id === src.id && d.title === 'Launch plan' && d.notebookId === nb.id))
  assert.ok(result.documents.some((d) => d.title === 'Launch worries'), 'matching notes are searched too')
})

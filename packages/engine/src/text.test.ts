import assert from 'node:assert/strict'
import { test } from 'node:test'

import { buildChatPrompt, fairShare } from './chat.ts'
import { chunkText, cosineSimilarity, decodeVector, encodeVector, parseLinks, titleFrom } from './text.ts'

test('chunkText keeps short text whole and splits long text with overlap', () => {
  assert.deepEqual(chunkText('  short  '), ['short'])
  assert.deepEqual(chunkText(''), [])

  const paragraph = (n: number) => `Paragraph ${n} ` + 'word '.repeat(80)
  const text = Array.from({ length: 12 }, (_, i) => paragraph(i)).join('\n\n')
  const chunks = chunkText(text, 1000, 100)
  assert.ok(chunks.length > 3)
  for (const c of chunks) assert.ok(c.length <= 1000 + 100, `chunk of ${c.length} chars is within bounds`)
  assert.ok(chunks.join(' ').includes('Paragraph 11'), 'nothing is dropped')
})

test('chunkText hard-splits a single enormous sentence', () => {
  const chunks = chunkText('x'.repeat(5000), 1000, 0)
  assert.equal(chunks.length, 5)
})

test('vectors survive base64 round trips and cosine similarity behaves', () => {
  const v = [0.25, -1.5, 3, 0]
  assert.deepEqual([...decodeVector(encodeVector(v))], v)
  assert.equal(cosineSimilarity([1, 0], [1, 0]), 1)
  assert.equal(cosineSimilarity([1, 0], [0, 1]), 0)
  assert.equal(cosineSimilarity([1, 0], [1, 0, 0]), 0)
})

test('titleFrom uses the first line and truncates', () => {
  assert.equal(titleFrom('Hello\nworld'), 'Hello')
  assert.equal(titleFrom('a'.repeat(100), 10), 'aaaaaaaaa…')
})

test('chat prompt cites sources by title and notes when nothing is processed', () => {
  const prompt = buildChatPrompt({
    notebook: { name: 'N "1"', description: '' },
    sources: [],
    chunks: [],
    notes: [{ title: 'Line\nbreak', content: 'body' }],
  })
  assert.match(prompt, /notebook "N "1""/)
  assert.match(prompt, /no processed sources yet/)
  assert.match(prompt, /<note title="Line break">/)
})

test('parseLinks finds every address in pasted text, once each', () => {
  const text = `Read https://a.com/x, then (https://b.org/paper.pdf).
https://a.com/x
arxiv.org/abs/2203.02155 and not-a-link`
  assert.deepEqual(parseLinks(text), ['https://a.com/x', 'https://b.org/paper.pdf', 'https://arxiv.org/abs/2203.02155'])
})

test('fairShare gives short sources what they need and splits the rest evenly', () => {
  assert.deepEqual(fairShare([10, 1000, 1000], 610), [10, 300, 300])
  assert.deepEqual(fairShare([5, 5], 100), [5, 5])
  assert.deepEqual(fairShare([], 100), [])
})

test('chat prompt keeps whole sources and excerpts together', () => {
  const prompt = buildChatPrompt({
    notebook: { name: 'N', description: '' },
    sources: [{ title: 'Short (insights)', text: 'summary' }],
    chunks: [
      { title: 'Long', text: 'opening' },
      { title: 'Long', text: 'later' },
    ],
    notes: [],
  })
  assert.match(prompt, /<source title="Short \(insights\)">/)
  assert.match(prompt, /<source title="Long" excerpts="true">\nopening\n\[…\]\nlater\n<\/source>/)
})

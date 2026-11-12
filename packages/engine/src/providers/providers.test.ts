import assert from 'node:assert/strict'
import { test } from 'node:test'

import { readSse } from './http.ts'
import { createProvider } from './index.ts'

type Call = { url: string; init: RequestInit }

function sse(events: string[]): ReadableStream<Uint8Array> {
  const text = events.map((e) => `data: ${e}\n\n`).join('')
  const bytes = new TextEncoder().encode(text)
  // Split mid-event to exercise buffering.
  return new ReadableStream({
    start(controller) {
      controller.enqueue(bytes.slice(0, 7))
      controller.enqueue(bytes.slice(7))
      controller.close()
    },
  })
}

function fakeFetch(handler: (url: string, init: RequestInit) => Response, calls: Call[] = []): typeof fetch {
  return (async (input: string | URL | Request, init: RequestInit = {}) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
    calls.push({ url, init })
    return handler(url, init)
  }) as typeof fetch
}

test('readSse joins events split across reads and handles a missing final blank line', async () => {
  const bytes = new TextEncoder().encode('data: one\n\ndata: two\r\n\r\ndata: three')
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      for (const b of bytes) c.enqueue(new Uint8Array([b]))
      c.close()
    },
  })
  const out: string[] = []
  for await (const d of readSse(body)) out.push(d)
  assert.deepEqual(out, ['one', 'two', 'three'])
})

test('OpenAI-compatible chat streams deltas and sends the system prompt first', async () => {
  const calls: Call[] = []
  const provider = createProvider(
    { kind: 'openai-compatible', baseUrl: 'https://openrouter.ai/api/v1/', apiKey: 'k' },
    fakeFetch(
      () =>
        new Response(
          sse([
            JSON.stringify({ choices: [{ delta: { role: 'assistant' } }] }),
            JSON.stringify({ choices: [{ delta: { content: 'Hel' } }] }),
            JSON.stringify({ choices: [{ delta: { content: 'lo' } }] }),
            '[DONE]',
          ]),
        ),
      calls,
    ),
  )
  const deltas: string[] = []
  const text = await provider.chat({
    model: 'm',
    system: 'sys',
    messages: [{ role: 'user', content: 'hi' }],
    onText: (d) => deltas.push(d),
  })
  assert.equal(text, 'Hello')
  assert.deepEqual(deltas, ['Hel', 'lo'])
  assert.equal(calls[0].url, 'https://openrouter.ai/api/v1/chat/completions')
  const body = JSON.parse(calls[0].init.body as string)
  assert.deepEqual(body.messages[0], { role: 'system', content: 'sys' })
  assert.equal((calls[0].init.headers as Record<string, string>).Authorization, 'Bearer k')
})

test('OpenAI embeddings keep input order and batch large inputs', async () => {
  const calls: Call[] = []
  const provider = createProvider(
    { kind: 'openai', apiKey: 'k' },
    fakeFetch((_url, init) => {
      const input = JSON.parse(init.body as string).input as string[]
      const data = input.map((t, index) => ({ index, embedding: [t.length] })).reverse()
      return Response.json({ data })
    }, calls),
  )
  const texts = Array.from({ length: 70 }, (_, i) => 'x'.repeat(i + 1))
  const vectors = await provider.embed('text-embedding-3-small', texts)
  assert.equal(calls.length, 2)
  assert.deepEqual(vectors.map((v) => v[0]), texts.map((t) => t.length))
})

test('Gemini chat maps roles, skips thought parts and reads the SSE stream', async () => {
  const calls: Call[] = []
  const provider = createProvider(
    { kind: 'gemini', apiKey: 'g' },
    fakeFetch(
      () =>
        new Response(
          sse([
            JSON.stringify({ candidates: [{ content: { parts: [{ text: 'thinking', thought: true }] } }] }),
            JSON.stringify({ candidates: [{ content: { parts: [{ text: 'Answer' }] } }] }),
          ]),
        ),
      calls,
    ),
  )
  const text = await provider.chat({
    model: 'gemini-x',
    system: 'sys',
    messages: [
      { role: 'user', content: 'a' },
      { role: 'assistant', content: 'b' },
      { role: 'user', content: 'c' },
    ],
  })
  assert.equal(text, 'Answer')
  assert.match(calls[0].url, /models\/gemini-x:streamGenerateContent\?alt=sse$/)
  const body = JSON.parse(calls[0].init.body as string)
  assert.deepEqual(body.contents.map((c: { role: string }) => c.role), ['user', 'model', 'user'])
  assert.equal((calls[0].init.headers as Record<string, string>)['x-goog-api-key'], 'g')
})

test('Gemini model list keeps chat and embedding capabilities', async () => {
  const provider = createProvider(
    { kind: 'gemini', apiKey: 'g' },
    fakeFetch(() =>
      Response.json({
        models: [
          { name: 'models/chat-1', supportedGenerationMethods: ['generateContent'] },
          { name: 'models/embed-1', supportedGenerationMethods: ['embedContent'] },
        ],
      }),
    ),
  )
  const models = await provider.listModels()
  assert.deepEqual(models.map((m) => [m.id, m.capabilities]), [
    ['chat-1', ['chat']],
    ['embed-1', ['embedding']],
  ])
})

test('provider errors carry the API message and flag rejected keys', async () => {
  const provider = createProvider(
    { kind: 'openai', apiKey: 'bad' },
    fakeFetch(() => Response.json({ error: { message: 'Incorrect API key provided' } }, { status: 401 })),
  )
  await assert.rejects(provider.listModels(), /API key rejected: Incorrect API key provided/)
})

test('Anthropic chat streams through the SDK with the injected fetch and refusal fallback', async () => {
  const calls: Call[] = []
  const events = [
    {
      type: 'message_start',
      message: {
        id: 'msg_1',
        type: 'message',
        role: 'assistant',
        model: 'claude-opus-5-5',
        content: [],
        stop_reason: null,
        stop_sequence: null,
        usage: { input_tokens: 5, output_tokens: 0 },
      },
    },
    { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
    { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'Hi ' } },
    { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'there' } },
    { type: 'content_block_stop', index: 0 },
    { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 2 } },
    { type: 'message_stop' },
  ]
  const body = events.map((e) => `event: ${e.type}\ndata: ${JSON.stringify(e)}\n\n`).join('')
  const provider = createProvider(
    { kind: 'anthropic', apiKey: 'sk-ant-test' },
    fakeFetch(() => new Response(body, { headers: { 'content-type': 'text/event-stream' } }), calls),
  )
  const deltas: string[] = []
  const text = await provider.chat({
    model: 'claude-opus-5-5',
    system: 'sys',
    messages: [{ role: 'user', content: 'hello' }],
    onText: (d) => deltas.push(d),
  })
  assert.equal(text, 'Hi there')
  assert.deepEqual(deltas, ['Hi ', 'there'])
  assert.match(calls[0].url, /^https:\/\/api\.anthropic\.com\/v1\/messages/)
  const sent = JSON.parse(calls[0].init.body as string)
  assert.equal(sent.fallbacks, 'default')
  assert.equal(sent.system, 'sys')
  assert.equal(sent.stream, true)
})

test('Anthropic provider refuses to embed with a helpful message', async () => {
  const provider = createProvider({ kind: 'anthropic', apiKey: 'k' }, fakeFetch(() => new Response('')))
  assert.equal(provider.supportsEmbeddings, false)
  await assert.rejects(provider.embed('x', ['y']), /no embeddings API/)
})

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { ApiError, createClient, defaultContextConfig, normalizeBaseUrl } from './client.ts'

type Call = { url: string; init: RequestInit }

function fakeFetch(status: number, body: unknown, calls: Call[] = []): typeof fetch {
  return (async (url: string, init: RequestInit) => {
    calls.push({ url, init })
    return new Response(body === undefined ? '' : JSON.stringify(body), { status })
  }) as unknown as typeof fetch
}

test('normalizeBaseUrl accepts bare hosts, trailing slashes and /api', () => {
  assert.equal(normalizeBaseUrl('host.ts.net:8502/'), 'https://host.ts.net:8502')
  assert.equal(normalizeBaseUrl(' https://host:8502/api/ '), 'https://host:8502')
  assert.equal(normalizeBaseUrl('http://10.0.0.2:5055'), 'http://10.0.0.2:5055')
})

test('requests go to /api with query params and a bearer password', async () => {
  const calls: Call[] = []
  const client = createClient({ baseUrl: 'https://h', password: 'pw', fetch: fakeFetch(200, [], calls) })
  await client.notes.list('notebook:abc')
  assert.equal(calls[0].url, 'https://h/api/notes?notebook_id=notebook%3Aabc')
  assert.equal((calls[0].init.headers as Record<string, string>).Authorization, 'Bearer pw')
})

test('new sources are linked to the notebook and processed in the background', async () => {
  const calls: Call[] = []
  const client = createClient({ baseUrl: 'https://h', fetch: fakeFetch(200, {}, calls) })
  await client.sources.create('notebook:1', { type: 'link', url: 'https://example.com' })
  assert.equal(calls[0].url, 'https://h/api/sources/json')
  assert.deepEqual(JSON.parse(calls[0].init.body as string), {
    type: 'link',
    url: 'https://example.com',
    notebooks: ['notebook:1'],
    embed: true,
    async_processing: true,
  })
})

test('API errors surface the FastAPI detail message', async () => {
  const client = createClient({ baseUrl: 'https://h', fetch: fakeFetch(404, { detail: 'Notebook not found' }) })
  await assert.rejects(client.notebooks.get('x'), (error: unknown) => {
    assert.ok(error instanceof ApiError)
    assert.equal(error.status, 404)
    assert.equal(error.message, 'Notebook not found')
    return true
  })
})

test('audio URLs are absolute so the player can stream them', async () => {
  const client = createClient({
    baseUrl: 'https://h:8502',
    fetch: fakeFetch(200, { url: '/api/podcasts/episodes/e1/audio?media_ticket=t' }),
  })
  assert.equal(await client.podcasts.audioUrl('e1'), 'https://h:8502/api/podcasts/episodes/e1/audio?media_ticket=t')
})

test('default context prefers insights and falls back to full text', () => {
  const config = defaultContextConfig(
    [
      { id: 's1', insights_count: 2 },
      { id: 's2', insights_count: 0 },
    ],
    [{ id: 'n1' }],
  )
  assert.deepEqual(config, {
    sources: { s1: 'insights', s2: 'full content' },
    notes: { n1: 'full content' },
  })
})

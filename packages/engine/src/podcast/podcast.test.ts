import assert from 'node:assert/strict'
import { test } from 'node:test'

import { LocalBackend } from '../local.ts'
import { fakeProvider, memoryDb } from '../testing.ts'
import { bytesToPcm, encodeWav, SAMPLE_RATE, Timeline, trimSilence } from './audio.ts'
import { narrationMatches, renderLive, splitNarration, type LiveSocket } from './live.ts'
import { produce } from './produce.ts'
import { CHECK_SYSTEM, RESEARCH_SYSTEM, REVISE_SYSTEM, findQuote, materialSections, parseCheck, type MaterialPart } from './check.ts'
import { parseScript } from './script.ts'
import { createVoiceProvider, type VoiceProvider } from './voices.ts'

/** A line voiced as 0.1 s of tone per word, so durations are predictable. */
function toneProvider(mode: 'line' | 'dialogue' = 'line'): VoiceProvider & { calls: string[] } {
  const tone = (words: number) => new Int16Array(Math.round(words * 0.1 * SAMPLE_RATE)).fill(1000)
  const calls: string[] = []
  return {
    kind: mode === 'line' ? 'openai' : 'gemini',
    mode,
    concurrency: 3,
    calls,
    async speakLine(text, voice) {
      calls.push(`${voice}:${text}`)
      await new Promise((r) => setTimeout(r, Math.random() * 5))
      return tone(text.split(' ').length)
    },
    async speakDialogue(lines) {
      calls.push(lines.map((l) => l.text).join('|'))
      return tone(lines.reduce((n, l) => n + l.text.split(' ').length, 0))
    },
    async listVoices() {
      return []
    },
    async listModels() {
      return []
    },
  }
}

const SCRIPT = {
  title: 'Dark patterns, explained',
  summary: 'Two hosts walk through the FTC report.',
  chapters: [
    { title: 'What they are', lines: [{ host: 0, text: 'one two three' }, { host: 1, text: 'four five' }] },
    { title: 'Why it matters', lines: [{ host: 0, text: 'six seven eight nine' }] },
  ],
}

test('WAV files have a correct header and samples', () => {
  const wav = encodeWav(new Int16Array([0, 1000, -1000]))
  assert.equal(new TextDecoder().decode(wav.slice(0, 4)), 'RIFF')
  const view = new DataView(wav.buffer)
  assert.equal(view.getUint32(24, true), SAMPLE_RATE)
  assert.equal(view.getUint32(40, true), 6)
  assert.deepEqual([...bytesToPcm(wav.slice(44))], [0, 1000, -1000])
})

test('trimSilence keeps the audible part with a little air', () => {
  const pcm = new Int16Array(SAMPLE_RATE).fill(0)
  pcm.fill(500, 10_000, 11_000)
  const trimmed = trimSilence(pcm, 0.01)
  assert.equal(trimmed.length, 1000 + 2 * 240)
})

test('the timeline reports where each piece starts', () => {
  const t = new Timeline()
  assert.deepEqual(t.append(new Int16Array(SAMPLE_RATE)), { start: 0, end: 1 })
  t.pause(0.5)
  assert.deepEqual(t.append(new Int16Array(SAMPLE_RATE / 2)), { start: 1.5, end: 2 })
})

test('parseScript accepts fenced JSON, clamps hosts and strips stage directions', () => {
  const raw = 'Sure!\n```json\n{"title":"T","summary":"S","chapters":[{"title":"A","lines":[{"host":5,"text":"Hi **there** [laughs]"},{"host":0,"text":""}]},{"title":"Empty","lines":[]}]}\n```'
  const script = parseScript(raw, 2)
  assert.deepEqual(script.chapters, [{ title: 'A', lines: [{ host: 1, text: 'Hi there' }] }])
  assert.throws(() => parseScript('no json here', 2), /expected structure/)
})

test('line-by-line production keeps script order and exact timing despite parallel requests', async () => {
  const provider = toneProvider('line')
  const progress: number[] = []
  const result = await produce({
    script: SCRIPT,
    hosts: ['Marin', 'Cedar'],
    voices: ['marin', 'cedar'],
    provider,
    onProgress: (done) => {
      progress.push(done)
    },
  })
  assert.deepEqual(progress, [1, 2, 3])
  assert.deepEqual(result.lines.map((l) => [l.speaker, l.text]), [
    ['Marin', 'one two three'],
    ['Cedar', 'four five'],
    ['Marin', 'six seven eight nine'],
  ])
  const [a, b, c] = result.lines
  assert.equal(a.start, 0)
  assert.ok(Math.abs(a.end - 0.3) < 1e-9)
  assert.ok(Math.abs(b.start - (0.3 + 0.32)) < 1e-9, 'a short pause between lines')
  assert.ok(Math.abs(c.start - (b.end + 0.9)) < 1e-9, 'a longer pause between chapters')
  assert.deepEqual(result.chapters.map((ch) => ch.title), ['What they are', 'Why it matters'])
  assert.equal(result.chapters[1].start, c.start)
  assert.ok(result.exactTiming)
  assert.ok(Math.abs(result.duration - result.pcm.length / SAMPLE_RATE) < 1e-9)
  assert.ok(provider.calls.includes('cedar:four five'), 'each host uses their own voice')
})

test('dialogue production voices a chapter at once and spreads line times by length', async () => {
  const result = await produce({ script: SCRIPT, hosts: ['Kore', 'Puck'], voices: ['Kore', 'Puck'], provider: toneProvider('dialogue') })
  assert.equal(result.exactTiming, false)
  const [a, b] = result.lines
  assert.equal(a.start, 0)
  assert.ok(Math.abs(a.end - b.start) < 1e-9)
  assert.ok(a.end - a.start > b.end - b.start, 'the longer line gets more time')
})

test('narrationMatches accepts faithful readings and rejects truncated or unrelated ones', () => {
  const script = 'The FTC groups dark patterns into four tactics that mislead people.'
  assert.ok(narrationMatches(script, 'The FTC groups dark patterns into four tactics that mislead people.'))
  assert.ok(narrationMatches(script, 'So, the FTC groups dark patterns into four tactics that mislead people'))
  assert.ok(!narrationMatches(script, 'The FTC groups dark patterns into four'))
  assert.ok(!narrationMatches(script, 'Hello, how can I help you today?'))
  assert.ok(!narrationMatches(script, ''))
})

test('splitNarration keeps sessions under the word cap', () => {
  const parts = splitNarration(Array.from({ length: 350 }, (_, i) => `w${i}`).join(' '), 160)
  assert.deepEqual(parts.map((p) => p.split(' ').length), [160, 160, 30])
})

/** Scripted Live server: answers session.start, speaks once commentary arrives, closes on request. */
function fakeLive(spokenText: string) {
  const sent: Array<{ type: string; [k: string]: unknown }> = []
  let headers: Record<string, string> = {}
  const audio = (value: number, samples: number) => {
    const bytes = new Uint8Array(samples * 2)
    const view = new DataView(bytes.buffer)
    for (let i = 0; i < samples; i++) view.setInt16(i * 2, value, true)
    return btoa(String.fromCharCode(...bytes))
  }
  const open = async (_url: string, h: Record<string, string>): Promise<LiveSocket> => {
    headers = h
    const queue: string[] = []
    const push = (e: object) => queue.push(JSON.stringify(e))
    return {
      send(message) {
        const event = JSON.parse(message)
        sent.push(event)
        if (event.type === 'session.start') push({ type: 'session.started', session: { id: 'sess_1' } })
        if (event.type === 'session.commentary.append') {
          push({ type: 'session.output_audio.delta', delta: audio(0, 2400) })
          push({ type: 'session.output_audio.delta', delta: audio(3000, 4800) })
          push({ type: 'session.output_transcript.delta', delta: spokenText })
          push({ type: 'session.output_audio.delta', delta: audio(0, 2400) })
        }
        if (event.type === 'session.close') push({ type: 'session.closed', usage: { seconds: 2 } })
      },
      async next(timeout) {
        if (queue.length) return queue.shift()!
        await new Promise((r) => setTimeout(r, Math.min(timeout, 5)))
        return queue.shift() ?? null
      },
      close() {},
    }
  }
  return { open, sent, headers: () => headers }
}

test('OpenAI Live narration follows the session protocol and returns trimmed speech', async () => {
  const text = 'Subscription traps add friction only on the way out.'
  const live = fakeLive(text)
  const pcm = await renderLive({ text, voice: 'marin', apiKey: 'sk-test', model: 'gpt-live-1', openSocket: live.open, quietMs: 20, silenceIntervalMs: 5 })
  assert.equal(live.headers().Authorization, 'Bearer sk-test')
  const start = live.sent[0] as unknown as { session: { model: string; audio: { output: { voice: string } } } }
  assert.equal(live.sent[0].type, 'session.start')
  assert.equal(start.session.model, 'gpt-live-1')
  assert.equal(start.session.audio.output.voice, 'marin')
  assert.ok(live.sent.some((e) => e.type === 'session.input_audio.append'), 'silence is streamed while it speaks')
  assert.equal(live.sent.at(-1)?.type, 'session.close')
  assert.ok(pcm.length >= 4800 && pcm.length < 4800 + 2400 + 6000 + 1, 'leading silence is trimmed')
})

test('OpenAI Live narration that drifts from the script fails instead of being kept', async () => {
  const live = fakeLive('Sure, happy to help with that.')
  await assert.rejects(
    renderLive({ text: 'Read this exact line please.', voice: 'cedar', apiKey: 'k', model: 'gpt-live-1', openSocket: live.open, quietMs: 20, silenceIntervalMs: 5 }),
    /did not read the whole line/,
  )
})

type Call = { url: string; init: RequestInit }
const fetchReturning = (body: BodyInit | object, calls: Call[], headers: Record<string, string> = {}) =>
  (async (url: string, init: RequestInit) => {
    calls.push({ url, init })
    return body instanceof Uint8Array || typeof body === 'string'
      ? new Response(body as BodyInit, { headers })
      : Response.json(body)
  }) as unknown as typeof fetch

test('voice providers ask for 24 kHz PCM in their own request formats', async () => {
  const pcmBytes = new Uint8Array([0x10, 0x27, 0xf0, 0xd8]) // 10000, -10000

  const openaiCalls: Call[] = []
  const openai = createVoiceProvider({ kind: 'openai', model: 'gpt-4o-mini-tts', apiKey: 'k', voices: ['marin'] }, fetchReturning(pcmBytes, openaiCalls))
  assert.deepEqual([...(await openai.speakLine('Hello', 'marin'))].length > 0, true)
  const openaiBody = JSON.parse(openaiCalls[0].init.body as string)
  assert.equal(openaiCalls[0].url, 'https://api.openai.com/v1/audio/speech')
  assert.equal(openaiBody.response_format, 'pcm')
  assert.equal(openaiBody.voice, 'marin')

  const elevenCalls: Call[] = []
  const eleven = createVoiceProvider({ kind: 'elevenlabs', model: 'eleven_multilingual_v2', apiKey: 'x', voices: ['v1'] }, fetchReturning(pcmBytes, elevenCalls))
  await eleven.speakLine('Hello', 'voice123')
  assert.match(elevenCalls[0].url, /text-to-speech\/voice123\?output_format=pcm_24000$/)
  assert.equal((elevenCalls[0].init.headers as Record<string, string>)['xi-api-key'], 'x')

  const geminiCalls: Call[] = []
  const audio = btoa(String.fromCharCode(...pcmBytes))
  const gemini = createVoiceProvider(
    { kind: 'gemini', model: 'gemini-2.5-flash-preview-tts', apiKey: 'g', voices: ['Kore', 'Puck'] },
    fetchReturning({ candidates: [{ content: { parts: [{ inlineData: { mimeType: 'audio/L16;codec=pcm;rate=24000', data: audio } }] } }] }, geminiCalls),
  )
  await gemini.speakDialogue(
    [
      { host: 0, speaker: 'Kore', text: 'First.' },
      { host: 1, speaker: 'Puck', text: 'Second.' },
    ],
    ['Kore', 'Puck'],
  )
  const geminiBody = JSON.parse(geminiCalls[0].init.body as string)
  assert.deepEqual(geminiBody.generationConfig.responseModalities, ['AUDIO'])
  const speakers = geminiBody.generationConfig.speechConfig.multiSpeakerVoiceConfig.speakerVoiceConfigs
  assert.deepEqual(speakers.map((s: { speaker: string; voiceConfig: { prebuiltVoiceConfig: { voiceName: string } } }) => [s.speaker, s.voiceConfig.prebuiltVoiceConfig.voiceName]), [
    ['Kore', 'Kore'],
    ['Puck', 'Puck'],
  ])
  assert.match(geminiBody.contents[0].parts[0].text, /Kore: First\.\nPuck: Second\./)
})

async function podcastBackend(voice: VoiceProvider, reply: string) {
  const chat = fakeProvider()
  chat.reply = reply
  const saved = new Map<string, Uint8Array>()
  const backend = await LocalBackend.open({
    db: memoryDb(),
    fetch,
    extractUrl: async () => ({ title: '', text: '' }),
    getAiConfig: async () => ({ chat: { provider: { kind: 'openai' }, model: 'fake-chat' }, voiceKeys: { openai: 'sk' } }),
    createProvider: () => chat,
    createVoiceProvider: () => voice,
    audio: {
      async save(id, wav) {
        saved.set(id, wav)
        return `mem:${id}`
      },
      async url(ref) {
        return `blob:${ref}`
      },
      async remove(ref) {
        saved.delete(ref.slice(4))
      },
    },
  })
  const speakers = await backend.saveSpeakerProfile({
    name: 'Duo',
    description: '',
    voiceKind: 'openai',
    voiceModel: 'gpt-4o-mini-tts',
    speakers: [
      { name: 'Marin', voice: 'marin', backstory: 'Former FTC staffer', personality: 'Precise' },
      { name: 'Cedar', voice: 'cedar', backstory: 'Product designer', personality: 'Skeptical' },
    ],
  })
  const episodeProfile = await backend.saveEpisodeProfile({
    name: 'Explainer',
    description: '',
    speakerProfileId: speakers.id,
    language: 'en',
    briefing: 'Explain it plainly.',
    segments: 2,
  })
  return { backend, chat, saved, speakers, episodeProfile }
}

test('the server’s podcast profiles are seeded once', async () => {
  const backend = await LocalBackend.open({ db: memoryDb(), fetch, extractUrl: async () => ({ title: '', text: '' }), getAiConfig: async () => ({}) })
  const { episode, speaker } = await backend.podcastProfiles()
  assert.equal(episode.length, 3)
  assert.equal(speaker.length, 3)
  for (const e of episode) assert.ok(speaker.some((s) => s.id === e.speakerProfileId), `${e.name} points at a seeded speaker profile`)
  await assert.rejects(backend.deleteSpeakerProfile(episode[0].speakerProfileId), /uses these speakers/)
})

test('an on-device episode is written from the profiles, voiced and saved with chapters and a timed transcript', async () => {
  const voice = toneProvider('line')
  const { backend, chat, saved, episodeProfile } = await podcastBackend(voice, JSON.stringify(SCRIPT))
  const nb = await backend.createNotebook('Research')
  await backend.addSource(nb.id, { type: 'text', content: 'The FTC report covers four tactics.', title: 'FTC report' })
  await backend.idle()

  const created = await backend.createEpisode({ notebookId: nb.id, episodeProfile: episodeProfile.id, focus: 'Subscription traps' })
  assert.equal(created.status, 'writing')
  await backend.idle()

  const episode = await backend.getEpisode(created.id)
  assert.equal(episode.status, 'completed', episode.error)
  assert.equal(episode.name, 'Dark patterns, explained')
  assert.deepEqual(episode.chapters.map((c) => c.title), ['What they are', 'Why it matters'])
  assert.deepEqual(episode.transcript.map((l) => l.speaker), ['Marin', 'Cedar', 'Marin'])
  assert.ok(episode.exactTiming)
  assert.ok(episode.duration! > 1)
  const prompt = chat.chats[0].messages[0].content
  assert.match(prompt, /Subscription traps/)
  assert.match(prompt, /Background: Former FTC staffer/)
  assert.match(prompt, /Explain it plainly\./)
  assert.match(prompt, /in 2 chapters/)
  assert.match(prompt, /The FTC report covers four tactics/)
  assert.ok(voice.calls.includes('cedar:four five'), 'each speaker uses their own voice')
  assert.equal(await backend.episodeAudioUrl(created.id), `blob:mem:${created.id}`)
  const kept = await backend.episodePrompt(created.id)
  assert.equal(kept?.prompt, prompt, 'the exact prompt is saved with the episode')
  assert.equal(kept?.system, chat.chats[0].system)

  await backend.deleteEpisode(created.id)
  assert.equal(await backend.episodePrompt(created.id), null, 'the prompt goes with the episode')
  assert.equal(saved.size, 0)
  assert.equal((await backend.listEpisodes(nb.id)).length, 0)
})

test('a failed episode records why and can be retried with the same choices', async () => {
  const voice = toneProvider('line')
  const speak = voice.speakLine
  voice.speakLine = async () => {
    throw new Error('Voice quota exceeded')
  }
  const { backend, episodeProfile } = await podcastBackend(voice, JSON.stringify(SCRIPT))
  const nb = await backend.createNotebook('N')
  await backend.createNote(nb.id, { content: 'Something to talk about' })
  const e = await backend.createEpisode({ notebookId: nb.id, episodeProfile: episodeProfile.id })
  await backend.idle()
  const failed = await backend.getEpisode(e.id)
  assert.equal(failed.status, 'failed')
  assert.match(failed.error!, /Voice quota exceeded/)

  voice.speakLine = speak
  await backend.retryEpisode(e.id)
  await backend.idle()
  assert.equal((await backend.getEpisode(e.id)).status, 'completed')
})

test('three speakers are voiced line by line even with a two-voice dialogue provider', async () => {
  const provider = toneProvider('dialogue')
  const script = {
    title: 'Panel',
    summary: '',
    chapters: [{ title: 'One', lines: [{ host: 0, text: 'a b' }, { host: 1, text: 'c d' }, { host: 2, text: 'e f' }] }],
  }
  const result = await produce({ script, hosts: ['A', 'B', 'C'], voices: ['v1', 'v2', 'v3'], provider })
  assert.ok(result.exactTiming)
  assert.deepEqual(provider.calls, ['v1:a b', 'v2:c d', 'v3:e f'])
})

const MATERIAL: MaterialPart[] = [
  { title: 'FTC report', text: 'Staff found that 76% of subscription sites used at least one dark pattern.\n## Methods\nThe report does not measure how much money users lost.' },
  { title: 'Design study', excerpt: true, text: 'In a lab study of 120 people, hidden cancel buttons roughly doubled the time to cancel —\nbut the effect on real-world churn was not tested.' },
  { title: 'My thoughts', note: true, text: 'Ask about enforcement.' },
]

test('the claim check verifies every quote against the material itself', () => {
  const raw = JSON.stringify({
    claims: [
      { line: 1, claim: '76% of sites used a dark pattern', verdict: 'supported', quote: 'staff found that 76% of subscription sites used at least one dark pattern' },
      // Punctuation and a line break differ from the source, the words do not.
      { line: 2, claim: 'Hidden buttons double cancel time', verdict: 'supported', quote: 'hidden cancel buttons roughly doubled the time to cancel, but the effect' },
      { line: 2, claim: 'Hidden buttons raise churn', verdict: 'overstated', quote: 'the effect on real-world churn was not tested', note: 'Churn was not measured.' },
      // The model says supported, but the quote is made up.
      { line: 3, claim: 'Users lost $2 billion', verdict: 'supported', quote: 'users lost an estimated two billion dollars to dark patterns' },
      { line: 3, claim: 'Regulators will act soon', verdict: 'unsupported', quote: '' },
    ],
  })
  const check = parseCheck(`\`\`\`json\n${raw}\n\`\`\``, SCRIPT, MATERIAL)
  assert.deepEqual(
    check.claims.map((c) => c.verdict),
    ['supported', 'supported', 'overstated', 'unsupported', 'unsupported'],
  )
  assert.equal(check.claims[0].source, 'FTC report')
  assert.equal(check.claims[1].source, 'Design study')
  assert.equal(check.claims[3].quoteFound, false)
  assert.match(check.claims[3].note!, /not actually there/)
  assert.equal(check.claims[2].said, 'four five')
  assert.deepEqual([check.supported, check.overstated, check.unsupported], [2, 1, 2])
  assert.deepEqual([check.sourcesUsed, check.sourcesTotal], [2, 2], 'notes are not counted as sources')
})

test('findQuote tolerates a few PDF slips in a long quote but not a different sentence', () => {
  const sections = materialSections(MATERIAL)
  assert.equal(findQuote('In a lab study of 120 people, hidden cancel butons roughly doubled the time to cancel', sections)?.title, 'Design study')
  assert.equal(findQuote('In a field study of 9000 people, hidden cancel buttons tripled churn', sections), undefined)
  assert.equal(findQuote('76%', sections), undefined, 'too short to count as evidence')
})

test('a new episode is checked against the material it was written from', async () => {
  const { backend, chat, episodeProfile } = await podcastBackend(toneProvider('line'), '')
  chat.reply = (request) =>
    request.system === CHECK_SYSTEM
      ? JSON.stringify({ claims: [{ line: 1, claim: 'Four tactics', verdict: 'supported', quote: 'The FTC report covers four tactics' }] })
      : JSON.stringify(SCRIPT)
  const nb = await backend.createNotebook('N')
  await backend.addSource(nb.id, { type: 'text', content: 'The FTC report covers four tactics.', title: 'FTC report' })
  await backend.idle()
  const created = await backend.createEpisode({ notebookId: nb.id, episodeProfile: episodeProfile.id })
  await backend.idle()

  const { check } = await backend.getEpisode(created.id)
  assert.equal(check?.status, 'done', check?.error)
  assert.equal(check?.writer, 'fake-chat')
  assert.deepEqual([check?.supported, check?.sourcesUsed, check?.sourcesTotal], [1, 1, 1])
  assert.equal((await backend.listEpisodes(nb.id))[0].check?.supported, 1)

  // A broken check is reported on the episode, which stays playable.
  chat.reply = (request) => (request.system === CHECK_SYSTEM ? 'no json here' : JSON.stringify(SCRIPT))
  await backend.checkEpisode(created.id)
  await backend.idle()
  const again = await backend.getEpisode(created.id)
  assert.equal(again.status, 'completed')
  assert.equal(again.check?.status, 'failed')
  assert.equal(chat.chats.at(-1)?.messages[0].content.includes('The FTC report covers four tactics'), true, 'rechecks use the saved material')
})

test('a careful episode fixes flagged lines and checks again before voicing', async () => {
  const { backend, chat, episodeProfile } = await podcastBackend(toneProvider('line'), '')
  const fixed = { ...SCRIPT, chapters: [{ title: 'What they are', lines: [{ host: 0, text: 'the report covers four tactics' }] }] }
  let checks = 0
  chat.reply = (request) => {
    if (request.system === REVISE_SYSTEM) return JSON.stringify(fixed)
    if (request.system !== CHECK_SYSTEM) return JSON.stringify(SCRIPT)
    checks++
    return JSON.stringify({
      claims:
        checks === 1
          ? [{ line: 1, claim: 'Ten tactics', verdict: 'overstated', quote: 'The FTC report covers four tactics', note: 'The report lists four.' }]
          : [{ line: 1, claim: 'Four tactics', verdict: 'supported', quote: 'The FTC report covers four tactics' }],
    })
  }
  const nb = await backend.createNotebook('N')
  await backend.addSource(nb.id, { type: 'text', content: 'The FTC report covers four tactics.', title: 'FTC report' })
  await backend.idle()
  const created = await backend.createEpisode({ notebookId: nb.id, episodeProfile: episodeProfile.id, effort: 'careful' })
  await backend.idle()

  const episode = await backend.getEpisode(created.id)
  assert.equal(episode.status, 'completed', episode.error)
  assert.equal(episode.effort, 'careful')
  assert.deepEqual(episode.transcript.map((l) => l.text), ['the report covers four tactics'], 'the revised script is the one voiced')
  assert.equal(episode.check?.supported, 1)
  assert.deepEqual(episode.check?.history, [{ supported: 0, overstated: 1, unsupported: 0 }])
  const revise = chat.chats.find((c) => c.system === REVISE_SYSTEM)!
  assert.match(revise.messages[0].content, /The report lists four\./)
})

test('a thorough episode writes from study notes; a quick one is not checked', async () => {
  const { backend, chat, episodeProfile } = await podcastBackend(toneProvider('line'), '')
  chat.reply = (request) =>
    request.system === RESEARCH_SYSTEM
      ? 'FTC report: four tactics. Caveat: no money figures.'
      : request.system === CHECK_SYSTEM
        ? JSON.stringify({ claims: [] })
        : JSON.stringify(SCRIPT)
  const nb = await backend.createNotebook('N')
  await backend.createNote(nb.id, { content: 'Something to talk about' })
  const thorough = await backend.createEpisode({ notebookId: nb.id, episodeProfile: episodeProfile.id, effort: 'thorough' })
  await backend.idle()
  assert.match((await backend.episodePrompt(thorough.id))!.prompt, /# Research notes[\s\S]*Caveat: no money figures/)
  assert.equal((await backend.getEpisode(thorough.id)).check?.status, 'done')

  const before = chat.chats.length
  const quick = await backend.createEpisode({ notebookId: nb.id, episodeProfile: episodeProfile.id, effort: 'quick' })
  await backend.idle()
  assert.equal(chat.chats.length - before, 1, 'only the script is written')
  assert.equal((await backend.getEpisode(quick.id)).check, undefined)
})

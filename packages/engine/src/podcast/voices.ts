import { providerError } from '../providers/http.ts'
import { ProviderError } from '../providers/types.ts'
import { base64ToBytes, bytesToPcm, resample, SAMPLE_RATE, trimSilence } from './audio.ts'
import { renderLive, type OpenSocket } from './live.ts'

export type VoiceKind = 'openai' | 'openai-live' | 'gemini' | 'elevenlabs'

export interface VoiceConfig {
  kind: VoiceKind
  model: string
  apiKey?: string
  /** One voice per host, in host order. */
  voices: string[]
  /** Spoken names for the hosts, usually the voices' display names. */
  hosts?: string[]
}

export interface VoiceOption {
  id: string
  label: string
}

export interface DialogueLine {
  host: number
  speaker: string
  text: string
}

export interface VoiceProvider {
  readonly kind: VoiceKind
  /**
   * 'line' voices one line per request, which gives exact timing.
   * 'dialogue' voices a whole chapter with both hosts at once, which sounds
   * more natural; line timing inside it is estimated.
   */
  readonly mode: 'line' | 'dialogue'
  /** Requests that may run at the same time. */
  readonly concurrency: number
  speakLine(text: string, voice: string, signal?: AbortSignal): Promise<Int16Array>
  speakDialogue(lines: DialogueLine[], voices: string[], signal?: AbortSignal): Promise<Int16Array>
  listVoices(): Promise<VoiceOption[]>
  listModels(): Promise<string[]>
}

export const VOICE_LABELS: Record<VoiceKind, string> = {
  openai: 'OpenAI speech',
  'openai-live': 'OpenAI Live',
  gemini: 'Gemini',
  elevenlabs: 'ElevenLabs',
}

export const DEFAULT_VOICE_MODELS: Record<VoiceKind, string> = {
  openai: 'gpt-4o-mini-tts',
  'openai-live': 'gpt-live-1',
  gemini: 'gemini-2.5-flash-preview-tts',
  elevenlabs: 'eleven_multilingual_v2',
}

const OPENAI_VOICES = ['alloy', 'ash', 'ballad', 'cedar', 'coral', 'echo', 'fable', 'marin', 'nova', 'onyx', 'sage', 'shimmer', 'verse']
export const LIVE_VOICES = ['alloy', 'ash', 'ballad', 'coral', 'echo', 'sage', 'shimmer', 'verse', 'marin', 'cedar']
const GEMINI_VOICES: Array<[string, string]> = [
  ['Zephyr', 'bright'], ['Puck', 'upbeat'], ['Charon', 'informative'], ['Kore', 'firm'], ['Fenrir', 'excitable'],
  ['Leda', 'youthful'], ['Orus', 'firm'], ['Aoede', 'breezy'], ['Callirrhoe', 'easy-going'], ['Autonoe', 'bright'],
  ['Enceladus', 'breathy'], ['Iapetus', 'clear'], ['Umbriel', 'easy-going'], ['Algieba', 'smooth'], ['Despina', 'smooth'],
  ['Erinome', 'clear'], ['Algenib', 'gravelly'], ['Rasalgethi', 'informative'], ['Laomedeia', 'upbeat'], ['Achernar', 'soft'],
  ['Alnilam', 'firm'], ['Schedar', 'even'], ['Gacrux', 'mature'], ['Pulcherrima', 'forward'], ['Achird', 'friendly'],
  ['Zubenelgenubi', 'casual'], ['Vindemiatrix', 'gentle'], ['Sadachbia', 'lively'], ['Sadaltager', 'knowledgeable'], ['Sulafat', 'warm'],
]

/** Two contrasting voices per provider, so a new show sounds like two people. */
export const DEFAULT_VOICES: Record<VoiceKind, string[]> = {
  openai: ['marin', 'cedar'],
  'openai-live': ['marin', 'cedar'],
  gemini: ['Kore', 'Puck'],
  elevenlabs: [],
}

const HOST_STYLE =
  'Speak as a warm, curious podcast host in a natural conversational rhythm. Vary pace slightly, sound engaged, never theatrical.'

export function createVoiceProvider(config: VoiceConfig, doFetch: typeof fetch, openSocket?: OpenSocket): VoiceProvider {
  switch (config.kind) {
    case 'openai':
      return openaiSpeech(config, doFetch)
    case 'openai-live':
      return openaiLive(config, doFetch, openSocket)
    case 'gemini':
      return geminiSpeech(config, doFetch)
    case 'elevenlabs':
      return elevenLabs(config, doFetch)
  }
}

const unsupported = (kind: VoiceKind, what: string) => () => {
  throw new ProviderError('openai', 0, `${VOICE_LABELS[kind]} does not support ${what}`)
}

async function pcmResponse(kind: VoiceKind, response: Response): Promise<Int16Array> {
  if (!response.ok) {
    const err = await providerError(kind === 'gemini' ? 'gemini' : 'openai', response)
    throw new ProviderError(err.provider, err.status, `${VOICE_LABELS[kind]}: ${err.message}`)
  }
  return bytesToPcm(new Uint8Array(await response.arrayBuffer()))
}

function openaiSpeech(config: VoiceConfig, doFetch: typeof fetch): VoiceProvider {
  const headers = { 'Content-Type': 'application/json', Authorization: `Bearer ${config.apiKey ?? ''}` }
  return {
    kind: 'openai',
    mode: 'line',
    concurrency: 3,
    async speakLine(text, voice, signal) {
      const response = await doFetch('https://api.openai.com/v1/audio/speech', {
        method: 'POST',
        headers,
        signal,
        body: JSON.stringify({ model: config.model, voice, input: text, response_format: 'pcm', instructions: HOST_STYLE }),
      })
      return trimSilence(await pcmResponse('openai', response))
    },
    speakDialogue: unsupported('openai', 'dialogue requests'),
    async listVoices() {
      return OPENAI_VOICES.map((id) => ({ id, label: id[0].toUpperCase() + id.slice(1) }))
    },
    async listModels() {
      const response = await doFetch('https://api.openai.com/v1/models', { headers })
      if (!response.ok) throw await providerError('openai', response)
      const body = (await response.json()) as { data: Array<{ id: string }> }
      return body.data.map((m) => m.id).filter((id) => /tts/i.test(id)).sort()
    },
  }
}

function openaiLive(config: VoiceConfig, doFetch: typeof fetch, openSocket?: OpenSocket): VoiceProvider {
  return {
    kind: 'openai-live',
    mode: 'line',
    // Live renders in real time; sessions run one after another, like the server does.
    concurrency: 1,
    async speakLine(text, voice, signal) {
      if (!openSocket) throw new ProviderError('openai', 0, 'OpenAI Live needs the app’s WebSocket support')
      if (!config.apiKey) throw new ProviderError('openai', 401, 'OpenAI Live needs your OpenAI API key')
      return renderLive({ text, voice, apiKey: config.apiKey, model: config.model, openSocket, signal })
    },
    speakDialogue: unsupported('openai-live', 'dialogue requests'),
    async listVoices() {
      return LIVE_VOICES.map((id) => ({ id, label: id[0].toUpperCase() + id.slice(1) }))
    },
    async listModels() {
      const response = await doFetch('https://api.openai.com/v1/models', {
        headers: { Authorization: `Bearer ${config.apiKey ?? ''}` },
      })
      if (!response.ok) throw await providerError('openai', response)
      const body = (await response.json()) as { data: Array<{ id: string }> }
      const live = body.data.map((m) => m.id).filter((id) => /^gpt-live/i.test(id))
      return live.length ? live.sort() : [DEFAULT_VOICE_MODELS['openai-live']]
    },
  }
}

const GEMINI_BASE = 'https://generativelanguage.googleapis.com/v1beta'

function geminiSpeech(config: VoiceConfig, doFetch: typeof fetch): VoiceProvider {
  const headers = { 'Content-Type': 'application/json', 'x-goog-api-key': config.apiKey ?? '' }
  const model = config.model.startsWith('models/') ? config.model : `models/${config.model}`

  async function generate(prompt: string, speechConfig: object, signal?: AbortSignal): Promise<Int16Array> {
    const response = await doFetch(`${GEMINI_BASE}/${model}:generateContent`, {
      method: 'POST',
      headers,
      signal,
      body: JSON.stringify({
        contents: [{ role: 'user', parts: [{ text: prompt }] }],
        generationConfig: { responseModalities: ['AUDIO'], speechConfig },
      }),
    })
    if (!response.ok) {
      const err = await providerError('gemini', response)
      throw new ProviderError('gemini', err.status, `Gemini: ${err.message}`)
    }
    const body = (await response.json()) as {
      candidates?: Array<{ content?: { parts?: Array<{ inlineData?: { mimeType?: string; data?: string } }> } }>
    }
    const part = body.candidates?.[0]?.content?.parts?.find((p) => p.inlineData?.data)?.inlineData
    if (!part?.data) throw new ProviderError('gemini', 0, 'Gemini returned no audio')
    const rate = Number(/rate=(\d+)/.exec(part.mimeType ?? '')?.[1] ?? SAMPLE_RATE)
    return resample(bytesToPcm(base64ToBytes(part.data)), rate)
  }

  const voiceConfig = (name: string) => ({ prebuiltVoiceConfig: { voiceName: name } })

  return {
    kind: 'gemini',
    mode: 'dialogue',
    concurrency: 2,
    async speakLine(text, voice, signal) {
      return trimSilence(await generate(`Read this aloud, warmly and naturally:\n${text}`, { voiceConfig: voiceConfig(voice) }, signal))
    },
    async speakDialogue(lines, voices, signal) {
      const speakers = [...new Map(lines.map((l) => [l.speaker, l.host])).entries()]
      if (speakers.length < 2) return this.speakLine(lines.map((l) => l.text).join('\n'), voices[lines[0]?.host ?? 0], signal)
      const script = lines.map((l) => `${l.speaker}: ${l.text}`).join('\n')
      const names = speakers.map(([s]) => s).join(' and ')
      return trimSilence(
        await generate(
          `Perform this podcast conversation between ${names}. Natural, relaxed and engaged, with real turn-taking:\n${script}`,
          {
            multiSpeakerVoiceConfig: {
              speakerVoiceConfigs: speakers.slice(0, 2).map(([speaker, host]) => ({ speaker, voiceConfig: voiceConfig(voices[host]) })),
            },
          },
          signal,
        ),
      )
    },
    async listVoices() {
      return GEMINI_VOICES.map(([id, tone]) => ({ id, label: `${id}, ${tone}` }))
    },
    async listModels() {
      const response = await doFetch(`${GEMINI_BASE}/models?pageSize=1000`, { headers })
      if (!response.ok) throw await providerError('gemini', response)
      const body = (await response.json()) as { models?: Array<{ name: string }> }
      return (body.models ?? []).map((m) => m.name.replace(/^models\//, '')).filter((id) => /tts/i.test(id)).sort()
    },
  }
}

function elevenLabs(config: VoiceConfig, doFetch: typeof fetch): VoiceProvider {
  const headers = { 'Content-Type': 'application/json', 'xi-api-key': config.apiKey ?? '' }
  return {
    kind: 'elevenlabs',
    mode: 'line',
    concurrency: 2,
    async speakLine(text, voice, signal) {
      const response = await doFetch(
        `https://api.elevenlabs.io/v1/text-to-speech/${encodeURIComponent(voice)}?output_format=pcm_24000`,
        { method: 'POST', headers, signal, body: JSON.stringify({ text, model_id: config.model }) },
      )
      return trimSilence(await pcmResponse('elevenlabs', response))
    },
    speakDialogue: unsupported('elevenlabs', 'dialogue requests'),
    async listVoices() {
      const response = await doFetch('https://api.elevenlabs.io/v1/voices', { headers })
      if (!response.ok) throw await providerError('openai', response)
      const body = (await response.json()) as { voices: Array<{ voice_id: string; name: string }> }
      return body.voices.map((v) => ({ id: v.voice_id, label: v.name }))
    },
    async listModels() {
      const response = await doFetch('https://api.elevenlabs.io/v1/models', { headers })
      if (!response.ok) throw await providerError('openai', response)
      const body = (await response.json()) as Array<{ model_id: string; can_do_text_to_speech?: boolean }>
      return body.filter((m) => m.can_do_text_to_speech !== false).map((m) => m.model_id)
    },
  }
}

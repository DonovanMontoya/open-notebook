// Speech to text for audio and video sources, with the user's own key.
import { providerError } from './providers/http.ts'
import { DEFAULT_BASE_URLS, ProviderError, type ProviderConfig } from './providers/types.ts'

export const MEDIA_TYPES = /^(audio|video)\//
export const isMedia = (type: string, name = '') => MEDIA_TYPES.test(type) || /\.(mp3|m4a|wav|aac|ogg|flac|webm|mp4|mov|mkv|mpeg|mpga)$/i.test(name)

const OPENAI_LIMIT = 25 * 1024 * 1024
const GEMINI_INLINE_LIMIT = 19 * 1024 * 1024

export const DEFAULT_TRANSCRIPTION_MODELS: Partial<Record<ProviderConfig['kind'], string>> = {
  openai: 'gpt-4o-transcribe',
  gemini: 'gemini-2.5-flash',
  'openai-compatible': 'whisper-1',
}

const mb = (bytes: number) => `${Math.round(bytes / 1024 / 1024)} MB`

export async function transcribe(
  config: ProviderConfig,
  model: string,
  file: { data: ArrayBuffer; name: string; type: string },
  doFetch: typeof fetch,
  signal?: AbortSignal,
): Promise<string> {
  if (config.kind === 'anthropic') {
    throw new ProviderError('anthropic', 0, 'Anthropic cannot transcribe audio. Choose OpenAI or Gemini for speech to text.')
  }
  if (config.kind === 'gemini') {
    if (file.data.byteLength > GEMINI_INLINE_LIMIT) {
      throw new ProviderError('gemini', 0, `This recording is ${mb(file.data.byteLength)}; Gemini accepts up to 19 MB here.`)
    }
    const bytes = new Uint8Array(file.data)
    let binary = ''
    for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    const base = (config.baseUrl || DEFAULT_BASE_URLS.gemini!).replace(/\/+$/, '')
    const response = await doFetch(`${base}/models/${model}:generateContent`, {
      method: 'POST',
      signal,
      headers: { 'Content-Type': 'application/json', 'x-goog-api-key': config.apiKey ?? '' },
      body: JSON.stringify({
        contents: [
          {
            role: 'user',
            parts: [
              { inlineData: { mimeType: file.type || 'audio/mpeg', data: btoa(binary) } },
              { text: 'Transcribe this recording word for word. Start a new paragraph when the speaker or topic changes. Output only the transcript.' },
            ],
          },
        ],
      }),
    })
    if (!response.ok) throw await providerError('gemini', response)
    const body = (await response.json()) as { candidates?: Array<{ content?: { parts?: Array<{ text?: string }> } }> }
    return (body.candidates?.[0]?.content?.parts ?? []).map((p) => p.text ?? '').join('').trim()
  }

  if (file.data.byteLength > OPENAI_LIMIT) {
    throw new ProviderError(config.kind, 0, `This recording is ${mb(file.data.byteLength)}; OpenAI transcription accepts up to 25 MB.`)
  }
  const base = (config.baseUrl || DEFAULT_BASE_URLS[config.kind] || DEFAULT_BASE_URLS.openai!).replace(/\/+$/, '')
  const form = new FormData()
  form.append('file', new Blob([file.data], { type: file.type }), file.name)
  form.append('model', model)
  form.append('response_format', 'json')
  const response = await doFetch(`${base}/audio/transcriptions`, {
    method: 'POST',
    signal,
    headers: config.apiKey ? { Authorization: `Bearer ${config.apiKey}` } : {},
    body: form,
  })
  if (!response.ok) throw await providerError(config.kind, response)
  const body = (await response.json()) as { text?: string }
  return (body.text ?? '').trim()
}

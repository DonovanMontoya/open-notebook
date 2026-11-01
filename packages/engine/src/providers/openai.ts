import { providerError, readSse } from './http.ts'
import {
  DEFAULT_BASE_URLS,
  ProviderError,
  type ChatRequest,
  type ModelInfo,
  type Provider,
  type ProviderConfig,
} from './types.ts'

const EMBED_BATCH = 64

/** Models that only make sense for chat in a notebook UI are kept; audio, image and moderation models are dropped. */
const NON_CHAT = /(whisper|tts|dall-e|image|moderation|audio|realtime|transcribe|search|embedding|embed)/i

/**
 * OpenAI and any server that speaks the OpenAI chat-completions protocol:
 * OpenRouter, Groq, Together, LM Studio, Ollama (/v1), vLLM and so on.
 */
export function createOpenAIProvider(config: ProviderConfig, doFetch: typeof fetch): Provider {
  const kind = config.kind
  const baseUrl = (config.baseUrl || DEFAULT_BASE_URLS[kind] || DEFAULT_BASE_URLS.openai!).replace(/\/+$/, '')

  function headers(): Record<string, string> {
    const h: Record<string, string> = { 'Content-Type': 'application/json' }
    if (config.apiKey) h.Authorization = `Bearer ${config.apiKey}`
    if (baseUrl.includes('openrouter.ai')) {
      h['HTTP-Referer'] = 'https://github.com/DonovanMontoya/open-notebook'
      h['X-Title'] = 'Open Notebook'
    }
    return h
  }

  return {
    kind,
    supportsEmbeddings: true,

    async listModels(): Promise<ModelInfo[]> {
      const response = await doFetch(`${baseUrl}/models`, { headers: headers() })
      if (!response.ok) throw await providerError(kind, response)
      const body = (await response.json()) as { data?: Array<{ id: string; name?: string }> }
      return (body.data ?? [])
        .map((m): ModelInfo => {
          const isEmbedding = /embed/i.test(m.id)
          return {
            id: m.id,
            label: m.name && m.name !== m.id ? m.name : undefined,
            capabilities: isEmbedding ? ['embedding'] : NON_CHAT.test(m.id) ? [] : ['chat'],
          }
        })
        .sort((a, b) => a.id.localeCompare(b.id))
    },

    async chat(request: ChatRequest): Promise<string> {
      const response = await doFetch(`${baseUrl}/chat/completions`, {
        method: 'POST',
        headers: headers(),
        signal: request.signal,
        body: JSON.stringify({
          model: request.model,
          stream: true,
          messages: [{ role: 'system', content: request.system }, ...request.messages],
        }),
      })
      if (!response.ok) throw await providerError(kind, response)
      if (!response.body) throw new ProviderError(kind, 0, 'The provider returned an empty response')

      let text = ''
      for await (const data of readSse(response.body, request.signal)) {
        if (data === '[DONE]') break
        let event: { choices?: Array<{ delta?: { content?: string | null } }>; error?: { message?: string } }
        try {
          event = JSON.parse(data)
        } catch {
          continue
        }
        if (event.error) throw new ProviderError(kind, 0, event.error.message ?? 'Stream error')
        const delta = event.choices?.[0]?.delta?.content
        if (delta) {
          text += delta
          request.onText?.(delta)
        }
      }
      return text
    },

    async embed(model: string, texts: string[], signal?: AbortSignal): Promise<number[][]> {
      const vectors: number[][] = []
      for (let i = 0; i < texts.length; i += EMBED_BATCH) {
        const batch = texts.slice(i, i + EMBED_BATCH)
        const response = await doFetch(`${baseUrl}/embeddings`, {
          method: 'POST',
          headers: headers(),
          signal,
          body: JSON.stringify({ model, input: batch }),
        })
        if (!response.ok) throw await providerError(kind, response)
        const body = (await response.json()) as { data: Array<{ index: number; embedding: number[] }> }
        const ordered = [...body.data].sort((a, b) => a.index - b.index)
        vectors.push(...ordered.map((d) => d.embedding))
      }
      return vectors
    },
  }
}

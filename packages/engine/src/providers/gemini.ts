import { providerError, readSse } from './http.ts'
import {
  DEFAULT_BASE_URLS,
  ProviderError,
  type ChatRequest,
  type ModelInfo,
  type Provider,
  type ProviderConfig,
} from './types.ts'

const EMBED_BATCH = 100

interface GeminiModel {
  name: string
  displayName?: string
  supportedGenerationMethods?: string[]
}

/** Google Gemini through the Generative Language REST API. */
export function createGeminiProvider(config: ProviderConfig, doFetch: typeof fetch): Provider {
  const kind = 'gemini' as const
  const baseUrl = (config.baseUrl || DEFAULT_BASE_URLS.gemini!).replace(/\/+$/, '')

  function headers(): Record<string, string> {
    return { 'Content-Type': 'application/json', 'x-goog-api-key': config.apiKey ?? '' }
  }

  const modelPath = (model: string) => (model.startsWith('models/') ? model : `models/${model}`)

  return {
    kind,
    supportsEmbeddings: true,

    async listModels(): Promise<ModelInfo[]> {
      const models: ModelInfo[] = []
      let pageToken = ''
      do {
        const url = `${baseUrl}/models?pageSize=1000${pageToken ? `&pageToken=${encodeURIComponent(pageToken)}` : ''}`
        const response = await doFetch(url, { headers: headers() })
        if (!response.ok) throw await providerError(kind, response)
        const body = (await response.json()) as { models?: GeminiModel[]; nextPageToken?: string }
        for (const m of body.models ?? []) {
          const methods = m.supportedGenerationMethods ?? []
          const capabilities: ModelInfo['capabilities'] = []
          if (methods.includes('generateContent')) capabilities.push('chat')
          if (methods.includes('embedContent') || methods.includes('batchEmbedContents')) capabilities.push('embedding')
          models.push({ id: m.name.replace(/^models\//, ''), label: m.displayName, capabilities })
        }
        pageToken = body.nextPageToken ?? ''
      } while (pageToken)
      return models
    },

    async chat(request: ChatRequest): Promise<string> {
      const response = await doFetch(`${baseUrl}/${modelPath(request.model)}:streamGenerateContent?alt=sse`, {
        method: 'POST',
        headers: headers(),
        signal: request.signal,
        body: JSON.stringify({
          systemInstruction: { parts: [{ text: request.system }] },
          contents: request.messages.map((m) => ({
            role: m.role === 'assistant' ? 'model' : 'user',
            parts: [{ text: m.content }],
          })),
          ...(request.maxTokens ? { generationConfig: { maxOutputTokens: request.maxTokens } } : {}),
        }),
      })
      if (!response.ok) throw await providerError(kind, response)
      if (!response.body) throw new ProviderError(kind, 0, 'The provider returned an empty response')

      let text = ''
      for await (const data of readSse(response.body, request.signal)) {
        let event: {
          candidates?: Array<{ content?: { parts?: Array<{ text?: string; thought?: boolean }> }; finishReason?: string }>
          promptFeedback?: { blockReason?: string }
          error?: { message?: string }
        }
        try {
          event = JSON.parse(data)
        } catch {
          continue
        }
        if (event.error) throw new ProviderError(kind, 0, event.error.message ?? 'Stream error')
        if (event.promptFeedback?.blockReason) {
          throw new ProviderError(kind, 0, `Gemini blocked the request (${event.promptFeedback.blockReason})`)
        }
        for (const part of event.candidates?.[0]?.content?.parts ?? []) {
          if (part.text && !part.thought) {
            text += part.text
            request.onText?.(part.text)
          }
        }
      }
      return text
    },

    async embed(model: string, texts: string[], signal?: AbortSignal): Promise<number[][]> {
      const path = modelPath(model)
      const vectors: number[][] = []
      for (let i = 0; i < texts.length; i += EMBED_BATCH) {
        const batch = texts.slice(i, i + EMBED_BATCH)
        const response = await doFetch(`${baseUrl}/${path}:batchEmbedContents`, {
          method: 'POST',
          headers: headers(),
          signal,
          body: JSON.stringify({
            requests: batch.map((text) => ({ model: path, content: { parts: [{ text }] } })),
          }),
        })
        if (!response.ok) throw await providerError(kind, response)
        const body = (await response.json()) as { embeddings: Array<{ values: number[] }> }
        vectors.push(...body.embeddings.map((e) => e.values))
      }
      return vectors
    },
  }
}

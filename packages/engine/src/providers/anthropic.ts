import Anthropic from '@anthropic-ai/sdk'

import { ProviderError, type ChatRequest, type ModelInfo, type Provider, type ProviderConfig } from './types.ts'

/** Models that accept the server-side refusal fallback ("default" routing). */
const FALLBACK_MODELS = new Set(['claude-fable-5-1', 'claude-opus-5-5', 'claude-opus-5', 'claude-sonnet-5-5'])

export const DEFAULT_ANTHROPIC_MODEL = 'claude-opus-5-5'

function toProviderError(error: unknown): unknown {
  if (error instanceof Anthropic.AuthenticationError || error instanceof Anthropic.PermissionDeniedError) {
    return new ProviderError('anthropic', error.status, `API key rejected: ${error.message}`)
  }
  if (error instanceof Anthropic.APIError) {
    return new ProviderError('anthropic', error.status ?? 0, error.message)
  }
  return error
}

/**
 * Anthropic through the official SDK. The app runs in a webview, so the SDK's
 * browser guard is lifted; the key lives in the OS keychain and is only ever
 * sent to api.anthropic.com. Pass the Tauri HTTP plugin's fetch to avoid CORS.
 */
export function createAnthropicProvider(config: ProviderConfig, doFetch: typeof fetch): Provider {
  const client = new Anthropic({
    apiKey: config.apiKey ?? '',
    // Explicit so an ANTHROPIC_BASE_URL in the environment can never redirect the key.
    baseURL: config.baseUrl || 'https://api.anthropic.com',
    dangerouslyAllowBrowser: true,
    fetch: doFetch,
  })

  return {
    kind: 'anthropic',
    supportsEmbeddings: false,

    async listModels(): Promise<ModelInfo[]> {
      try {
        const models: ModelInfo[] = []
        for await (const model of client.models.list()) {
          models.push({ id: model.id, label: model.display_name, capabilities: ['chat'] })
        }
        return models
      } catch (error) {
        throw toProviderError(error)
      }
    },

    async chat(request: ChatRequest): Promise<string> {
      const useFallback = FALLBACK_MODELS.has(request.model)
      try {
        const stream = client.beta.messages.stream(
          {
            model: request.model,
            max_tokens: request.maxTokens ?? 64000,
            system: request.system,
            messages: request.messages.map((m) => ({ role: m.role, content: m.content })),
            ...(useFallback ? { betas: ['server-side-fallback-2026-07-01'], fallbacks: 'default' as const } : {}),
          },
          { signal: request.signal },
        )
        if (request.onText) stream.on('text', request.onText)
        const message = await stream.finalMessage()
        if (message.stop_reason === 'refusal') {
          throw new ProviderError('anthropic', 0, 'Claude declined to answer this request.')
        }
        let text = ''
        for (const block of message.content) if (block.type === 'text') text += block.text
        if (message.stop_reason === 'max_tokens') text += '\n\n_(Reply cut off at the output limit.)_'
        return text
      } catch (error) {
        throw toProviderError(error)
      }
    },

    async embed(): Promise<number[][]> {
      throw new ProviderError(
        'anthropic',
        0,
        'Anthropic has no embeddings API. Choose OpenAI, Gemini or an OpenAI-compatible provider for embeddings.',
      )
    },
  }
}

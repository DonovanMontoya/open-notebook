import { createAnthropicProvider } from './anthropic.ts'
import { createGeminiProvider } from './gemini.ts'
import { createOpenAIProvider } from './openai.ts'
import type { Provider, ProviderConfig } from './types.ts'

export function createProvider(config: ProviderConfig, doFetch: typeof fetch = fetch): Provider {
  switch (config.kind) {
    case 'anthropic':
      return createAnthropicProvider(config, doFetch)
    case 'gemini':
      return createGeminiProvider(config, doFetch)
    case 'openai':
    case 'openai-compatible':
      return createOpenAIProvider(config, doFetch)
  }
}

export { DEFAULT_ANTHROPIC_MODEL } from './anthropic.ts'
export { readSse } from './http.ts'
export * from './types.ts'

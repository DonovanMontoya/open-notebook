export type ProviderKind = 'openai' | 'anthropic' | 'gemini' | 'openai-compatible'

export interface ProviderConfig {
  kind: ProviderKind
  /** Required for openai-compatible (OpenRouter, Groq, LM Studio, Ollama...). */
  baseUrl?: string
  apiKey?: string
}

export interface ChatTurn {
  role: 'user' | 'assistant'
  content: string
}

export interface ChatRequest {
  model: string
  system: string
  messages: ChatTurn[]
  maxTokens?: number
  signal?: AbortSignal
  /** Called with each text delta as it streams in. */
  onText?: (delta: string) => void
}

export interface ModelInfo {
  id: string
  label?: string
  /** What the model can be used for, when the provider says. */
  capabilities?: Array<'chat' | 'embedding'>
}

export interface Provider {
  readonly kind: ProviderKind
  readonly supportsEmbeddings: boolean
  listModels(): Promise<ModelInfo[]>
  /** Streams a reply and resolves with the full text. */
  chat(request: ChatRequest): Promise<string>
  embed(model: string, texts: string[], signal?: AbortSignal): Promise<number[][]>
}

export class ProviderError extends Error {
  readonly status: number
  readonly provider: ProviderKind

  constructor(provider: ProviderKind, status: number, message: string) {
    super(message)
    this.name = 'ProviderError'
    this.provider = provider
    this.status = status
  }
}

export const PROVIDER_LABELS: Record<ProviderKind, string> = {
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  gemini: 'Google Gemini',
  'openai-compatible': 'OpenAI-compatible',
}

export const DEFAULT_BASE_URLS: Partial<Record<ProviderKind, string>> = {
  openai: 'https://api.openai.com/v1',
  gemini: 'https://generativelanguage.googleapis.com/v1beta',
  'openai-compatible': 'https://openrouter.ai/api/v1',
}

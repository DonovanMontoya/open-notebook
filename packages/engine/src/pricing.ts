// List prices for estimating what on-device usage costs. Providers change prices; the user can override any of these.

/** Dollars per million input and output tokens, or per minute of audio. */
export interface Price {
  input?: number
  output?: number
  perMinute?: number
}

/** Published standard rates, November 2026. Only models whose price is known are listed. */
export const MODEL_PRICES: Record<string, Price> = {
  'claude-fable-5-1': { input: 10, output: 50 },
  'claude-fable-5': { input: 10, output: 50 },
  'claude-opus-5-5': { input: 4, output: 20 },
  'claude-opus-5': { input: 5, output: 25 },
  'claude-opus-4-8': { input: 5, output: 25 },
  'claude-sonnet-5-5': { input: 2, output: 10 },
  'claude-sonnet-5': { input: 2, output: 10 },
  'claude-haiku-5-5': { input: 0.1, output: 0.5 },
  'text-embedding-3-large': { input: 0.13 },
  'text-embedding-3-small': { input: 0.02 },
  'gpt-4o-mini-tts': { perMinute: 0.015 },
  // The voice layer only; the model it hands questions to is billed by OpenAI on top, and the app can't see those tokens.
  'gpt-live-1': { perMinute: 0.05 },
  'gemini-embedding-001': { input: 0.15 },
  'gemini-2.5-flash': { input: 0.3, output: 2.5 },
}

export interface Priced {
  model: string
  inputTokens: number
  outputTokens: number
  audioSeconds: number
}

/** The price for a model: the user's own first, then the list. Dated ids like claude-opus-5-5-20261001 match their base id. */
export function priceFor(model: string, overrides: Record<string, Price> = {}): Price | undefined {
  const lookup = (table: Record<string, Price>) => table[model] ?? Object.entries(table).find(([id]) => model.startsWith(`${id}-`))?.[1]
  return lookup(overrides) ?? lookup(MODEL_PRICES)
}

/** Estimated dollars for some usage, or undefined when the model has no price. */
export function costOf(usage: Priced, overrides?: Record<string, Price>): number | undefined {
  const price = priceFor(usage.model, overrides)
  if (!price) return undefined
  return (
    (usage.inputTokens * (price.input ?? 0)) / 1e6 + (usage.outputTokens * (price.output ?? 0)) / 1e6 + (usage.audioSeconds / 60) * (price.perMinute ?? 0)
  )
}

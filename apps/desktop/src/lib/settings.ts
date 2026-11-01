import type { AiConfig, ProviderConfig, ProviderKind, VoiceConfig, VoiceKind } from '@open-notebook/engine'
import { useSyncExternalStore } from 'react'

import { MISSING_ENTITLEMENT, secrets } from '../platform/index.ts'

export interface ModelSetting {
  provider: ProviderKind
  model: string
}

export interface AppSettings {
  /** local = everything on this device with your API keys; remote = an Open Notebook server. */
  mode: 'local' | 'remote'
  serverUrl: string
  /** Base URL for the OpenAI-compatible provider (OpenRouter by default, or LM Studio, Ollama...). */
  compatibleBaseUrl: string
  chat?: ModelSetting
  embedding?: ModelSetting
  /** Speech to text for audio and video sources. */
  transcription?: ModelSetting
  /** Model for transformations, insights and study decks; the chat model when unset. */
  transformation?: ModelSetting
  /** Keep API keys in iCloud Keychain so they follow you to other Macs and the iPhone. */
  syncKeys: boolean
}

/** Which saved key each voice provider uses. */
export const VOICE_KEY: Record<VoiceKind, ProviderKind | 'elevenlabs'> = {
  openai: 'openai',
  'openai-live': 'openai',
  gemini: 'gemini',
  elevenlabs: 'elevenlabs',
}

const STORAGE_KEY = 'open-notebook-settings'

export const DEFAULT_SETTINGS: AppSettings = {
  mode: 'local',
  serverUrl: 'https://razer-blade.codlet-butterfly.ts.net:8502',
  compatibleBaseUrl: 'https://openrouter.ai/api/v1',
  syncKeys: false,
}

function read(): AppSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? { ...DEFAULT_SETTINGS, ...JSON.parse(raw) } : DEFAULT_SETTINGS
  } catch {
    return DEFAULT_SETTINGS
  }
}

let current = read()
const listeners = new Set<() => void>()

export function getSettings(): AppSettings {
  return current
}

export function updateSettings(patch: Partial<AppSettings>): void {
  current = { ...current, ...patch }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(current))
  for (const l of listeners) l()
}

export function useSettings(): AppSettings {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb)
      return () => listeners.delete(cb)
    },
    getSettings,
  )
}

// Secrets live in the keychain; cache them so chat doesn't hit the keychain on every request.

export const secretName = {
  /** Chat providers plus voice-only ones such as ElevenLabs. */
  provider: (kind: ProviderKind | 'elevenlabs') => `provider:${kind}`,
  serverPassword: 'server:password',
}

const secretCache = new Map<string, string | null>()
const secretListeners = new Set<() => void>()
let secretVersion = 0

export async function getSecret(name: string): Promise<string | null> {
  if (!secretCache.has(name)) secretCache.set(name, await secrets.get(name))
  return secretCache.get(name) ?? null
}

export async function setSecret(name: string, value: string): Promise<void> {
  const trimmed = value.trim()
  if (trimmed) await secrets.set(name, trimmed, current.syncKeys)
  else await secrets.delete(name)
  secretCache.set(name, trimmed || null)
  secretVersion++
  for (const l of secretListeners) l()
}

/** Re-renders when a secret changes; returns a counter for use as a dependency. */
export function useSecretVersion(): number {
  return useSyncExternalStore(
    (cb) => {
      secretListeners.add(cb)
      return () => secretListeners.delete(cb)
    },
    () => secretVersion,
  )
}

export async function providerConfig(kind: ProviderKind, settings = current): Promise<ProviderConfig> {
  return {
    kind,
    apiKey: (await getSecret(secretName.provider(kind))) ?? undefined,
    baseUrl: kind === 'openai-compatible' ? settings.compatibleBaseUrl : undefined,
  }
}

/** What the local engine reads before each chat, embedding or search call. */
export async function aiConfig(): Promise<AiConfig> {
  const s = current
  return {
    chat: s.chat?.model ? { provider: await providerConfig(s.chat.provider, s), model: s.chat.model } : undefined,
    embedding: s.embedding?.model
      ? { provider: await providerConfig(s.embedding.provider, s), model: s.embedding.model }
      : undefined,
    voiceKeys: Object.fromEntries(
      await Promise.all(
        (Object.keys(VOICE_KEY) as VoiceKind[]).map(async (k) => [k, (await getSecret(secretName.provider(VOICE_KEY[k]))) ?? undefined] as const),
      ),
    ),
    transformation: s.transformation?.model
      ? { provider: await providerConfig(s.transformation.provider, s), model: s.transformation.model }
      : undefined,
    providers: Object.fromEntries(
      await Promise.all(
        (['openai', 'anthropic', 'gemini', 'openai-compatible'] as ProviderKind[]).map(async (k) => [k, await providerConfig(k, s)] as const),
      ),
    ),
    transcription: s.transcription?.model
      ? { provider: await providerConfig(s.transcription.provider, s), model: s.transcription.model }
      : undefined,
  }
}

/** A voice provider config for previews and voice lists, with the right saved key. */
export async function voiceConfig(kind: VoiceKind, model: string): Promise<VoiceConfig> {
  return { kind, model, voices: [], apiKey: (await getSecret(secretName.provider(VOICE_KEY[kind]))) ?? undefined }
}

/** Every secret name the app uses. */
export function allSecretNames(): string[] {
  const kinds = ['openai', 'anthropic', 'gemini', 'openai-compatible', 'elevenlabs'] as const
  return [secretName.serverPassword, ...kinds.map((k) => secretName.provider(k))]
}

export type SyncResult = { ok: true; moved: number } | { ok: false; reason: 'unsigned' | 'error'; message?: string }

/**
 * Turns iCloud Keychain sync on or off and moves existing keys to the matching
 * store. Unsigned builds can't use iCloud Keychain, so the setting stays off.
 */
export async function setKeySync(enabled: boolean): Promise<SyncResult> {
  if (enabled && !(await secrets.syncAvailable())) return { ok: false, reason: 'unsigned' }
  let moved = 0
  try {
    for (const name of allSecretNames()) {
      const value = await secrets.get(name)
      if (!value) continue
      await secrets.set(name, value, enabled)
      moved++
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    return message.includes(MISSING_ENTITLEMENT) ? { ok: false, reason: 'unsigned' } : { ok: false, reason: 'error', message }
  }
  updateSettings({ syncKeys: enabled })
  return { ok: true, moved }
}

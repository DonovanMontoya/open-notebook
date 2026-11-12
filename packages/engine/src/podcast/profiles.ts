import type { VoiceKind } from './voices.ts'

/** One voice in a show. */
export interface Speaker {
  name: string
  /** Voice id for the provider (OpenAI voice name, Gemini voice, ElevenLabs voice id). */
  voice: string
  backstory: string
  personality: string
}

/** Who speaks, and with which voices. Same shape as the server's speaker profiles. */
export interface SpeakerProfile {
  id: string
  name: string
  description: string
  /** On device: which voice provider. Absent for server profiles. */
  voiceKind?: VoiceKind
  /** On device: the voice model name. On a server: its TTS model record id. */
  voiceModel: string
  speakers: Speaker[]
}

/** What an episode covers and how long it runs. Same shape as the server's episode profiles. */
export interface EpisodeProfile {
  id: string
  name: string
  description: string
  speakerProfileId: string
  language?: string | null
  /** Default briefing: the angle and style of the episode. */
  briefing: string
  /** Number of segments (chapters), which sets the length. */
  segments: number
}

export type SpeakerProfileSeed = Omit<SpeakerProfile, 'id'>
export type EpisodeProfileSeed = Omit<EpisodeProfile, 'id' | 'speakerProfileId'> & { speakerProfileName: string }

/** About 280 spoken words per segment, roughly two minutes. */
export const WORDS_PER_SEGMENT = 280

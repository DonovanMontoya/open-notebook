export type * from './backend.ts'
export { buildChatPrompt } from './chat.ts'
export { migrate, type SqlDriver } from './db.ts'
export { FULL_CONTEXT_CHARS, LocalBackend, NotFoundError, type AiConfig, type AudioStore, type FileStore, type LocalDeps, type ModelChoice } from './local.ts'
export * from './providers/index.ts'
export { RemoteBackend } from './remote.ts'
export { chunkText, cosineSimilarity, decodeVector, encodeVector, parseLinks, titleFrom } from './text.ts'
export { bestPassage, linkCitations, matchSource, sentenceAround, type CitableSource } from './citations.ts'
export { finishedBlocks } from './blocks.ts'
export { encodeWav, SAMPLE_RATE } from './podcast/audio.ts'
export { narrationMatches, type LiveSocket, type OpenSocket } from './podcast/live.ts'
export { WORDS_PER_SEGMENT, type EpisodeProfile, type Speaker, type SpeakerProfile } from './podcast/profiles.ts'
export {
  createVoiceProvider,
  DEFAULT_VOICE_MODELS,
  DEFAULT_VOICES,
  VOICE_LABELS,
  type VoiceConfig,
  type VoiceKind,
  type VoiceOption,
} from './podcast/voices.ts'
export { type AskOptions, type AskResult, type AskSearch, type AskStep } from './ask.ts'
export { DEFAULT_TRANSCRIPTION_MODELS, isMedia } from './transcribe.ts'
export { captionsToText, parseWatchPage, pickTrack, youtubeId } from './youtube.ts'
export { isDeckNote, STUDY_PREFIX, type StoredDeck, type StudyDeck, type StudyItem, type StudyKind, type StudyRequest } from './studio.ts'
export { livePayload } from './live-talk.ts'
export { costOf, MODEL_PRICES, priceFor, type Price } from './pricing.ts'

import type { AskOptions, AskResult } from './ask.ts'
import type { StoredDeck, StudyDeck, StudyRequest } from './studio.ts'
import type { EpisodeProfile, SpeakerProfile } from './podcast/profiles.ts'
import type {
  ChatMessage,
  ChatSession,
  ChatSessionWithMessages,
  NewSource,
  Note,
  Notebook,
  SourceDetail,
  SourceSummary,
} from '@open-notebook/api-client'

/** A source the app has already turned into text, such as a PDF read on device. */
export interface DocumentSource {
  type: 'document'
  title: string
  /** Text read on device. Empty for audio and video, which are transcribed. */
  content: string
  fileName?: string
  /** The original file: uploaded to a server, kept on device for download, or transcribed. */
  file?: Blob
}

/** Transformations to run on the source once it is processed, by id. */
export type SourceInput = (NewSource | DocumentSource) & { transformations?: string[] }

/** A reusable prompt that turns a source into an insight (summary, key points...). */
export interface Transformation {
  id: string
  name: string
  title: string
  description: string
  prompt: string
  /** Pre-selected when adding sources. */
  applyDefault: boolean
}

export interface Insight {
  id: string
  sourceId: string
  /** The transformation title it came from, e.g. "Dense Summary". */
  type: string
  content: string
  created?: string
  /** On-device insights show while being written; server ones appear when finished. */
  status?: 'running' | 'completed' | 'failed'
  error?: string
}

/** How much of each source and note goes into chat. Missing entries use the defaults. */
export interface ContextSelection {
  /** full: the whole text; insights: only its insights; off: left out. */
  sources?: Record<string, 'full' | 'insights' | 'off'>
  /** false leaves the note out. */
  notes?: Record<string, boolean>
}

export interface SearchHit {
  kind: 'source' | 'note'
  /** The source or note id. */
  id: string
  title: string
  snippet: string
  score: number
  /** Known for on-device results; look it up from the source otherwise. */
  notebookId?: string
}

export interface SendChatArgs {
  notebookId: string
  /** Omit to start a new session titled from the message. */
  sessionId?: string
  message: string
  context?: ContextSelection
  /** Model for this message: a server model id, or "provider::model" on device. */
  modelOverride?: string
  onText?: (delta: string) => void
  signal?: AbortSignal
}

export interface SendSourceChatArgs {
  sourceId: string
  sessionId?: string
  message: string
  modelOverride?: string
  onText?: (delta: string) => void
  signal?: AbortSignal
}

export interface SendChatResult {
  session: ChatSession
  messages: ChatMessage[]
}

export type EpisodeStatus = 'queued' | 'writing' | 'voicing' | 'saving' | 'completed' | 'failed'

export interface EpisodeLine {
  speaker: string
  text: string
  /** Seconds into the episode; absent when the server did not record timing. */
  start?: number
  end?: number
}

export interface EpisodeChapter {
  title: string
  start: number
}

export interface Episode {
  id: string
  notebookId?: string
  name: string
  summary?: string
  status: EpisodeStatus
  progress?: { done: number; total: number }
  error?: string
  created: string
  duration?: number
  chapters: EpisodeChapter[]
  transcript: EpisodeLine[]
  /** Whether line start times are measured (true) or estimated from text length. */
  exactTiming: boolean
  voiceLabel?: string
}

/** A model registered on a server (connected mode). */
export interface ServerModel {
  id: string
  name: string
  provider: string
  /** language, embedding, text_to_speech or speech_to_text. */
  type: string
}

export type LiveVoice = 'marin' | 'cedar' | 'alloy' | 'coral'

export interface UsageGroup {
  model: string
  feature: string
  calls: number
  inputTokens: number
  outputTokens: number
  audioSeconds: number
  /** Unknown on device, where prices aren't tracked. */
  estimatedUsd?: number | null
}

export interface UsageReport {
  days: number
  calls: number
  estimatedUsd?: number | null
  groups: UsageGroup[]
  /** On device, token counts are estimated from text length. */
  estimated: boolean
}

export interface CreateEpisode {
  notebookId: string
  /** Episode profile id. */
  episodeProfile: string
  /** Speaker profile id; the episode profile's own when omitted. */
  speakerProfile?: string
  focus?: string
  name?: string
}

/**
 * Everything the UI needs, implemented twice: on device (LocalBackend) and
 * against an Open Notebook server (RemoteBackend). Shapes follow the server API.
 */
export interface Backend {
  readonly kind: 'local' | 'remote'
  /** Whether chat replies stream token by token. */
  readonly streamsChat: boolean

  listNotebooks(): Promise<Notebook[]>
  getNotebook(id: string): Promise<Notebook>
  createNotebook(name: string, description?: string): Promise<Notebook>
  updateNotebook(id: string, data: { name?: string; description?: string; archived?: boolean }): Promise<Notebook>
  deleteNotebook(id: string): Promise<void>

  listSources(notebookId: string): Promise<SourceSummary[]>
  getSource(id: string): Promise<SourceDetail>
  addSource(notebookId: string, input: SourceInput): Promise<SourceSummary>
  /** All sources across notebooks, each with the notebook ids it is in. */
  listAllSources(): Promise<SourceSummary[]>
  /** Adds an existing source to another notebook. */
  linkSource(sourceId: string, notebookId: string): Promise<void>
  /** Takes a source out of one notebook without deleting it. */
  unlinkSource(sourceId: string, notebookId: string): Promise<void>
  updateSource(id: string, data: { title?: string }): Promise<SourceDetail>
  /** The original file of an uploaded source, or null when there is none. */
  downloadSource(id: string): Promise<{ data: ArrayBuffer; name: string; type: string } | null>
  /** Re-runs processing for a source that failed. */
  retrySource(id: string): Promise<void>
  deleteSource(id: string): Promise<void>

  listNotes(notebookId: string): Promise<Note[]>
  createNote(notebookId: string, data: { title?: string; content: string; note_type?: 'human' | 'ai' }): Promise<Note>
  updateNote(id: string, data: { title?: string; content?: string }): Promise<Note>
  deleteNote(id: string): Promise<void>

  listChatSessions(notebookId: string): Promise<ChatSession[]>
  getChatSession(id: string): Promise<ChatSessionWithMessages>
  sendChatMessage(args: SendChatArgs): Promise<SendChatResult>
  deleteChatSession(id: string): Promise<void>
  renameChatSession(id: string, title: string): Promise<void>

  /** Conversations about one source, apart from the notebook chat. */
  listSourceChatSessions(sourceId: string): Promise<ChatSession[]>
  getSourceChatSession(sourceId: string, sessionId: string): Promise<ChatSessionWithMessages>
  sendSourceChatMessage(args: SendSourceChatArgs): Promise<SendChatResult>
  deleteSourceChatSession(sourceId: string, sessionId: string): Promise<void>

  search(query: string, notebookId?: string): Promise<SearchHit[]>
  /** Researches a question across all notebooks (or one) and answers with citations. */
  ask(question: string, options?: AskOptions): Promise<AskResult>

  /** Flashcards or a quiz from chosen sources, saved as a note in the notebook. */
  generateStudyDeck(notebookId: string, request: StudyRequest): Promise<StudyDeck>
  listStudyDecks(notebookId: string): Promise<StoredDeck[]>

  listTransformations(): Promise<Transformation[]>
  saveTransformation(input: Omit<Transformation, 'id'> & { id?: string }): Promise<Transformation>
  deleteTransformation(id: string): Promise<void>
  /** Runs a transformation on some text and returns the result, for trying prompts out. */
  runTransformation(transformationId: string, text: string, signal?: AbortSignal): Promise<string>
  /** The instructions placed before every transformation prompt. */
  getTransformationInstructions(): Promise<string>
  setTransformationInstructions(text: string): Promise<void>

  listInsights(sourceId: string): Promise<Insight[]>
  /** Starts a transformation on a source; the insight appears in listInsights when done. */
  createInsight(sourceId: string, transformationId: string): Promise<void>
  deleteInsight(id: string): Promise<void>
  saveInsightAsNote(insightId: string, notebookId: string): Promise<Note>

  /** Starts a GPT Live voice conversation: takes a WebRTC offer, returns the answer SDP. */
  createLiveSession(offerSdp: string, voice: LiveVoice): Promise<string>

  usage(days: number): Promise<UsageReport>
  /** Re-embeds every source with the current embedding model. Returns how many were queued. */
  rebuildIndex(): Promise<number>

  /** Models registered on the server; empty on device, where Settings picks models directly. */
  listModels(): Promise<ServerModel[]>

  /** 'local' writes and voices episodes on device; 'server' uses the server's podcast profiles. */
  readonly podcastMode: 'local' | 'server'
  listEpisodes(notebookId: string): Promise<Episode[]>
  getEpisode(id: string): Promise<Episode>
  createEpisode(input: CreateEpisode): Promise<Episode>
  deleteEpisode(id: string): Promise<void>
  /** A URL the audio element can stream. */
  episodeAudioUrl(id: string): Promise<string>
  /** Makes a failed episode again with the same choices. */
  retryEpisode(id: string): Promise<void>
  podcastProfiles(): Promise<{ episode: EpisodeProfile[]; speaker: SpeakerProfile[] }>
  saveEpisodeProfile(profile: Omit<EpisodeProfile, 'id'> & { id?: string }): Promise<EpisodeProfile>
  deleteEpisodeProfile(id: string): Promise<void>
  saveSpeakerProfile(profile: Omit<SpeakerProfile, 'id'> & { id?: string }): Promise<SpeakerProfile>
  deleteSpeakerProfile(id: string): Promise<void>
}

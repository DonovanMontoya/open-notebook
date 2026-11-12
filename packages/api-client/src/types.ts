// Response and request shapes for the Open Notebook REST API (api/routers/*).
// Kept in step with frontend/src/lib/types so the web and mobile apps agree.

export interface Notebook {
  id: string
  name: string
  description: string
  archived: boolean
  created: string
  updated: string
  source_count: number
  note_count: number
}

export interface CreateNotebookRequest {
  name: string
  description?: string
}

export interface UpdateNotebookRequest {
  name?: string
  description?: string
  archived?: boolean
}

export interface Note {
  id: string
  title: string | null
  content: string | null
  note_type: string | null
  created: string
  updated: string
}

export interface CreateNoteRequest {
  title?: string
  content: string
  note_type?: string
  notebook_id?: string
}

export interface UpdateNoteRequest {
  title?: string
  content?: string
  note_type?: string
}

export interface SourceSummary {
  id: string
  title: string | null
  topics?: string[]
  asset: { file_path?: string; url?: string } | null
  embedded: boolean
  embedded_chunks: number
  insights_count: number
  created: string
  updated: string
  file_available?: boolean
  command_id?: string
  status?: string
  processing_info?: Record<string, unknown>
  /** Notebook ids the source is in, when the API includes them. */
  notebooks?: string[]
}

export interface SourceDetail extends SourceSummary {
  full_text: string
}

export interface SourceInsight {
  id: string
  source_id: string
  insight_type: string
  content: string
  created?: string | null
  updated?: string | null
}

export type NewSource = (
  | { type: 'link'; url: string; title?: string }
  | { type: 'text'; content: string; title?: string }
) & { transformations?: string[] }

export interface ChatSession {
  id: string
  title: string
  notebook_id: string
  created: string
  updated: string
  message_count?: number
  model_override?: string | null
}

export interface ChatMessage {
  id: string
  type: 'human' | 'ai'
  content: string
  timestamp?: string
}

export interface ChatSessionWithMessages extends ChatSession {
  messages: ChatMessage[]
}

/** How much of a source or note goes into the chat context. */
export type ContextMode = 'full content' | 'insights' | 'not in'

export interface ChatContext {
  sources: Array<Record<string, unknown>>
  notes: Array<Record<string, unknown>>
}

export interface BuildContextResponse {
  context: ChatContext
  token_count: number
  char_count: number
}

export type EpisodeStatus =
  | 'running'
  | 'processing'
  | 'completed'
  | 'failed'
  | 'error'
  | 'pending'
  | 'submitted'
  | 'unknown'

export interface PodcastEpisode {
  id: string
  name: string
  briefing: string
  audio_file?: string | null
  audio_url?: string | null
  created?: string | null
  job_status?: EpisodeStatus | null
  error_message?: string | null
  episode_profile?: { name?: string } | null
  speaker_profile?: { name?: string } | null
  transcript?: { transcript?: Array<{ speaker?: string; dialogue?: string }> } | null
  outline?: { segments?: Array<{ name?: string; description?: string }> } | null
}

export interface ServerEpisodeProfile {
  id: string
  name: string
  description: string
  speaker_config: string | null
  speaker_config_name?: string | null
  outline_llm?: string | null
  transcript_llm?: string | null
  language?: string | null
  default_briefing: string
  num_segments: number
  max_tokens?: number | null
}

export interface ServerSpeakerProfile {
  id: string
  name: string
  description: string
  voice_model?: string | null
  speakers: Array<{ name: string; voice_id: string; backstory: string; personality: string; voice_model?: string | null }>
}

export interface PodcastGenerationRequest {
  episode_profile: string
  speaker_profile: string
  episode_name: string
  notebook_id?: string
  content?: string
  briefing_suffix?: string | null
}

export interface PodcastGenerationResponse {
  job_id: string
  status: string
  message: string
  episode_profile: string
  episode_name: string
}

export interface AuthStatus {
  auth_enabled: boolean
  message?: string
}

export interface SearchResult {
  id: string
  parent_id: string
  title: string
  relevance?: number
  similarity?: number
  matches?: string[]
}

export interface SearchResponse {
  results: SearchResult[]
  total_count: number
  search_type: string
}

export interface TransformationRecord {
  id: string
  name: string
  title: string
  description: string
  prompt: string
  apply_default: boolean
}

export type TransformationInput = Omit<TransformationRecord, 'id'>

export interface ServerUsageGroup {
  model: string
  feature: string
  calls: number
  input_tokens: number
  output_tokens: number
  audio_seconds: number
  estimated_usd: number | null
}

export interface ServerUsage {
  days: number
  estimated_usd: number | null
  calls: number
  groups: ServerUsageGroup[]
}

export interface ServerCredential {
  id: string
  name: string
  provider: string
  modalities: string[]
  base_url?: string | null
  has_api_key: boolean
  model_count?: number
  decryption_error?: string | null
}

export interface ServerJob {
  job_id?: string
  id?: string
  command?: string
  name?: string
  status: string
  created?: string
  updated?: string
  error_message?: string | null
}

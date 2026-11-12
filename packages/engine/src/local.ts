import type {
  ChatMessage,
  ChatSession,
  ChatSessionWithMessages,
  Note,
  Notebook,
  SourceDetail,
  SourceSummary,
} from '@open-notebook/api-client'

import type {
  Backend,
  ContextSelection,
  CreateEpisode,
  Insight,
  Transformation,
  Episode,
  EpisodePrompt,
  EpisodeCheck,
  PodcastEffort,
  EpisodeStatus,
  SearchHit,
  SendChatArgs,
  SendChatResult,
  SendSourceChatArgs,
  SourceInput,
  UsageReport,
  LiveVoice,
} from './backend.ts'
import { isLiveBackend, livePayload, type LiveMaterial } from './live-talk.ts'
import { DEFAULT_TRANSFORMATION_INSTRUCTIONS, DEFAULT_TRANSFORMATIONS } from './insights/seeds.ts'
import { encodeWav } from './podcast/audio.ts'
import { CHECK_SYSTEM, RESEARCH_SYSTEM, REVISE_SYSTEM, checkPrompt, materialText, parseCheck, researchPrompt, revisePrompt, type MaterialPart, type ScriptCheck } from './podcast/check.ts'
import type { OpenSocket } from './podcast/live.ts'
import { produce } from './podcast/produce.ts'
import { DEFAULT_EPISODE_PROFILES, DEFAULT_SPEAKER_PROFILES } from './podcast/profile-seeds.ts'
import { WORDS_PER_SEGMENT, type EpisodeProfile, type SpeakerProfile } from './podcast/profiles.ts'
import { parseScript, scriptPrompt, type PodcastScript } from './podcast/script.ts'
import { createVoiceProvider, VOICE_LABELS, type VoiceConfig, type VoiceKind, type VoiceProvider } from './podcast/voices.ts'
import { finalAnswerPrompt, parseStrategy, searchAnswerPrompt, strategyPrompt, type AskOptions, type AskResult } from './ask.ts'
import { buildChatPrompt, fairShare, type ContextChunk, type ContextSource } from './chat.ts'
import { migrate, type SqlDriver } from './db.ts'
import { createProvider } from './providers/index.ts'
import { decksFromNotes, parseDeck, STUDY_INPUT_LIMIT, STUDY_PREFIX, studyInput, studyPrompt, type StoredDeck, type StudyDeck, type StudyRequest } from './studio.ts'
import { isMedia, transcribe } from './transcribe.ts'
import { Semaphore } from './semaphore.ts'
import type { ChatTurn, Provider, ProviderConfig } from './providers/types.ts'
import { chunkText, cosineSimilarity, decodeVector, encodeVector, titleFrom } from './text.ts'

export interface ModelChoice {
  provider: ProviderConfig
  model: string
}

export interface AiConfig {
  chat?: ModelChoice
  /** Optional: without it, search and retrieval fall back to keyword matching. */
  embedding?: ModelChoice
  /** API keys for voice providers, by kind. */
  voiceKeys?: Partial<Record<VoiceKind, string>>
  /** Model for transformations and insights; the chat model when unset. */
  transformation?: ModelChoice
  /** Speech to text for audio and video sources. */
  transcription?: ModelChoice
  /** Every provider with a saved key, so a conversation can switch models. */
  providers?: Partial<Record<ProviderConfig['kind'], ProviderConfig>>
}

/** Original files of uploaded sources, so they can be downloaded or transcribed later. */
export interface FileStore {
  save(sourceId: string, data: Uint8Array, name: string): Promise<string>
  read(ref: string): Promise<ArrayBuffer>
  remove(ref: string): Promise<void>
}

/** Where finished episode audio is kept. The app stores files; tests keep them in memory. */
export interface AudioStore {
  /** Saves a WAV recording and returns a reference for url() and remove(). */
  save(episodeId: string, wav: Uint8Array): Promise<string>
  url(ref: string): Promise<string>
  remove(ref: string): Promise<void>
}

export interface LocalDeps {
  db: SqlDriver
  /** Used for provider calls. In the app this is the Tauri HTTP plugin's fetch (no CORS). */
  fetch: typeof fetch
  /** Downloads a URL and returns its readable text. Supplied by the app (HTML and PDF parsing). */
  extractUrl: (url: string, signal?: AbortSignal) => Promise<{ title: string; text: string }>
  /** Read on every call so settings changes apply without rebuilding the backend. */
  getAiConfig: () => Promise<AiConfig>
  now?: () => Date
  newId?: (prefix: string) => string
  /** Overrides provider construction, for tests. */
  createProvider?: (config: ProviderConfig) => Provider
  /** Original source files. Without it, files are read once and not kept. */
  files?: FileStore
  /** Episode audio storage. Without it, podcasts can't be created. */
  audio?: AudioStore
  /** WebSocket opener for OpenAI Live voices (needs an Authorization header). */
  openSocket?: OpenSocket
  /** Overrides voice provider construction, for tests. */
  createVoiceProvider?: (config: VoiceConfig) => VoiceProvider
}

type SourceStatus = 'processing' | 'completed' | 'failed'

interface NotebookRow {
  id: string
  name: string
  description: string
  archived: number
  created: string
  updated: string
  source_count: number
  note_count: number
}

interface SourceRow {
  id: string
  notebook_id: string
  title: string | null
  kind: string
  url: string | null
  full_text: string
  status: SourceStatus
  error: string | null
  created: string
  updated: string
  chunk_count: number
  embedded_count: number
  insights_count: number
  file_ref: string | null
  file_name: string | null
  file_type: string | null
}

interface NoteRow {
  id: string
  notebook_id: string
  title: string | null
  content: string
  note_type: string
  created: string
  updated: string
}

interface ChunkRow {
  id: string
  source_id: string
  text: string
  embedding: string | null
  title: string | null
}

const HISTORY_TURNS = 20
/** Sources processed at the same time; the rest wait their turn. */
const SOURCE_CONCURRENCY = 2
const SEARCH_LIMIT = 20

export class NotFoundError extends Error {
  constructor(what: string) {
    super(`${what} not found`)
    this.name = 'NotFoundError'
  }
}

export class LocalBackend implements Backend {
  readonly kind = 'local' as const
  readonly streamsChat = true
  readonly podcastMode = 'local' as const

  private readonly db: SqlDriver
  private readonly deps: LocalDeps
  private readonly running = new Map<string, AbortController>()

  private constructor(deps: LocalDeps) {
    this.deps = deps
    this.db = deps.db
  }

  /** Creates the schema and marks sources left mid-processing by a previous run as failed. */
  static async open(deps: LocalDeps): Promise<LocalBackend> {
    await migrate(deps.db)
    const backend = new LocalBackend(deps)
    await deps.db.execute(
      `UPDATE source SET status = 'failed', error = $1 WHERE status = 'processing'`,
      ['Processing was interrupted when the app closed. Retry to continue.'],
    )
    await deps.db.execute(
      `UPDATE podcast_episode SET status = 'failed', error = $1 WHERE status NOT IN ('completed', 'failed')`,
      ['The app closed before this episode finished. Delete it and create it again.'],
    )
    await deps.db.execute(`UPDATE source_insight SET status = 'failed', error = $1 WHERE status = 'running'`, [
      'The app closed before this insight finished. Delete it and run the transformation again.',
    ])
    await backend.seedTransformations()
    await backend.seedPodcastProfiles()
    return backend
  }

  private now(): string {
    return (this.deps.now?.() ?? new Date()).toISOString()
  }

  private id(prefix: string): string {
    return this.deps.newId?.(prefix) ?? `${prefix}:${crypto.randomUUID()}`
  }

  /** A provider whose calls are recorded in the usage log under a feature name. */
  private provider(config: ProviderConfig, feature: string): Provider {
    const inner = this.deps.createProvider?.(config) ?? createProvider(config, this.deps.fetch)
    const record = (model: string, inputChars: number, outputChars: number) =>
      this.recordUsage({ feature, model, inputTokens: Math.ceil(inputChars / 4), outputTokens: Math.ceil(outputChars / 4) })
    return {
      ...inner,
      kind: inner.kind,
      supportsEmbeddings: inner.supportsEmbeddings,
      listModels: () => inner.listModels(),
      async chat(request) {
        const reply = await inner.chat(request)
        const input = request.system.length + request.messages.reduce((n, m) => n + m.content.length, 0)
        await record(request.model, input, reply.length)
        return reply
      },
      async embed(model, texts, signal) {
        const vectors = await inner.embed(model, texts, signal)
        await record(model, texts.reduce((n, t) => n + t.length, 0), 0)
        return vectors
      },
    }
  }

  private async recordUsage(e: { feature: string; model: string; inputTokens?: number; outputTokens?: number; audioSeconds?: number }) {
    await this.db
      .execute(
        `INSERT INTO usage_event (id, created, feature, model, input_tokens, output_tokens, audio_seconds) VALUES ($1, $2, $3, $4, $5, $6, $7)`,
        [this.id('usage'), this.now(), e.feature, e.model, e.inputTokens ?? 0, e.outputTokens ?? 0, e.audioSeconds ?? 0],
      )
      .catch(() => undefined)
  }

  async usage(days: number): Promise<UsageReport> {
    const since = new Date((this.deps.now?.() ?? new Date()).getTime() - days * 86_400_000).toISOString()
    const groups = await this.db.select<{ model: string; feature: string; calls: number; input: number; output: number; audio: number }>(
      `SELECT model, feature, COUNT(*) AS calls, SUM(input_tokens) AS input, SUM(output_tokens) AS output, SUM(audio_seconds) AS audio
         FROM usage_event WHERE created >= $1 GROUP BY model, feature ORDER BY calls DESC`,
      [since],
    )
    return {
      days,
      calls: groups.reduce((n, g) => n + Number(g.calls), 0),
      estimated: true,
      groups: groups.map((g) => ({
        model: g.model,
        feature: g.feature,
        calls: Number(g.calls),
        inputTokens: Number(g.input),
        outputTokens: Number(g.output),
        audioSeconds: Number(g.audio),
      })),
    }
  }

  async rebuildIndex(): Promise<number> {
    const rows = await this.db.select<{ id: string }>(`SELECT id FROM source WHERE status IN ('completed', 'failed')`)
    for (const r of rows) await this.retrySource(r.id)
    return rows.length
  }


  // Notebooks

  async listNotebooks(): Promise<Notebook[]> {
    const rows = await this.db.select<NotebookRow>(
      `SELECT n.*,
              (SELECT COUNT(*) FROM notebook_source l WHERE l.notebook_id = n.id) AS source_count,
              (SELECT COUNT(*) FROM note t WHERE t.notebook_id = n.id) AS note_count
         FROM notebook n ORDER BY n.updated DESC`,
    )
    return rows.map(toNotebook)
  }

  async getNotebook(id: string): Promise<Notebook> {
    const rows = await this.db.select<NotebookRow>(
      `SELECT n.*,
              (SELECT COUNT(*) FROM notebook_source l WHERE l.notebook_id = n.id) AS source_count,
              (SELECT COUNT(*) FROM note t WHERE t.notebook_id = n.id) AS note_count
         FROM notebook n WHERE n.id = $1`,
      [id],
    )
    if (!rows[0]) throw new NotFoundError('Notebook')
    return toNotebook(rows[0])
  }

  async createNotebook(name: string, description = ''): Promise<Notebook> {
    const id = this.id('notebook')
    const now = this.now()
    await this.db.execute(
      `INSERT INTO notebook (id, name, description, archived, created, updated) VALUES ($1, $2, $3, 0, $4, $4)`,
      [id, name.trim(), description.trim(), now],
    )
    return this.getNotebook(id)
  }

  async updateNotebook(id: string, data: { name?: string; description?: string; archived?: boolean }): Promise<Notebook> {
    const current = await this.getNotebook(id)
    await this.db.execute(`UPDATE notebook SET name = $1, description = $2, archived = $3, updated = $4 WHERE id = $5`, [
      data.name?.trim() || current.name,
      data.description ?? current.description,
      (data.archived ?? current.archived) ? 1 : 0,
      this.now(),
      id,
    ])
    return this.getNotebook(id)
  }

  async deleteNotebook(id: string): Promise<void> {
    const episodes = await this.db.select<{ id: string }>(`SELECT id FROM podcast_episode WHERE notebook_id = $1`, [id])
    for (const e of episodes) await this.deleteEpisode(e.id)
    // Sources only in this notebook go with it; shared ones just lose the link, like on the server.
    const sources = await this.db.select<{ id: string; links: number }>(
      `SELECT l.source_id AS id, (SELECT COUNT(*) FROM notebook_source o WHERE o.source_id = l.source_id) AS links
         FROM notebook_source l WHERE l.notebook_id = $1`,
      [id],
    )
    for (const s of sources) {
      if (Number(s.links) <= 1) await this.deleteSource(s.id)
      else await this.unlinkSource(s.id, id)
    }
    const sessions = await this.db.select<{ id: string }>(`SELECT id FROM chat_session WHERE notebook_id = $1`, [id])
    for (const s of sessions) await this.deleteChatSession(s.id)
    await this.db.execute(`DELETE FROM note WHERE notebook_id = $1`, [id])
    await this.db.execute(`DELETE FROM notebook WHERE id = $1`, [id])
  }

  private async touchNotebook(id: string): Promise<void> {
    await this.db.execute(`UPDATE notebook SET updated = $1 WHERE id = $2`, [this.now(), id])
  }

  // Sources

  private sourceQuery(where: string): string {
    return `SELECT s.*,
                   (SELECT COUNT(*) FROM chunk c WHERE c.source_id = s.id) AS chunk_count,
                   (SELECT COUNT(*) FROM chunk c WHERE c.source_id = s.id AND c.embedding IS NOT NULL) AS embedded_count,
                   (SELECT COUNT(*) FROM source_insight i WHERE i.source_id = s.id AND i.status = 'completed') AS insights_count
              FROM source s WHERE ${where}`
  }

  async listSources(notebookId: string): Promise<SourceSummary[]> {
    const rows = await this.db.select<SourceRow>(`${this.sourceQuery(`s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = $1)`)} ORDER BY s.updated DESC`, [
      notebookId,
    ])
    return rows.map(toSourceSummary)
  }

  async getSource(id: string): Promise<SourceDetail> {
    const rows = await this.db.select<SourceRow>(this.sourceQuery('s.id = $1'), [id])
    const row = rows[0]
    if (!row) throw new NotFoundError('Source')
    const links = await this.db.select<{ notebook_id: string }>(`SELECT notebook_id FROM notebook_source WHERE source_id = $1`, [id])
    const notebooks = [row.notebook_id, ...links.map((l) => l.notebook_id).filter((n) => n !== row.notebook_id)]
    return { ...toSourceSummary(row), full_text: row.full_text, notebooks }
  }

  async addSource(notebookId: string, input: SourceInput): Promise<SourceSummary> {
    await this.getNotebook(notebookId)
    const id = this.id('source')
    const now = this.now()
    const url = input.type === 'link' ? input.url.trim() : null
    const text = input.type === 'link' ? '' : input.content
    const title = input.title?.trim() || (input.type === 'link' ? url : titleFrom(text)) || 'Untitled source'
    await this.db.execute(
      `INSERT INTO source (id, notebook_id, title, kind, url, full_text, status, pending_transformations, created, updated)
       VALUES ($1, $2, $3, $4, $5, $6, 'processing', $7, $8, $8)`,
      [id, notebookId, title, input.type, url, text, input.transformations?.length ? JSON.stringify(input.transformations) : null, now],
    )
    await this.db.execute(`INSERT INTO notebook_source (source_id, notebook_id) VALUES ($1, $2)`, [id, notebookId])
    if (input.type === 'document' && input.file) {
      const name = input.fileName || input.title || 'source'
      const media = isMedia(input.file.type, name)
      if (media && !this.deps.files) throw new Error('This build cannot keep audio or video files for transcription.')
      if (this.deps.files) {
        const ref = await this.deps.files.save(id, new Uint8Array(await input.file.arrayBuffer()), name)
        await this.db.execute(`UPDATE source SET file_ref = $1, file_name = $2, file_type = $3, kind = $4 WHERE id = $5`, [
          ref,
          name,
          input.file.type,
          media ? 'media' : 'document',
          id,
        ])
      }
    }
    await this.touchNotebook(notebookId)
    this.process(id, input.title?.trim() ? input.title.trim() : undefined)
    return toSourceSummary((await this.db.select<SourceRow>(this.sourceQuery('s.id = $1'), [id]))[0])
  }

  async retrySource(id: string): Promise<void> {
    await this.getSource(id)
    await this.db.execute(`UPDATE source SET status = 'processing', error = NULL, updated = $1 WHERE id = $2`, [
      this.now(),
      id,
    ])
    this.process(id)
  }

  async deleteSource(id: string): Promise<void> {
    this.running.get(id)?.abort()
    await this.db.execute(`DELETE FROM chunk WHERE source_id = $1`, [id])
    await this.db.execute(`DELETE FROM source_insight WHERE source_id = $1`, [id])
    const chats = await this.db.select<{ id: string }>(`SELECT id FROM chat_session WHERE source_id = $1`, [id])
    for (const c of chats) await this.deleteChatSession(c.id)
    const [file] = await this.db.select<{ file_ref: string | null }>(`SELECT file_ref FROM source WHERE id = $1`, [id])
    if (file?.file_ref) await this.deps.files?.remove(file.file_ref).catch(() => undefined)
    await this.db.execute(`DELETE FROM notebook_source WHERE source_id = $1`, [id])
    await this.db.execute(`DELETE FROM source WHERE id = $1`, [id])
  }

  /** Every source in every notebook, newest first, with the notebooks each is in. */
  async listAllSources(): Promise<SourceSummary[]> {
    const rows = await this.db.select<SourceRow>(`${this.sourceQuery('1 = 1')} ORDER BY s.updated DESC`)
    const links = await this.db.select<{ source_id: string; notebook_id: string }>(`SELECT source_id, notebook_id FROM notebook_source`)
    const bySource = new Map<string, string[]>()
    for (const l of links) bySource.set(l.source_id, [...(bySource.get(l.source_id) ?? []), l.notebook_id])
    return rows.map((r) => ({ ...toSourceSummary(r), notebooks: bySource.get(r.id) ?? [r.notebook_id] }))
  }

  async linkSource(sourceId: string, notebookId: string): Promise<void> {
    await this.getSource(sourceId)
    await this.getNotebook(notebookId)
    await this.db.execute(`INSERT OR IGNORE INTO notebook_source (source_id, notebook_id) VALUES ($1, $2)`, [sourceId, notebookId])
    await this.touchNotebook(notebookId)
  }

  async unlinkSource(sourceId: string, notebookId: string): Promise<void> {
    // source.notebook_id must name a notebook and cascades when that notebook is deleted, so a source
    // with no notebook left would be deleted along with its former one. Keep at least one link.
    const links = await this.db.select<{ notebook_id: string }>(`SELECT notebook_id FROM notebook_source WHERE source_id = $1`, [sourceId])
    if (!links.some((l) => l.notebook_id === notebookId)) return
    if (links.length <= 1) throw new Error('A source has to stay in at least one notebook. Delete the source instead.')
    await this.db.execute(`DELETE FROM notebook_source WHERE source_id = $1 AND notebook_id = $2`, [sourceId, notebookId])
    // Keep the home notebook pointing at a notebook the source is still in.
    const [left] = await this.db.select<{ notebook_id: string }>(`SELECT notebook_id FROM notebook_source WHERE source_id = $1`, [sourceId])
    if (left) await this.db.execute(`UPDATE source SET notebook_id = $1 WHERE id = $2 AND notebook_id = $3`, [left.notebook_id, sourceId, notebookId])
  }

  async downloadSource(id: string): Promise<{ data: ArrayBuffer; name: string; type: string } | null> {
    const [row] = await this.db.select<{ file_ref: string | null; file_name: string | null; file_type: string | null }>(
      `SELECT file_ref, file_name, file_type FROM source WHERE id = $1`,
      [id],
    )
    if (!row?.file_ref || !this.deps.files) return null
    return { data: await this.deps.files.read(row.file_ref), name: row.file_name ?? 'source', type: row.file_type ?? '' }
  }

  async updateSource(id: string, data: { title?: string }): Promise<SourceDetail> {
    if (data.title !== undefined) {
      await this.db.execute(`UPDATE source SET title = $1, updated = $2 WHERE id = $3`, [data.title.trim() || 'Untitled source', this.now(), id])
    }
    return this.getSource(id)
  }

  /** Resolves when no source is processing. Used by tests and before quitting. */
  async idle(): Promise<void> {
    while (this.jobs.size) await Promise.allSettled([...this.jobs])
  }

  private readonly jobs = new Set<Promise<void>>()
  private readonly slots = new Semaphore(SOURCE_CONCURRENCY)

  /** Queues a source for processing; at most SOURCE_CONCURRENCY run at once. */
  private process(id: string, explicitTitle?: string): void {
    const controller = new AbortController()
    this.running.set(id, controller)
    const job = this.slots
      .run(() => (controller.signal.aborted ? Promise.resolve() : this.runPipeline(id, controller.signal, explicitTitle)))
      .catch(async (error: unknown) => {
        if (controller.signal.aborted) return
        const message = error instanceof Error ? error.message : String(error)
        await this.db.execute(`UPDATE source SET status = 'failed', error = $1, updated = $2 WHERE id = $3`, [
          message,
          this.now(),
          id,
        ])
      })
      .finally(() => {
        this.running.delete(id)
        this.jobs.delete(job)
      })
    this.jobs.add(job)
  }

  /** Extract (links), chunk, embed (when an embedding model is set), then mark completed. */
  private async runPipeline(id: string, signal: AbortSignal, explicitTitle?: string): Promise<void> {
    const source = await this.getSource(id)
    let text = source.full_text
    let title = source.title

    if (source.asset?.url && !text) {
      const extracted = await this.deps.extractUrl(source.asset.url, signal)
      text = extracted.text
      if (!explicitTitle && extracted.title) title = extracted.title
      if (!text.trim()) throw new Error('No readable text was found at that address.')
      await this.db.execute(`UPDATE source SET full_text = $1, title = $2, updated = $3 WHERE id = $4`, [
        text,
        title,
        this.now(),
        id,
      ])
    }

    if (!text.trim()) {
      const [row] = await this.db.select<{ kind: string; file_ref: string | null; file_name: string | null; file_type: string | null }>(
        `SELECT kind, file_ref, file_name, file_type FROM source WHERE id = $1`,
        [id],
      )
      if (row?.kind === 'media' && row.file_ref && this.deps.files) {
        const config = await this.deps.getAiConfig()
        if (!config.transcription) throw new Error('Choose a speech to text model in Settings to transcribe audio and video.')
        text = await transcribe(
          config.transcription.provider,
          config.transcription.model,
          { data: await this.deps.files.read(row.file_ref), name: row.file_name ?? 'recording', type: row.file_type ?? '' },
          this.deps.fetch,
          signal,
        )
        if (!text) throw new Error('No speech was found in this recording.')
        await this.recordUsage({ feature: 'transcription', model: config.transcription.model, outputTokens: Math.ceil(text.length / 4) })
        await this.db.execute(`UPDATE source SET full_text = $1, updated = $2 WHERE id = $3`, [text, this.now(), id])
      }
    }

    const pieces = chunkText(text)
    const config = await this.deps.getAiConfig()
    let vectors: number[][] | null = null
    if (config.embedding && pieces.length) {
      const provider = this.provider(config.embedding.provider, 'embedding')
      vectors = await provider.embed(config.embedding.model, pieces, signal)
    }
    if (signal.aborted) return

    await this.db.execute(`DELETE FROM chunk WHERE source_id = $1`, [id])
    for (let i = 0; i < pieces.length; i++) {
      await this.db.execute(
        `INSERT INTO chunk (id, source_id, position, text, embedding, embedding_model) VALUES ($1, $2, $3, $4, $5, $6)`,
        [
          `${id}#${i}`,
          id,
          i,
          pieces[i],
          vectors ? encodeVector(vectors[i]) : null,
          vectors ? config.embedding!.model : null,
        ],
      )
    }
    await this.db.execute(`UPDATE source SET status = 'completed', error = NULL, updated = $1 WHERE id = $2`, [
      this.now(),
      id,
    ])
    // Transformations chosen when the source was added run once its text is ready.
    const [pending] = await this.db.select<{ pending_transformations: string | null }>(
      `SELECT pending_transformations FROM source WHERE id = $1`,
      [id],
    )
    if (pending?.pending_transformations) {
      await this.db.execute(`UPDATE source SET pending_transformations = NULL WHERE id = $1`, [id])
      for (const tid of JSON.parse(pending.pending_transformations) as string[]) await this.createInsight(id, tid).catch(() => undefined)
    }
  }

  // Notes

  async listNotes(notebookId: string): Promise<Note[]> {
    const rows = await this.db.select<NoteRow>(`SELECT * FROM note WHERE notebook_id = $1 ORDER BY updated DESC`, [
      notebookId,
    ])
    return rows.map(toNote)
  }

  private async getNote(id: string): Promise<NoteRow> {
    const rows = await this.db.select<NoteRow>(`SELECT * FROM note WHERE id = $1`, [id])
    if (!rows[0]) throw new NotFoundError('Note')
    return rows[0]
  }

  async createNote(
    notebookId: string,
    data: { title?: string; content: string; note_type?: 'human' | 'ai' },
  ): Promise<Note> {
    await this.getNotebook(notebookId)
    const id = this.id('note')
    const now = this.now()
    const title = data.title?.trim() || titleFrom(data.content) || 'Untitled note'
    await this.db.execute(
      `INSERT INTO note (id, notebook_id, title, content, note_type, created, updated) VALUES ($1, $2, $3, $4, $5, $6, $6)`,
      [id, notebookId, title, data.content, data.note_type ?? 'human', now],
    )
    await this.touchNotebook(notebookId)
    return toNote(await this.getNote(id))
  }

  async updateNote(id: string, data: { title?: string; content?: string }): Promise<Note> {
    const current = await this.getNote(id)
    await this.db.execute(`UPDATE note SET title = $1, content = $2, updated = $3 WHERE id = $4`, [
      data.title ?? current.title,
      data.content ?? current.content,
      this.now(),
      id,
    ])
    await this.touchNotebook(current.notebook_id)
    return toNote(await this.getNote(id))
  }

  async deleteNote(id: string): Promise<void> {
    await this.db.execute(`DELETE FROM note WHERE id = $1`, [id])
  }

  // Chat

  async listChatSessions(notebookId: string): Promise<ChatSession[]> {
    return this.db.select<ChatSession>(
      `SELECT s.*, (SELECT COUNT(*) FROM chat_message m WHERE m.session_id = s.id) AS message_count
         FROM chat_session s WHERE s.notebook_id = $1 AND s.source_id IS NULL ORDER BY s.updated DESC`,
      [notebookId],
    )
  }

  async getChatSession(id: string): Promise<ChatSessionWithMessages> {
    const sessions = await this.db.select<ChatSession>(`SELECT * FROM chat_session WHERE id = $1`, [id])
    if (!sessions[0]) throw new NotFoundError('Chat session')
    const messages = await this.db.select<ChatMessage>(
      `SELECT id, type, content, timestamp FROM chat_message WHERE session_id = $1 ORDER BY timestamp, id`,
      [id],
    )
    return { ...sessions[0], message_count: messages.length, messages }
  }

  async deleteChatSession(id: string): Promise<void> {
    await this.db.execute(`DELETE FROM chat_message WHERE session_id = $1`, [id])
    await this.db.execute(`DELETE FROM chat_session WHERE id = $1`, [id])
  }

  async renameChatSession(id: string, title: string): Promise<void> {
    await this.db.execute(`UPDATE chat_session SET title = $1 WHERE id = $2`, [title.trim() || 'Untitled conversation', id])
  }

  async sendChatMessage(args: SendChatArgs): Promise<SendChatResult> {
    const notebook = await this.getNotebook(args.notebookId)
    return this.converse({
      ...args,
      buildSystem: async (config) => {
        const { sources, chunks } = await this.contextFor(args.notebookId, args.message, config, args.signal, args.context)
        const notes = (await this.listNotes(args.notebookId))
          .filter((n) => args.context?.notes?.[n.id] !== false)
          .map((n) => ({ title: n.title ?? 'Note', content: n.content ?? '' }))
        return buildChatPrompt({ notebook, sources, chunks, notes })
      },
    })
  }

  // Source chat: a conversation about one source.

  async listSourceChatSessions(sourceId: string): Promise<ChatSession[]> {
    return this.db.select<ChatSession>(
      `SELECT s.*, (SELECT COUNT(*) FROM chat_message m WHERE m.session_id = s.id) AS message_count
         FROM chat_session s WHERE s.source_id = $1 ORDER BY s.updated DESC`,
      [sourceId],
    )
  }

  getSourceChatSession(_sourceId: string, sessionId: string): Promise<ChatSessionWithMessages> {
    return this.getChatSession(sessionId)
  }

  deleteSourceChatSession(_sourceId: string, sessionId: string): Promise<void> {
    return this.deleteChatSession(sessionId)
  }

  async sendSourceChatMessage(args: SendSourceChatArgs): Promise<SendChatResult> {
    const source = await this.getSource(args.sourceId)
    const notebookId = source.notebooks?.[0] ?? ''
    const notebook = await this.getNotebook(notebookId)
    return this.converse({
      ...args,
      notebookId,
      sourceId: args.sourceId,
      buildSystem: async (config) => {
        const title = source.title ?? 'Untitled'
        const insights = (await this.listInsights(args.sourceId)).filter((i) => i.status === 'completed')
        const insightText = insights.map((i) => `## ${i.type}\n${i.content}`).join('\n\n')
        const fits = source.full_text.length <= FULL_CONTEXT_CHARS
        const chunks = fits
          ? []
          : (await this.rankChunks(args.message, config, notebookId, RETRIEVAL_CHUNKS, args.signal, new Set([args.sourceId]))).map(
              (h) => ({ title: h.title, text: h.text }),
            )
        const sources = [
          ...(fits ? [{ title, text: source.full_text }] : []),
          ...(insightText ? [{ title: `${title} (insights)`, text: insightText }] : []),
        ]
        return buildChatPrompt({
          notebook: { name: notebook.name, description: `A conversation about one source: "${title}". Answer from it.` },
          sources,
          chunks,
          notes: [],
        })
      },
    })
  }

  /** Shared by notebook and source chat: history, the model call, then saving both turns. */
  private async converse(args: {
    notebookId: string
    sourceId?: string
    sessionId?: string
    message: string
    modelOverride?: string
    onText?: (delta: string) => void
    signal?: AbortSignal
    buildSystem: (config: AiConfig) => Promise<string>
  }): Promise<SendChatResult> {
    const config = await this.deps.getAiConfig()
    let chat = config.chat
    if (args.modelOverride) {
      const [kind, ...rest] = args.modelOverride.split('::')
      const provider = config.providers?.[kind as ProviderConfig['kind']] ?? (config.chat?.provider.kind === kind ? config.chat.provider : undefined)
      if (!provider || !rest.length) throw new Error(`No API key is saved for the model ${args.modelOverride}.`)
      chat = { provider, model: rest.join('::') }
    }
    if (!chat) throw new Error('Choose a chat model in Settings first.')

    const history: ChatTurn[] = []
    if (args.sessionId) {
      const existing = await this.getChatSession(args.sessionId)
      for (const m of existing.messages.slice(-HISTORY_TURNS)) {
        history.push({ role: m.type === 'human' ? 'user' : 'assistant', content: m.content })
      }
    }

    const reply = await this.provider(chat.provider, 'chat').chat({
      model: chat.model,
      system: await args.buildSystem(config),
      messages: [...history, { role: 'user', content: args.message }],
      signal: args.signal,
      onText: args.onText,
    })

    // Persist only after a successful reply, so a failed request leaves no orphan turn.
    const now = this.now()
    let sessionId = args.sessionId
    if (!sessionId) {
      sessionId = this.id('chat_session')
      await this.db.execute(
        `INSERT INTO chat_session (id, notebook_id, source_id, title, created, updated) VALUES ($1, $2, $3, $4, $5, $5)`,
        [sessionId, args.notebookId, args.sourceId ?? null, titleFrom(args.message, 40) || 'New chat', now],
      )
    }
    const later = new Date(Date.parse(now) + 1).toISOString()
    await this.db.execute(`INSERT INTO chat_message (id, session_id, type, content, timestamp) VALUES ($1, $2, 'human', $3, $4)`, [
      this.id('chat_message'),
      sessionId,
      args.message,
      now,
    ])
    await this.db.execute(`INSERT INTO chat_message (id, session_id, type, content, timestamp) VALUES ($1, $2, 'ai', $3, $4)`, [
      this.id('chat_message'),
      sessionId,
      reply,
      later,
    ])
    await this.db.execute(`UPDATE chat_session SET updated = $1 WHERE id = $2`, [later, sessionId])

    const session = await this.getChatSession(sessionId)
    return { session, messages: session.messages }
  }

  /**
   * Small notebooks go to the model whole. Larger ones send the chunks most
   * similar to the question, or keyword matches when no embedding model is set.
   */
  private async contextFor(
    notebookId: string,
    question: string,
    config: AiConfig,
    signal?: AbortSignal,
    selection?: ContextSelection,
  ): Promise<{ sources: ContextSource[]; chunks: ContextChunk[] }> {
    const rows = await this.db.select<{ id: string; title: string | null; full_text: string }>(
      `SELECT id, title, full_text FROM source
        WHERE id IN (SELECT source_id FROM notebook_source WHERE notebook_id = $1) AND status = 'completed' ORDER BY created`,
      [notebookId],
    )
    const insights = await this.db.select<{ source_id: string; insight_type: string; content: string }>(
      `SELECT i.source_id, i.insight_type, i.content FROM source_insight i JOIN source s ON s.id = i.source_id
        WHERE s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = $1) AND i.status = 'completed' ORDER BY i.created`,
      [notebookId],
    )
    const bySource = new Map<string, string[]>()
    for (const i of insights) bySource.set(i.source_id, [...(bySource.get(i.source_id) ?? []), `## ${i.insight_type}\n${i.content}`])

    // Without a choice, a source contributes its insights if it has some and its full text otherwise.
    const mode = (id: string) => selection?.sources?.[id] ?? (bySource.has(id) ? 'insights' : 'full')
    const sources: ContextSource[] = []
    const full = rows.filter((r) => mode(r.id) === 'full')
    for (const r of rows) {
      if (mode(r.id) === 'insights' && bySource.has(r.id)) {
        sources.push({ title: `${r.title ?? 'Untitled'} (insights)`, text: bySource.get(r.id)!.join('\n\n') })
      }
    }
    const total = full.reduce((sum, r) => sum + r.full_text.length, 0)
    const insightChars = sources.reduce((sum, s) => sum + s.text.length, 0)
    const budget = Math.max(FULL_CONTEXT_CHARS - insightChars, FULL_CONTEXT_CHARS / 2)
    const whole = (r: (typeof rows)[number]) => ({ title: r.title ?? 'Untitled', text: r.full_text })
    if (total <= budget) return { sources: [...full.map(whole), ...sources], chunks: [] }

    // Too much to send whole. Every source gets a fair share of the budget, so a question about all of
    // them sees all of them; a source longer than its share sends its opening plus its most relevant passages.
    const shares = fairShare(
      full.map((r) => r.full_text.length),
      budget,
    )
    const clipped = full.map((r, i) => ({ r, share: shares[i] })).filter((x) => x.share < x.r.full_text.length)
    const ids = clipped.map((x) => x.r.id)
    const scores = new Map((await this.rankChunks(question, config, notebookId, Infinity, signal, new Set(ids))).map((h) => [h.id, h.score]))
    const chunkRows = await this.db.select<{ id: string; source_id: string; position: number; text: string }>(
      `SELECT id, source_id, position, text FROM chunk WHERE source_id IN (${ids.map((_, i) => `$${i + 1}`).join(', ')}) ORDER BY position`,
      ids,
    )
    const chunks: ContextChunk[] = []
    for (const { r, share } of clipped) {
      const own = chunkRows.filter((c) => c.source_id === r.id)
      const title = r.title ?? 'Untitled'
      if (!own.length) {
        chunks.push({ title, text: r.full_text.slice(0, share) })
        continue
      }
      const [opening, ...rest] = own
      const ranked = [opening, ...rest.sort((a, b) => (scores.get(b.id) ?? 0) - (scores.get(a.id) ?? 0))]
      const picked = new Set<string>()
      let used = 0
      for (const c of ranked) {
        if (used + c.text.length > share) continue
        picked.add(c.id)
        used += c.text.length
      }
      if (!picked.size) {
        // Every chunk is longer than this source's share: send the most relevant one, cut to fit.
        const best = own.reduce((top, c) => ((scores.get(c.id) ?? 0) > (scores.get(top.id) ?? 0) ? c : top), opening)
        if (share > 0) chunks.push({ title, text: best.text.slice(0, share) })
        continue
      }
      for (const c of own) if (picked.has(c.id)) chunks.push({ title, text: c.text })
    }
    return { sources: [...full.filter((_, i) => shares[i] >= full[i].full_text.length).map(whole), ...sources], chunks }
  }

  private async rankChunks(
    query: string,
    config: AiConfig,
    notebookId: string | undefined,
    limit: number,
    signal?: AbortSignal,
    onlySources?: Set<string>,
  ): Promise<Array<{ id: string; sourceId: string; title: string; text: string; score: number }>> {
    const keep = (r: { source_id: string }) => !onlySources || onlySources.has(r.source_id)
    if (config.embedding) {
      const model = config.embedding.model
      const [queryVector] = await this.provider(config.embedding.provider, 'search').embed(model, [query], signal)
      const rows = await this.db.select<ChunkRow>(
        `SELECT c.id, c.source_id, c.text, c.embedding, s.title FROM chunk c JOIN source s ON s.id = c.source_id
          WHERE c.embedding IS NOT NULL AND c.embedding_model = $1${notebookId ? ' AND s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = $2)' : ''}`,
        notebookId ? [model, notebookId] : [model],
      )
      if (rows.length) {
        return rows
          .filter(keep)
          .map((r) => ({
            id: r.id,
            sourceId: r.source_id,
            title: r.title ?? 'Untitled',
            text: r.text,
            score: cosineSimilarity(queryVector, decodeVector(r.embedding!)),
          }))
          .sort((a, b) => b.score - a.score)
          .slice(0, limit)
      }
    }

    // Keyword fallback: score chunks by how many query terms they contain.
    const terms = keywordTerms(query)
    if (!terms.length) return []
    const rows = await this.db.select<ChunkRow>(
      `SELECT c.id, c.source_id, c.text, NULL AS embedding, s.title FROM chunk c JOIN source s ON s.id = c.source_id
        ${notebookId ? 'WHERE s.id IN (SELECT source_id FROM notebook_source WHERE notebook_id = $1)' : ''}`,
      notebookId ? [notebookId] : [],
    )
    return rows
      .filter(keep)
      .map((r) => ({ id: r.id, sourceId: r.source_id, title: r.title ?? 'Untitled', text: r.text, score: keywordScore(r.text, terms) }))
      .filter((r) => r.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit)
  }

  // Search

  async search(query: string, notebookId?: string): Promise<SearchHit[]> {
    const config = await this.deps.getAiConfig()
    const chunks = await this.rankChunks(query, config, notebookId, SEARCH_LIMIT * 3)
    const notebookOf = new Map(
      (await this.db.select<{ id: string; notebook_id: string }>(`SELECT id, notebook_id FROM source`)).map((r) => [r.id, r.notebook_id]),
    )
    const bestBySource = new Map<string, SearchHit>()
    for (const c of chunks) {
      if (!bestBySource.has(c.sourceId)) {
        bestBySource.set(c.sourceId, { kind: 'source', id: c.sourceId, title: c.title, snippet: snippet(c.text, query), score: c.score, notebookId: notebookOf.get(c.sourceId) })
      }
    }

    const terms = keywordTerms(query)
    const notes = terms.length
      ? await this.db.select<NoteRow>(
          notebookId ? `SELECT * FROM note WHERE notebook_id = $1` : `SELECT * FROM note`,
          notebookId ? [notebookId] : [],
        )
      : []
    const noteHits: SearchHit[] = notes
      .map((n) => ({ n, score: keywordScore(`${n.title ?? ''}\n${n.content}`, terms) }))
      .filter((x) => x.score > 0)
      .map(({ n, score }) => ({
        kind: 'note' as const,
        id: n.id,
        title: n.title ?? 'Note',
        snippet: snippet(n.content, query),
        score,
        notebookId: n.notebook_id,
      }))

    return [...bestBySource.values(), ...noteHits].slice(0, SEARCH_LIMIT * 2)
  }

  // Ask

  async ask(question: string, options: AskOptions = {}): Promise<AskResult> {
    const config = await this.deps.getAiConfig()
    if (!config.chat) throw new Error('Choose a chat model in Settings first.')
    const chat = (content: string) =>
      this.provider(config.chat!.provider, 'ask').chat({
        model: config.chat!.model,
        system: 'Follow the instructions in the message exactly.',
        messages: [{ role: 'user', content }],
        signal: options.signal,
      })

    const strategy = parseStrategy(await chat(strategyPrompt(question)))
    if (!strategy.searches.length) strategy.searches.push({ term: question, instructions: 'Answer the question.' })
    options.onStep?.({ kind: 'strategy', ...strategy })

    const documents = new Map<string, { id: string; title: string; notebookId?: string }>()
    const slots = new Semaphore(2)
    const answers = await Promise.all(
      strategy.searches.map((search) =>
        slots.run(async () => {
          const hits = await this.rankChunks(`${search.term}\n${search.instructions}`, config, options.notebookId, 8, options.signal)
          const notebookOf = new Map(
            (await this.db.select<{ id: string; notebook_id: string }>(`SELECT id, notebook_id FROM source`)).map((r) => [r.id, r.notebook_id]),
          )
          const results = hits.map((h) => ({ id: h.sourceId, title: h.title, text: h.text }))
          for (const h of hits) documents.set(h.sourceId, { id: h.sourceId, title: h.title, notebookId: notebookOf.get(h.sourceId) })
          const terms = keywordTerms(search.term)
          if (terms.length) {
            const notes = await this.db.select<NoteRow>(
              options.notebookId ? `SELECT * FROM note WHERE notebook_id = $1` : `SELECT * FROM note`,
              options.notebookId ? [options.notebookId] : [],
            )
            for (const n of notes
              .map((n) => ({ n, score: keywordScore(`${n.title ?? ''}\n${n.content}`, terms) }))
              .filter((x) => x.score > 0)
              .sort((a, b) => b.score - a.score)
              .slice(0, 3)) {
              results.push({ id: n.n.id, title: n.n.title ?? 'Note', text: n.n.content })
              documents.set(n.n.id, { id: n.n.id, title: n.n.title ?? 'Note', notebookId: n.n.notebook_id })
            }
          }
          const answer = results.length ? await chat(searchAnswerPrompt(question, search, results)) : 'Nothing in the notebooks matched this search.'
          options.onStep?.({ kind: 'answer', term: search.term, answer })
          return { term: search.term, answer }
        }),
      ),
    )

    const answer = await chat(finalAnswerPrompt(question, strategy.reasoning, answers))
    options.onStep?.({ kind: 'final', answer })
    return { answer, documents: [...documents.values()] }
  }

  // Studio

  async generateStudyDeck(notebookId: string, request: StudyRequest): Promise<StudyDeck> {
    const config = await this.deps.getAiConfig()
    const model = config.transformation ?? config.chat
    if (!model) throw new Error('Choose a chat model in Settings first.')
    const inNotebook = new Set((await this.listSources(notebookId)).map((s) => s.id))
    if (!request.sourceIds.length || request.sourceIds.some((id) => !inNotebook.has(id))) {
      throw new Error('Choose sources from this notebook.')
    }
    const sources = await Promise.all([...new Set(request.sourceIds)].map((id) => this.getSource(id)))
    if (sources.some((s) => !s.full_text.trim())) throw new Error('Wait for every selected source to finish processing.')
    const input = studyInput(sources.map((s) => ({ id: s.id, title: s.title, full_text: s.full_text })))
    if (input.length > STUDY_INPUT_LIMIT) throw new Error('Select fewer sources; together they are longer than 300,000 characters.')
    const raw = await this.provider(model.provider, 'study').chat({
      model: model.model,
      system: studyPrompt(request),
      messages: [{ role: 'user', content: input }],
    })
    const deck = parseDeck(raw, request)
    await this.createNote(notebookId, { title: deck.title, content: STUDY_PREFIX + JSON.stringify(deck), note_type: 'ai' })
    return deck
  }

  async listStudyDecks(notebookId: string): Promise<StoredDeck[]> {
    return decksFromNotes(await this.listNotes(notebookId))
  }

  // Transformations and insights

  /** Adds the server's default transformations the first time the app runs. */
  async seedTransformations(): Promise<void> {
    const [seeded] = await this.db.select<{ value: string }>(`SELECT value FROM app_setting WHERE key = 'transformations_seeded'`)
    if (seeded) return
    const now = this.now()
    for (const t of DEFAULT_TRANSFORMATIONS) {
      await this.db.execute(
        `INSERT INTO transformation (id, name, title, description, prompt, apply_default, created, updated)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7)`,
        [this.id('transformation'), t.name, t.title, t.description, t.prompt, t.applyDefault ? 1 : 0, now],
      )
    }
    await this.db.execute(`INSERT INTO app_setting (key, value) VALUES ('transformations_seeded', '1')`)
  }

  async listTransformations(): Promise<Transformation[]> {
    const rows = await this.db.select<TransformationRow>(`SELECT * FROM transformation ORDER BY apply_default DESC, title`)
    return rows.map(toTransformation)
  }

  private async getTransformation(id: string): Promise<Transformation> {
    const [row] = await this.db.select<TransformationRow>(`SELECT * FROM transformation WHERE id = $1`, [id])
    if (!row) throw new NotFoundError('Transformation')
    return toTransformation(row)
  }

  async saveTransformation(input: Omit<Transformation, 'id'> & { id?: string }): Promise<Transformation> {
    const now = this.now()
    const values = [input.name.trim(), input.title.trim() || input.name.trim(), input.description, input.prompt, input.applyDefault ? 1 : 0, now]
    if (input.id) {
      await this.db.execute(
        `UPDATE transformation SET name = $1, title = $2, description = $3, prompt = $4, apply_default = $5, updated = $6 WHERE id = $7`,
        [...values, input.id],
      )
      return this.getTransformation(input.id)
    }
    const id = this.id('transformation')
    await this.db.execute(
      `INSERT INTO transformation (id, name, title, description, prompt, apply_default, created, updated)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $7)`,
      [id, ...values],
    )
    return this.getTransformation(id)
  }

  async deleteTransformation(id: string): Promise<void> {
    await this.db.execute(`DELETE FROM transformation WHERE id = $1`, [id])
  }

  async getTransformationInstructions(): Promise<string> {
    const [row] = await this.db.select<{ value: string }>(`SELECT value FROM app_setting WHERE key = 'transformation_instructions'`)
    return row?.value ?? DEFAULT_TRANSFORMATION_INSTRUCTIONS
  }

  async setTransformationInstructions(text: string): Promise<void> {
    await this.db.execute(
      `INSERT INTO app_setting (key, value) VALUES ('transformation_instructions', $1)
       ON CONFLICT(key) DO UPDATE SET value = excluded.value`,
      [text],
    )
  }

  async runTransformation(transformationId: string, text: string, signal?: AbortSignal): Promise<string> {
    const config = await this.deps.getAiConfig()
    const model = config.transformation ?? config.chat
    if (!model) throw new Error('Choose a chat model in Settings to run transformations.')
    const t = await this.getTransformation(transformationId)
    const instructions = await this.getTransformationInstructions()
    const input = text.length > TRANSFORMATION_CHARS ? `${text.slice(0, TRANSFORMATION_CHARS)}\n\n[Text shortened to fit]` : text
    return this.provider(model.provider, 'transformation').chat({
      model: model.model,
      system: `${instructions}\n\n${t.prompt}`,
      messages: [{ role: 'user', content: input }],
      signal,
    })
  }

  async listInsights(sourceId: string): Promise<Insight[]> {
    const rows = await this.db.select<InsightRow>(`SELECT * FROM source_insight WHERE source_id = $1 ORDER BY created`, [sourceId])
    return rows.map(toInsight)
  }

  async createInsight(sourceId: string, transformationId: string): Promise<void> {
    const source = await this.getSource(sourceId)
    if (!source.full_text?.trim()) throw new Error('This source has no text to transform yet.')
    const t = await this.getTransformation(transformationId)
    const id = this.id('insight')
    const now = this.now()
    await this.db.execute(
      `INSERT INTO source_insight (id, source_id, insight_type, content, status, created, updated) VALUES ($1, $2, $3, '', 'running', $4, $4)`,
      [id, sourceId, t.title, now],
    )
    const controller = new AbortController()
    this.running.set(id, controller)
    const job = this.slots
      .run(async () => {
        const content = await this.runTransformation(transformationId, source.full_text, controller.signal)
        await this.db.execute(`UPDATE source_insight SET content = $1, status = 'completed', updated = $2 WHERE id = $3`, [
          content.trim(),
          this.now(),
          id,
        ])
      })
      .catch(async (error: unknown) => {
        if (controller.signal.aborted) return
        await this.db.execute(`UPDATE source_insight SET status = 'failed', error = $1, updated = $2 WHERE id = $3`, [
          error instanceof Error ? error.message : String(error),
          this.now(),
          id,
        ])
      })
      .finally(() => {
        this.running.delete(id)
        this.jobs.delete(job)
      })
    this.jobs.add(job)
  }

  async deleteInsight(id: string): Promise<void> {
    this.running.get(id)?.abort()
    await this.db.execute(`DELETE FROM source_insight WHERE id = $1`, [id])
  }

  async saveInsightAsNote(insightId: string, notebookId: string): Promise<Note> {
    const [row] = await this.db.select<InsightRow & { source_title: string | null }>(
      `SELECT i.*, s.title AS source_title FROM source_insight i JOIN source s ON s.id = i.source_id WHERE i.id = $1`,
      [insightId],
    )
    if (!row) throw new NotFoundError('Insight')
    return this.createNote(notebookId, {
      title: `${row.insight_type}: ${row.source_title ?? 'Source'}`,
      content: row.content,
      note_type: 'ai',
    })
  }

  async listModels() {
    return []
  }

  async createLiveSession(offerSdp: string, voice: LiveVoice, notebookId?: string): Promise<string> {
    if (!offerSdp.startsWith('v=0')) throw new Error('Invalid WebRTC offer')
    const config = await this.deps.getAiConfig()
    const openai = config.providers?.openai
    if (!openai?.apiKey) throw new Error('Live needs your OpenAI API key in Settings.')
    const backend = [config.chat, config.transformation].find((c) => c?.provider.kind === 'openai' && isLiveBackend(c.model))
    if (!backend) throw new Error('Live needs an OpenAI chat or transformation model, such as a GPT-6 model, in Settings.')
    const response = await this.deps.fetch('https://api.openai.com/v1/live/sessions', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${openai.apiKey}` },
      body: JSON.stringify(livePayload(offerSdp, voice, backend.model, notebookId ? await this.liveMaterial(notebookId, config) : undefined)),
    })
    if (!response.ok) throw new Error(`OpenAI Live did not start a session (HTTP ${response.status}). Check Live access and your credit balance.`)
    const data = (await response.json()) as { transport?: { sdp?: string } }
    if (!data.transport?.sdp) throw new Error('OpenAI returned an invalid Live session')
    return data.transport.sdp
  }

  async recordLiveTalk(seconds: number): Promise<void> {
    if (seconds > 0) await this.recordUsage({ feature: 'live_talk', model: 'gpt-live-1', audioSeconds: Math.round(seconds) })
  }

  /** What a live conversation knows: an overview for the voice model, the full chat context for the model it delegates to. */
  private async liveMaterial(notebookId: string, config: AiConfig): Promise<LiveMaterial> {
    const notebook = await this.getNotebook(notebookId)
    const query = [notebook.name, notebook.description].filter(Boolean).join('. ')
    const { sources, chunks } = await this.contextFor(notebookId, query, config)
    const notes = (await this.listNotes(notebookId)).map((n) => ({ title: n.title ?? 'Note', content: n.content ?? '' }))
    const full = buildChatPrompt({ notebook, sources, chunks, notes })

    const rows = await this.db.select<{ id: string; title: string | null; full_text: string }>(
      `SELECT id, title, full_text FROM source
        WHERE id IN (SELECT source_id FROM notebook_source WHERE notebook_id = $1) AND status = 'completed' ORDER BY created`,
      [notebookId],
    )
    const insights = await this.db.select<{ source_id: string; content: string }>(
      `SELECT source_id, content FROM source_insight WHERE status = 'completed' AND source_id IN
        (SELECT source_id FROM notebook_source WHERE notebook_id = $1) ORDER BY created`,
      [notebookId],
    )
    const each = Math.floor(LIVE_OVERVIEW_CHARS / Math.max(rows.length, 1))
    const overview = rows
      .map((r) => {
        const about = insights.find((i) => i.source_id === r.id)?.content ?? r.full_text
        return `Source: ${r.title ?? 'Untitled'}\n${clip(about, each)}`
      })
      .join('\n\n')
    const noteTitles = notes.filter((n) => n.content.trim()).map((n) => n.title)
    return {
      notebook: notebook.name,
      overview: [overview || 'The notebook has no processed sources yet.', noteTitles.length ? `Notes: ${noteTitles.join(', ')}` : ''].filter(Boolean).join('\n\n'),
      full,
    }
  }

  // Podcasts

  /** Adds the server's default speaker and episode profiles the first time the app runs. */
  async seedPodcastProfiles(): Promise<void> {
    const [seeded] = await this.db.select<{ value: string }>(`SELECT value FROM app_setting WHERE key = 'podcast_profiles_seeded'`)
    if (seeded) return
    const ids = new Map<string, string>()
    for (const sp of DEFAULT_SPEAKER_PROFILES) ids.set(sp.name, (await this.saveSpeakerProfile(sp)).id)
    for (const { speakerProfileName, ...ep } of DEFAULT_EPISODE_PROFILES) {
      const speakerProfileId = ids.get(speakerProfileName)
      if (speakerProfileId) await this.saveEpisodeProfile({ ...ep, speakerProfileId })
    }
    await this.db.execute(`INSERT INTO app_setting (key, value) VALUES ('podcast_profiles_seeded', '1')`)
  }

  async podcastProfiles(): Promise<{ episode: EpisodeProfile[]; speaker: SpeakerProfile[] }> {
    const speaker = (await this.db.select<SpeakerProfileRow>(`SELECT * FROM speaker_profile ORDER BY name`)).map(toSpeakerProfile)
    const episode = (await this.db.select<EpisodeProfileRow>(`SELECT * FROM episode_profile ORDER BY name`)).map(toEpisodeProfile)
    return { episode, speaker }
  }

  async saveSpeakerProfile(p: Omit<SpeakerProfile, 'id'> & { id?: string }): Promise<SpeakerProfile> {
    if (!p.speakers.length || p.speakers.length > 4) throw new Error('A speaker profile has one to four speakers.')
    const id = p.id ?? this.id('speaker_profile')
    await this.db.execute(
      `INSERT INTO speaker_profile (id, name, description, voice_kind, voice_model, speakers) VALUES ($1, $2, $3, $4, $5, $6)
       ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, voice_kind = excluded.voice_kind,
         voice_model = excluded.voice_model, speakers = excluded.speakers`,
      [id, p.name.trim(), p.description, p.voiceKind ?? 'openai', p.voiceModel, JSON.stringify(p.speakers)],
    )
    return toSpeakerProfile((await this.db.select<SpeakerProfileRow>(`SELECT * FROM speaker_profile WHERE id = $1`, [id]))[0])
  }

  async deleteSpeakerProfile(id: string): Promise<void> {
    const [used] = await this.db.select<{ name: string }>(`SELECT name FROM episode_profile WHERE speaker_profile_id = $1`, [id])
    if (used) throw new Error(`The episode profile “${used.name}” uses these speakers. Change it first.`)
    await this.db.execute(`DELETE FROM speaker_profile WHERE id = $1`, [id])
  }

  async saveEpisodeProfile(p: Omit<EpisodeProfile, 'id'> & { id?: string }): Promise<EpisodeProfile> {
    const id = p.id ?? this.id('episode_profile')
    await this.db.execute(
      `INSERT INTO episode_profile (id, name, description, speaker_profile_id, language, briefing, segments) VALUES ($1, $2, $3, $4, $5, $6, $7)
       ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description, speaker_profile_id = excluded.speaker_profile_id,
         language = excluded.language, briefing = excluded.briefing, segments = excluded.segments`,
      [id, p.name.trim(), p.description, p.speakerProfileId, p.language ?? null, p.briefing, Math.max(1, Math.min(12, Math.round(p.segments)))],
    )
    return toEpisodeProfile((await this.db.select<EpisodeProfileRow>(`SELECT * FROM episode_profile WHERE id = $1`, [id]))[0])
  }

  async deleteEpisodeProfile(id: string): Promise<void> {
    await this.db.execute(`DELETE FROM episode_profile WHERE id = $1`, [id])
  }

  async listEpisodes(notebookId: string): Promise<Episode[]> {
    const rows = await this.db.select<EpisodeRow>(`${EPISODE_SELECT} WHERE e.notebook_id = $1 ORDER BY e.created DESC`, [notebookId])
    return rows.map(toEpisode)
  }

  async getEpisode(id: string): Promise<Episode> {
    const rows = await this.db.select<EpisodeRow>(`${EPISODE_SELECT} WHERE e.id = $1`, [id])
    if (!rows[0]) throw new NotFoundError('Episode')
    return toEpisode(rows[0])
  }

  private async profilesFor(episodeProfileId: string, speakerProfileId?: string) {
    const { episode, speaker } = await this.podcastProfiles()
    const ep = episode.find((p) => p.id === episodeProfileId)
    if (!ep) throw new Error('Choose an episode profile.')
    const sp = speaker.find((p) => p.id === (speakerProfileId || ep.speakerProfileId))
    if (!sp) throw new Error('This episode profile’s speakers were deleted. Choose a speaker profile.')
    return { ep, sp }
  }

  async createEpisode(input: CreateEpisode): Promise<Episode> {
    const config = await this.deps.getAiConfig()
    if (!config.chat) throw new Error('Choose a chat model in Settings to write episodes.')
    if (!this.deps.audio) throw new Error('This build cannot store audio.')
    await this.getNotebook(input.notebookId)
    const { ep, sp } = await this.profilesFor(input.episodeProfile, input.speakerProfile)
    const id = this.id('episode')
    const now = this.now()
    await this.db.execute(
      `INSERT INTO podcast_episode (id, notebook_id, name, focus, format, length, status, voice_label, episode_profile_id, speaker_profile_id, effort, created, updated)
       VALUES ($1, $2, $3, $4, $5, $6, 'writing', $7, $8, $9, $11, $10, $10)`,
      [
        id,
        input.notebookId,
        input.name?.trim() || 'New episode',
        input.focus?.trim() || null,
        sp.speakers.length > 1 ? 'conversation' : 'monologue',
        String(ep.segments),
        `${sp.speakers.map((s) => s.name.split(' ')[0]).join(', ')}, ${VOICE_LABELS[sp.voiceKind ?? 'openai']}`,
        ep.id,
        sp.id,
        now,
        input.effort ?? 'standard',
      ],
    )
    this.startEpisode(id, { notebookId: input.notebookId, focus: input.focus, name: input.name, effort: input.effort }, ep, sp, config)
    return this.getEpisode(id)
  }

  async retryEpisode(id: string): Promise<void> {
    const [row] = await this.db.select<EpisodeRow & { focus: string | null; episode_profile_id: string | null; speaker_profile_id: string | null; effort: PodcastEffort | null }>(
      `SELECT * FROM podcast_episode WHERE id = $1`,
      [id],
    )
    if (!row) throw new NotFoundError('Episode')
    if (!row.episode_profile_id) throw new Error('This episode was made before profiles; create a new one instead.')
    const { ep, sp } = await this.profilesFor(row.episode_profile_id, row.speaker_profile_id ?? undefined)
    const config = await this.deps.getAiConfig()
    await this.setEpisode(id, { status: 'writing', error: null, done: 0, total: 0 })
    this.startEpisode(id, { notebookId: row.notebook_id, focus: row.focus ?? undefined, effort: row.effort ?? undefined }, ep, sp, config)
  }

  private startEpisode(
    id: string,
    input: { notebookId: string; focus?: string; name?: string; effort?: PodcastEffort },
    ep: EpisodeProfile,
    sp: SpeakerProfile,
    config: AiConfig,
  ): void {
    const controller = new AbortController()
    this.running.set(id, controller)
    const job = this.makeEpisode(id, input, ep, sp, config, controller.signal)
      .catch(async (error: unknown) => {
        if (controller.signal.aborted) return
        await this.setEpisode(id, { status: 'failed', error: error instanceof Error ? error.message : String(error) })
      })
      .finally(() => {
        this.running.delete(id)
        this.jobs.delete(job)
      })
    this.jobs.add(job)
  }

  async deleteEpisode(id: string): Promise<void> {
    this.running.get(id)?.abort()
    this.running.get(`check:${id}`)?.abort()
    const rows = await this.db.select<{ audio_ref: string | null }>(`SELECT audio_ref FROM podcast_episode WHERE id = $1`, [id])
    if (rows[0]?.audio_ref) await this.deps.audio?.remove(rows[0].audio_ref).catch(() => undefined)
    await this.db.execute(`DELETE FROM podcast_episode WHERE id = $1`, [id])
  }

  async checkEpisode(id: string): Promise<void> {
    const [row] = await this.db.select<{ notebook_id: string; focus: string | null; script: string | null }>(
      `SELECT notebook_id, focus, script FROM podcast_episode WHERE id = $1`,
      [id],
    )
    if (!row) throw new NotFoundError('Episode')
    if (!row.script) throw new Error('This episode has no script to check yet.')
    const config = await this.deps.getAiConfig()
    if (!config.chat) throw new Error('Choose a chat model in Settings to check episodes.')
    const [saved] = await this.db.select<{ model: string; material: string | null }>(`SELECT model, material FROM podcast_prompt WHERE episode_id = $1`, [id])
    // Check against the material the script was written from when it was kept.
    const material: MaterialPart[] = saved?.material
      ? JSON.parse(saved.material)
      : await this.podcastMaterial(row.notebook_id, row.focus || (await this.getNotebook(row.notebook_id)).name, config, new AbortController().signal)
    this.startCheck(id, JSON.parse(row.script) as PodcastScript, material, config, saved?.model)
  }

  private saveCheck(id: string, status: EpisodeCheck['status'], model: string, result: object | null, error: string | null) {
    return this.db.execute(
      `INSERT INTO podcast_check (episode_id, status, model, result, error, created) VALUES ($1, $2, $3, $4, $5, $6)
       ON CONFLICT(episode_id) DO UPDATE SET status = excluded.status, model = excluded.model, result = excluded.result, error = excluded.error, created = excluded.created`,
      [id, status, model, result && JSON.stringify(result), error, this.now()],
    )
  }

  /** Checks a script's claims, saves the result on the episode and returns it. */
  private async runCheck(
    id: string,
    script: PodcastScript,
    material: MaterialPart[],
    config: AiConfig,
    extra: { writer?: string; history?: EpisodeCheck['history'] },
    signal: AbortSignal,
  ): Promise<ScriptCheck> {
    const model = config.chat!.model
    await this.saveCheck(id, 'checking', model, extra, null)
    const raw = await this.provider(config.chat!.provider, 'podcast_check').chat({
      model,
      system: CHECK_SYSTEM,
      messages: [{ role: 'user', content: checkPrompt(script, materialText(material)) }],
      maxTokens: 16_000,
      signal,
    })
    const check = parseCheck(raw, script, material)
    if (!signal.aborted) await this.saveCheck(id, 'done', model, { ...extra, ...check }, null)
    return check
  }

  /** Runs the claim check in the background; a failed check never fails the episode. */
  private startCheck(id: string, script: PodcastScript, material: MaterialPart[], config: AiConfig, writer?: string): void {
    const key = `check:${id}`
    this.running.get(key)?.abort()
    const controller = new AbortController()
    this.running.set(key, controller)
    const job = this.runCheck(id, script, material, config, { writer }, controller.signal)
      .then(() => undefined)
      .catch(async (error: unknown) => {
        if (controller.signal.aborted) return
        const message = error instanceof Error ? error.message : String(error)
        await this.saveCheck(id, 'failed', config.chat!.model, writer ? { writer } : null, message).catch(() => undefined)
      })
      .finally(() => {
        if (this.running.get(key) === controller) this.running.delete(key)
        this.jobs.delete(job)
      })
    this.jobs.add(job)
  }

  async episodePrompt(id: string): Promise<EpisodePrompt | null> {
    const [row] = await this.db.select<EpisodePrompt>(`SELECT model, system, prompt FROM podcast_prompt WHERE episode_id = $1`, [id])
    return row ?? null
  }

  async episodeAudioUrl(id: string): Promise<string> {
    const rows = await this.db.select<{ audio_ref: string | null }>(`SELECT audio_ref FROM podcast_episode WHERE id = $1`, [id])
    if (!rows[0]?.audio_ref || !this.deps.audio) throw new Error('This episode has no audio yet.')
    return this.deps.audio.url(rows[0].audio_ref)
  }

  private async setEpisode(
    id: string,
    patch: Partial<{ status: EpisodeStatus; error: string | null; done: number; total: number; name: string; summary: string }>,
  ): Promise<void> {
    const sets: string[] = []
    const params: unknown[] = []
    const add = (column: string, value: unknown) => {
      params.push(value)
      sets.push(`${column} = $${params.length}`)
    }
    if (patch.status) add('status', patch.status)
    if (patch.error !== undefined) add('error', patch.error)
    if (patch.done !== undefined) add('progress_done', patch.done)
    if (patch.total !== undefined) add('progress_total', patch.total)
    if (patch.name) add('name', patch.name)
    if (patch.summary !== undefined) add('summary', patch.summary)
    add('updated', this.now())
    params.push(id)
    await this.db.execute(`UPDATE podcast_episode SET ${sets.join(', ')} WHERE id = $${params.length}`, params)
  }

  /** Script, then voices, then one saved recording with chapter and line timing. */
  private async makeEpisode(
    id: string,
    input: { notebookId: string; focus?: string; name?: string; effort?: PodcastEffort },
    ep: EpisodeProfile,
    sp: SpeakerProfile,
    config: AiConfig,
    signal: AbortSignal,
  ): Promise<void> {
    const kind = sp.voiceKind ?? 'openai'
    const hosts = sp.speakers.map((s) => s.name)
    const voices = sp.speakers.map((s) => s.voice)
    const notebook = await this.getNotebook(input.notebookId)

    const parts = await this.podcastMaterial(input.notebookId, input.focus || notebook.name, config, signal)
    const material = materialText(parts)
    if (!material.trim()) throw new Error('This notebook has no processed sources or notes to talk about yet.')
    const effort = input.effort ?? 'standard'
    const writer = this.provider(config.chat!.provider, 'podcast_writing')
    const model = config.chat!.model

    let notes: string | undefined
    if (effort === 'thorough') {
      await this.setEpisode(id, { status: 'researching' })
      notes = await this.provider(config.chat!.provider, 'podcast_research').chat({
        model,
        system: RESEARCH_SYSTEM,
        messages: [{ role: 'user', content: researchPrompt(material, input.focus) }],
        maxTokens: 8_000,
        signal,
      })
      await this.setEpisode(id, { status: 'writing' })
    }

    const system = 'You are an audio producer who writes accurate, engaging podcast scripts from research material. You reply with JSON only.'
    const prompt = scriptPrompt({
      notebookName: notebook.name,
      material,
      speakers: sp.speakers,
      briefing: ep.briefing,
      segments: ep.segments,
      language: ep.language,
      focus: input.focus,
      notes,
    })
    await this.db.execute(
      `INSERT INTO podcast_prompt (episode_id, model, system, prompt, material, created) VALUES ($1, $2, $3, $4, $5, $6)
       ON CONFLICT(episode_id) DO UPDATE SET model = excluded.model, system = excluded.system, prompt = excluded.prompt,
         material = excluded.material, created = excluded.created`,
      [id, model, system, prompt, JSON.stringify(parts), this.now()],
    )
    await this.db.execute(`DELETE FROM podcast_check WHERE episode_id = $1`, [id])
    const raw = await writer.chat({
      model: config.chat!.model,
      system,
      messages: [{ role: 'user', content: prompt }],
      maxTokens: ep.segments * WORDS_PER_SEGMENT * 4,
      signal,
    })
    let script = parseScript(raw, hosts.length)

    // Careful and thorough episodes are checked, and flagged lines fixed, before any voice is recorded.
    const rounds = effort === 'thorough' ? 2 : effort === 'careful' ? 1 : 0
    if (rounds) {
      const history: NonNullable<EpisodeCheck['history']> = []
      try {
        await this.setEpisode(id, { status: 'checking' })
        let check = await this.runCheck(id, script, parts, config, { writer: model, history }, signal)
        for (let round = 0; round < rounds && check.overstated + check.unsupported > 0; round++) {
          await this.setEpisode(id, { status: 'revising' })
          const revised = await writer.chat({
            model,
            system: REVISE_SYSTEM,
            messages: [{ role: 'user', content: revisePrompt(script, check.claims.filter((c) => c.verdict !== 'supported'), material) }],
            maxTokens: ep.segments * WORDS_PER_SEGMENT * 4,
            signal,
          })
          let next: PodcastScript
          try {
            next = parseScript(revised, hosts.length)
          } catch {
            break // Keep the checked script rather than lose the episode to a bad revision.
          }
          history.push({ supported: check.supported, overstated: check.overstated, unsupported: check.unsupported })
          await this.setEpisode(id, { status: 'checking' })
          const nextCheck = await this.runCheck(id, next, parts, config, { writer: model, history }, signal)
          script = next
          check = nextCheck
        }
      } catch (error) {
        if (signal.aborted) throw error
        // A failed check is shown on the episode; the script is still voiced.
        const message = error instanceof Error ? error.message : String(error)
        await this.saveCheck(id, 'failed', model, { writer: model, history }, message)
      }
    }
    await this.db.execute(`UPDATE podcast_episode SET script = $1 WHERE id = $2`, [JSON.stringify(script), id])
    if (effort === 'standard') this.startCheck(id, script, parts, config, model)
    const lineCount = script.chapters.reduce((n, c) => n + c.lines.length, 0)
    await this.setEpisode(id, {
      status: 'voicing',
      name: input.name?.trim() || script.title,
      summary: script.summary,
      done: 0,
      total: lineCount,
    })

    const voice: VoiceConfig = { kind, model: sp.voiceModel, voices, hosts, apiKey: config.voiceKeys?.[kind] }
    const provider = this.deps.createVoiceProvider?.(voice) ?? createVoiceProvider(voice, this.deps.fetch, this.deps.openSocket)
    const result = await produce({
      script,
      hosts,
      voices,
      provider,
      signal,
      onProgress: (done, total) => this.setEpisode(id, { done, total }),
    })
    if (signal.aborted) return
    await this.recordUsage({ feature: 'podcast_audio', model: sp.voiceModel, audioSeconds: Math.round(result.duration) })

    await this.setEpisode(id, { status: 'saving' })
    const [old] = await this.db.select<{ audio_ref: string | null }>(`SELECT audio_ref FROM podcast_episode WHERE id = $1`, [id])
    if (old?.audio_ref) await this.deps.audio!.remove(old.audio_ref).catch(() => undefined)
    const ref = await this.deps.audio!.save(id, encodeWav(result.pcm))
    await this.db.execute(
      `UPDATE podcast_episode SET status = 'completed', error = NULL, transcript = $1, chapters = $2, exact_timing = $3,
              duration = $4, audio_ref = $5, updated = $6 WHERE id = $7`,
      [
        JSON.stringify(result.lines.map(({ speaker, text, start, end }) => ({ speaker, text, start, end }))),
        JSON.stringify(result.chapters),
        result.exactTiming ? 1 : 0,
        result.duration,
        ref,
        this.now(),
        id,
      ],
    )
  }

  /** Source text for a script: everything for small notebooks, the most relevant passages otherwise. */
  /**
   * What an episode is written and checked from: each source's own text (not its insights, so claims
   * can be traced to the paper), the most relevant passages of long sources, and the notes.
   */
  private async podcastMaterial(notebookId: string, query: string, config: AiConfig, signal: AbortSignal): Promise<MaterialPart[]> {
    const ids = await this.db.select<{ source_id: string }>(`SELECT source_id FROM notebook_source WHERE notebook_id = $1`, [notebookId])
    const selection: ContextSelection = { sources: Object.fromEntries(ids.map((r) => [r.source_id, 'full' as const])) }
    const { sources, chunks } = await this.contextFor(notebookId, query, config, signal, selection)
    const notes = await this.listNotes(notebookId)
    return [
      ...sources.map((s) => ({ title: s.title, text: s.text })),
      ...chunks.map((c) => ({ title: c.title, text: c.text, excerpt: true })),
      ...notes.filter((n) => n.content?.trim()).map((n) => ({ title: n.title ?? 'Untitled', text: n.content!, note: true })),
    ]
  }

}

/** Notebooks up to this many characters of source text are sent to the model whole (about 40k tokens). */
export const FULL_CONTEXT_CHARS = 160_000
/** The live voice model gets a short overview, about 4k tokens; details go through the model it delegates to. */
const LIVE_OVERVIEW_CHARS = 16_000

function clip(text: string, max: number): string {
  const t = text.replace(/\s+/g, ' ').trim()
  return t.length <= max ? t : `${t.slice(0, max).replace(/\s\S*$/, '')}…`
}
const RETRIEVAL_CHUNKS = 40
/** Source text sent to a transformation, about 100k tokens. */
const TRANSFORMATION_CHARS = 400_000

function keywordTerms(query: string): string[] {
  return [...new Set(query.toLowerCase().match(/[\p{L}\p{N}]{3,}/gu) ?? [])]
}

function keywordScore(text: string, terms: string[]): number {
  const lower = text.toLowerCase()
  let score = 0
  for (const term of terms) if (lower.includes(term)) score += 1
  return score / terms.length
}

function snippet(text: string, query: string, size = 220): string {
  const terms = keywordTerms(query)
  const lower = text.toLowerCase()
  const at = terms.map((t) => lower.indexOf(t)).filter((i) => i >= 0).sort((a, b) => a - b)[0] ?? 0
  const start = Math.max(0, at - 60)
  const body = text.slice(start, start + size).replace(/\s+/g, ' ').trim()
  return `${start > 0 ? '…' : ''}${body}${start + size < text.length ? '…' : ''}`
}

function toNotebook(row: NotebookRow): Notebook {
  return {
    id: row.id,
    name: row.name,
    description: row.description,
    archived: Boolean(row.archived),
    created: row.created,
    updated: row.updated,
    source_count: Number(row.source_count ?? 0),
    note_count: Number(row.note_count ?? 0),
  }
}

function toSourceSummary(row: SourceRow): SourceSummary {
  return {
    id: row.id,
    title: row.title,
    topics: [],
    asset: row.url ? { url: row.url } : row.file_name ? { file_path: row.file_name } : null,
    file_available: !!row.file_ref,
    embedded: Number(row.embedded_count) > 0,
    embedded_chunks: Number(row.embedded_count ?? 0),
    insights_count: Number(row.insights_count ?? 0),
    created: row.created,
    updated: row.updated,
    status: row.status,
    processing_info: row.error ? { error: row.error } : undefined,
  }
}

function toNote(row: NoteRow): Note {
  return {
    id: row.id,
    title: row.title,
    content: row.content,
    note_type: row.note_type,
    created: row.created,
    updated: row.updated,
  }
}

interface EpisodeRow {
  id: string
  notebook_id: string
  name: string
  summary: string | null
  status: EpisodeStatus
  progress_done: number
  progress_total: number
  error: string | null
  transcript: string | null
  chapters: string | null
  exact_timing: number
  duration: number | null
  voice_label: string | null
  created: string
  effort: PodcastEffort | null
  check_status: EpisodeCheck['status'] | null
  check_model: string | null
  check_result: string | null
  check_error: string | null
}

/** Episodes with their claim check, if any. */
const EPISODE_SELECT = `SELECT e.*, c.status AS check_status, c.model AS check_model, c.result AS check_result, c.error AS check_error
  FROM podcast_episode e LEFT JOIN podcast_check c ON c.episode_id = e.id`


function toEpisode(row: EpisodeRow): Episode {
  return {
    id: row.id,
    notebookId: row.notebook_id,
    name: row.name,
    summary: row.summary ?? undefined,
    status: row.status,
    progress: row.progress_total ? { done: Number(row.progress_done), total: Number(row.progress_total) } : undefined,
    error: row.error ?? undefined,
    created: row.created,
    duration: row.duration ?? undefined,
    chapters: row.chapters ? JSON.parse(row.chapters) : [],
    transcript: row.transcript ? JSON.parse(row.transcript) : [],
    exactTiming: Boolean(row.exact_timing),
    voiceLabel: row.voice_label ?? undefined,
    effort: row.effort ?? undefined,
    check: row.check_status
      ? {
          ...(row.check_result ? JSON.parse(row.check_result) : {}),
          status: row.check_status,
          model: row.check_model ?? '',
          error: row.check_error ?? undefined,
        }
      : undefined,
  }
}


interface TransformationRow {
  id: string
  name: string
  title: string
  description: string
  prompt: string
  apply_default: number
}

function toTransformation(row: TransformationRow): Transformation {
  return {
    id: row.id,
    name: row.name,
    title: row.title,
    description: row.description,
    prompt: row.prompt,
    applyDefault: Boolean(row.apply_default),
  }
}

interface InsightRow {
  id: string
  source_id: string
  insight_type: string
  content: string
  status: 'running' | 'completed' | 'failed'
  error: string | null
  created: string
}

function toInsight(row: InsightRow): Insight {
  return {
    id: row.id,
    sourceId: row.source_id,
    type: row.insight_type,
    content: row.content,
    created: row.created,
    status: row.status,
    error: row.error ?? undefined,
  }
}

interface SpeakerProfileRow {
  id: string
  name: string
  description: string
  voice_kind: VoiceKind
  voice_model: string
  speakers: string
}

function toSpeakerProfile(row: SpeakerProfileRow): SpeakerProfile {
  return {
    id: row.id,
    name: row.name,
    description: row.description,
    voiceKind: row.voice_kind,
    voiceModel: row.voice_model,
    speakers: JSON.parse(row.speakers),
  }
}

interface EpisodeProfileRow {
  id: string
  name: string
  description: string
  speaker_profile_id: string
  language: string | null
  briefing: string
  segments: number
}

function toEpisodeProfile(row: EpisodeProfileRow): EpisodeProfile {
  return {
    id: row.id,
    name: row.name,
    description: row.description,
    speakerProfileId: row.speaker_profile_id,
    language: row.language,
    briefing: row.briefing,
    segments: Number(row.segments),
  }
}

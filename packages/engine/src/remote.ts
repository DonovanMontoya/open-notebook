import {
  createClient,
  defaultContextConfig,
  type ClientConfig,
  type OpenNotebookClient,
  type SourceSummary,
} from '@open-notebook/api-client'

import type { PodcastEpisode } from '@open-notebook/api-client'

import type { AskOptions, AskResult } from './ask.ts'
import type { EpisodeProfile, SpeakerProfile } from './podcast/profiles.ts'
import { decksFromNotes, type StoredDeck, type StudyDeck, type StudyRequest } from './studio.ts'

import type {
  Backend,
  ContextSelection,
  CreateEpisode,
  Episode,
  Insight,
  Transformation,
  EpisodeStatus,
  LiveVoice,
  SearchHit,
  SendChatArgs,
  SendSourceChatArgs,
  SendChatResult,
  SourceInput,
} from './backend.ts'
import { titleFrom } from './text.ts'

/** Connected mode: the same Backend surface, served by an Open Notebook server. */
export class RemoteBackend implements Backend {
  readonly kind = 'remote' as const
  readonly streamsChat = false
  readonly podcastMode = 'server' as const
  readonly client: OpenNotebookClient

  constructor(config: ClientConfig) {
    this.client = createClient(config)
  }

  listNotebooks() {
    return this.client.notebooks.list()
  }

  getNotebook(id: string) {
    return this.client.notebooks.get(id)
  }

  createNotebook(name: string, description?: string) {
    return this.client.notebooks.create({ name, description })
  }

  updateNotebook(id: string, data: { name?: string; description?: string; archived?: boolean }) {
    return this.client.notebooks.update(id, data)
  }

  async deleteNotebook(id: string) {
    await this.client.notebooks.delete(id)
  }

  listSources(notebookId: string) {
    return this.client.sources.list(notebookId)
  }

  getSource(id: string) {
    return this.client.sources.get(id)
  }

  async addSource(notebookId: string, input: SourceInput): Promise<SourceSummary> {
    // Files go to the server as they are, so it can read PDFs, Office files, audio and video itself.
    if (input.type === 'document' && input.file && input.fileName) {
      return this.client.sources.upload(notebookId, input.file, input.fileName, { title: input.title, transformations: input.transformations })
    }
    const source =
      input.type === 'document'
        ? ({ type: 'text', content: input.content, title: input.title, transformations: input.transformations } as const)
        : input
    return this.client.sources.create(notebookId, source)
  }

  listAllSources() {
    return this.client.sources.listAll()
  }

  async linkSource(sourceId: string, notebookId: string) {
    await this.client.sources.link(sourceId, notebookId)
  }

  async unlinkSource(sourceId: string, notebookId: string) {
    await this.client.sources.unlink(sourceId, notebookId)
  }

  updateSource(id: string, data: { title?: string }) {
    return this.client.sources.update(id, data)
  }

  async downloadSource(id: string) {
    try {
      const file = await this.client.sources.download(id)
      return { data: file.data, name: file.name ?? 'source', type: file.type }
    } catch (error) {
      if (error instanceof Error && 'status' in error && (error as { status: number }).status === 404) return null
      throw error
    }
  }

  async retrySource(id: string) {
    await this.client.sources.retry(id)
  }

  async deleteSource(id: string) {
    await this.client.sources.delete(id)
  }

  listNotes(notebookId: string) {
    return this.client.notes.list(notebookId)
  }

  createNote(notebookId: string, data: { title?: string; content: string; note_type?: 'human' | 'ai' }) {
    return this.client.notes.create({ ...data, notebook_id: notebookId })
  }

  updateNote(id: string, data: { title?: string; content?: string }) {
    return this.client.notes.update(id, data)
  }

  async deleteNote(id: string) {
    await this.client.notes.delete(id)
  }

  listChatSessions(notebookId: string) {
    return this.client.chat.listSessions(notebookId)
  }

  getChatSession(id: string) {
    return this.client.chat.getSession(id)
  }

  async deleteChatSession(id: string) {
    await this.client.chat.deleteSession(id)
  }

  /** The server answers in one response; the full reply is passed to onText once. */
  async sendChatMessage(args: SendChatArgs): Promise<SendChatResult> {
    const sessionId =
      args.sessionId ?? (await this.client.chat.createSession(args.notebookId, titleFrom(args.message, 40))).id
    const [sources, notes] = await Promise.all([
      this.client.sources.list(args.notebookId),
      this.client.notes.list(args.notebookId),
    ])
    const { context } = await this.client.chat.buildContext(args.notebookId, serverContext(sources, notes, args.context))
    const result = await this.client.chat.send(sessionId, args.message, context, args.modelOverride)
    const reply = [...result.messages].reverse().find((m) => m.type === 'ai')
    if (reply) args.onText?.(reply.content)
    const session = await this.client.chat.getSession(sessionId)
    return { session, messages: session.messages }
  }

  async search(query: string, notebookId?: string): Promise<SearchHit[]> {
    const { results } = await this.client.search(query, { notebookId })
    return results.map((r) => ({
      kind: r.parent_id?.startsWith('note:') || r.id.startsWith('note:') ? 'note' : 'source',
      id: r.parent_id || r.id,
      title: r.title,
      snippet: r.matches?.[0] ?? '',
      score: r.similarity ?? r.relevance ?? 0,
    }))
  }

  async renameChatSession(id: string, title: string) {
    await this.client.chat.renameSession(id, title)
  }

  listSourceChatSessions(sourceId: string) {
    return this.client.sourceChat.listSessions(sourceId)
  }

  getSourceChatSession(sourceId: string, sessionId: string) {
    return this.client.sourceChat.getSession(sourceId, sessionId)
  }

  async sendSourceChatMessage(args: SendSourceChatArgs): Promise<SendChatResult> {
    const sessionId = args.sessionId ?? (await this.client.sourceChat.createSession(args.sourceId, titleFrom(args.message, 40))).id
    const reply = await this.client.sourceChat.send(args.sourceId, sessionId, args.message, args.modelOverride)
    if (reply) args.onText?.(reply)
    const session = await this.client.sourceChat.getSession(args.sourceId, sessionId)
    return { session, messages: session.messages }
  }

  async deleteSourceChatSession(sourceId: string, sessionId: string) {
    await this.client.sourceChat.deleteSession(sourceId, sessionId)
  }

  /** The server's Ask, with its default chat model for every step. */
  async ask(question: string, options: AskOptions = {}): Promise<AskResult> {
    const defaults = await this.client.models.defaults()
    const model = defaults.default_chat_model
    if (!model) throw new Error('The server has no default chat model. Set one in its Models settings.')
    const { answer } = await this.client.search.ask({
      question,
      strategy_model: model,
      answer_model: model,
      final_answer_model: model,
      notebook_id: options.notebookId,
    })
    options.onStep?.({ kind: 'final', answer })
    const ids = [...new Set([...answer.matchAll(/\[((?:source|note):[^\]\s]+)\]/g)].map((m) => m[1]))].slice(0, 12)
    const documents = await Promise.all(
      ids.map(async (id) => {
        try {
          if (id.startsWith('note:')) {
            const note = await this.client.notes.get(id)
            return { id, title: note.title ?? 'Note' }
          }
          const source = await this.client.sources.get(id)
          return { id, title: source.title ?? 'Source', notebookId: source.notebooks?.[0] }
        } catch {
          return { id, title: id }
        }
      }),
    )
    return { answer, documents }
  }

  generateStudyDeck(notebookId: string, request: StudyRequest): Promise<StudyDeck> {
    return this.client.studio.generate(notebookId, {
      kind: request.kind,
      source_ids: request.sourceIds,
      count: request.count,
      difficulty: request.difficulty,
      language: request.language ?? 'en',
      instructions: request.instructions ?? '',
    }) as Promise<StudyDeck>
  }

  /** The server keeps decks as notes too; reading them from notes keeps their note ids. */
  async listStudyDecks(notebookId: string): Promise<StoredDeck[]> {
    return decksFromNotes(await this.client.notes.list(notebookId))
  }

  // Transformations and insights

  async listTransformations(): Promise<Transformation[]> {
    return (await this.client.transformations.list()).map(toTransformation)
  }

  async saveTransformation(input: Omit<Transformation, 'id'> & { id?: string }): Promise<Transformation> {
    const body = {
      name: input.name,
      title: input.title,
      description: input.description,
      prompt: input.prompt,
      apply_default: input.applyDefault,
    }
    const saved = input.id
      ? await this.client.transformations.update(input.id, body)
      : await this.client.transformations.create(body)
    return toTransformation(saved)
  }

  async deleteTransformation(id: string): Promise<void> {
    await this.client.transformations.delete(id)
  }

  async runTransformation(transformationId: string, text: string): Promise<string> {
    return (await this.client.transformations.execute(transformationId, text)).output
  }

  async getTransformationInstructions(): Promise<string> {
    return (await this.client.transformations.getInstructions()).transformation_instructions
  }

  async setTransformationInstructions(text: string): Promise<void> {
    await this.client.transformations.setInstructions(text)
  }

  async listInsights(sourceId: string): Promise<Insight[]> {
    return (await this.client.sources.insights(sourceId)).map((i) => ({
      id: i.id,
      sourceId: i.source_id,
      type: i.insight_type,
      content: i.content,
      created: i.created ?? undefined,
      status: 'completed' as const,
    }))
  }

  async createInsight(sourceId: string, transformationId: string): Promise<void> {
    await this.client.insights.create(sourceId, transformationId)
  }

  async deleteInsight(id: string): Promise<void> {
    await this.client.insights.delete(id)
  }

  saveInsightAsNote(insightId: string, notebookId: string) {
    return this.client.insights.saveAsNote(insightId, notebookId)
  }

  // Podcasts: the server keeps one episode list for all notebooks.

  async listEpisodes(): Promise<Episode[]> {
    return (await this.client.podcasts.list()).map(toEpisode)
  }

  async getEpisode(id: string): Promise<Episode> {
    return toEpisode(await this.client.podcasts.get(id))
  }

  async createEpisode(input: CreateEpisode): Promise<Episode> {
    // The server generates by profile name.
    const [episodes, speakers] = await Promise.all([this.client.podcasts.episodeProfiles(), this.client.podcasts.speakerProfiles()])
    const ep = episodes.find((p) => p.id === input.episodeProfile)
    if (!ep) throw new Error('Choose an episode profile.')
    const sp = speakers.find((p) => p.id === (input.speakerProfile || ep.speaker_config))
    if (!sp) throw new Error('Choose a speaker profile.')
    const name = input.name?.trim() || `${(await this.client.notebooks.get(input.notebookId)).name} episode`
    const job = await this.client.podcasts.generate({
      episode_profile: ep.name,
      speaker_profile: sp.name,
      episode_name: name,
      notebook_id: input.notebookId,
      briefing_suffix: input.focus?.trim() || null,
    })
    // The episode record appears in the list once the server picks up the job.
    return { id: `job:${job.job_id}`, name, status: 'queued', created: new Date().toISOString(), chapters: [], transcript: [], exactTiming: false }
  }

  async createLiveSession(offerSdp: string, voice: LiveVoice) {
    return (await this.client.live(offerSdp, voice)).transport.sdp
  }

  async usage(days: number) {
    const u = await this.client.usage(days)
    return {
      days: u.days,
      calls: u.calls,
      estimatedUsd: u.estimated_usd,
      estimated: false,
      groups: u.groups.map((g) => ({
        model: g.model,
        feature: g.feature,
        calls: g.calls,
        inputTokens: g.input_tokens,
        outputTokens: g.output_tokens,
        audioSeconds: g.audio_seconds,
        estimatedUsd: g.estimated_usd,
      })),
    }
  }

  async rebuildIndex() {
    await this.client.embeddings.rebuild('all')
    return -1
  }

  async listModels() {
    return (await this.client.models.list()).map((m) => ({ id: m.id, name: m.name, provider: m.provider, type: m.type }))
  }

  async retryEpisode(id: string): Promise<void> {
    await this.client.podcasts.retry(id)
  }

  async deleteEpisode(id: string): Promise<void> {
    await this.client.podcasts.delete(id)
  }

  episodeAudioUrl(id: string): Promise<string> {
    return this.client.podcasts.audioUrl(id)
  }

  async podcastProfiles(): Promise<{ episode: EpisodeProfile[]; speaker: SpeakerProfile[] }> {
    const [episode, speaker] = await Promise.all([this.client.podcasts.episodeProfiles(), this.client.podcasts.speakerProfiles()])
    return {
      episode: episode.map((p) => ({
        id: p.id,
        name: p.name,
        description: p.description,
        speakerProfileId: p.speaker_config ?? '',
        language: p.language,
        briefing: p.default_briefing,
        segments: p.num_segments,
      })),
      speaker: speaker.map((p) => ({
        id: p.id,
        name: p.name,
        description: p.description,
        voiceModel: p.voice_model ?? '',
        speakers: p.speakers.map((x) => ({ name: x.name, voice: x.voice_id, backstory: x.backstory ?? '', personality: x.personality ?? '' })),
      })),
    }
  }

  async saveEpisodeProfile(p: Omit<EpisodeProfile, 'id'> & { id?: string }): Promise<EpisodeProfile> {
    // Keep the server's outline and transcript models when editing.
    const existing = p.id ? (await this.client.podcasts.episodeProfiles()).find((e) => e.id === p.id) : undefined
    const saved = await this.client.podcasts.saveEpisodeProfile(
      {
        name: p.name,
        description: p.description,
        speaker_config: p.speakerProfileId,
        outline_llm: existing?.outline_llm,
        transcript_llm: existing?.transcript_llm,
        language: p.language ?? null,
        default_briefing: p.briefing,
        num_segments: p.segments,
        max_tokens: existing?.max_tokens,
      },
      p.id,
    )
    return { ...p, id: saved.id }
  }

  async deleteEpisodeProfile(id: string): Promise<void> {
    await this.client.podcasts.deleteEpisodeProfile(id)
  }

  async saveSpeakerProfile(p: Omit<SpeakerProfile, 'id'> & { id?: string }): Promise<SpeakerProfile> {
    const saved = await this.client.podcasts.saveSpeakerProfile(
      {
        name: p.name,
        description: p.description,
        voice_model: p.voiceModel || null,
        speakers: p.speakers.map((x) => ({ name: x.name, voice_id: x.voice, backstory: x.backstory, personality: x.personality })),
      },
      p.id,
    )
    return { ...p, id: saved.id }
  }

  async deleteSpeakerProfile(id: string): Promise<void> {
    await this.client.podcasts.deleteSpeakerProfile(id)
  }

}

const SERVER_STATUS: Record<string, EpisodeStatus> = {
  completed: 'completed',
  failed: 'failed',
  error: 'failed',
  running: 'voicing',
  processing: 'voicing',
  pending: 'queued',
  submitted: 'queued',
}

function toEpisode(e: PodcastEpisode): Episode {
  const status = SERVER_STATUS[e.job_status ?? ''] ?? (e.audio_file ? 'completed' : 'queued')
  return {
    id: e.id,
    name: e.name,
    status,
    error: e.error_message || undefined,
    created: e.created ?? '',
    chapters: [],
    transcript: (e.transcript?.transcript ?? [])
      .filter((l) => l.dialogue?.trim())
      .map((l) => ({ speaker: l.speaker ?? '', text: l.dialogue!.trim() })),
    exactTiming: false,
    voiceLabel: e.speaker_profile?.name?.replace(/[_-]+/g, ' ').replace(/^\w/, (c) => c.toUpperCase()),
  }
}

function toTransformation(t: { id: string; name: string; title: string; description: string; prompt: string; apply_default: boolean }): Transformation {
  return { id: t.id, name: t.name, title: t.title, description: t.description, prompt: t.prompt, applyDefault: t.apply_default }
}

/** The server's context config: the app's choices, falling back to its defaults. */
function serverContext(
  sources: Array<{ id: string; insights_count: number }>,
  notes: Array<{ id: string }>,
  selection?: ContextSelection,
) {
  const config = defaultContextConfig(sources, notes)
  for (const [id, mode] of Object.entries(selection?.sources ?? {})) {
    if (id in config.sources) config.sources[id] = mode === 'full' ? 'full content' : mode === 'insights' ? 'insights' : 'not in'
  }
  for (const [id, on] of Object.entries(selection?.notes ?? {})) {
    if (id in config.notes) config.notes[id] = on ? 'full content' : 'not in'
  }
  return config
}

import type {
  AuthStatus,
  BuildContextResponse,
  ChatContext,
  ChatMessage,
  ChatSession,
  ChatSessionWithMessages,
  ContextMode,
  CreateNoteRequest,
  CreateNotebookRequest,
  NewSource,
  Note,
  Notebook,
  PodcastEpisode,
  PodcastGenerationRequest,
  PodcastGenerationResponse,
  ServerCredential,
  ServerEpisodeProfile,
  ServerJob,
  ServerUsage,
  ServerSpeakerProfile,
  SearchResponse,
  SourceDetail,
  SourceInsight,
  SourceSummary,
  TransformationInput,
  TransformationRecord,
  UpdateNoteRequest,
  UpdateNotebookRequest,
} from './types.ts'

export interface ClientConfig {
  /** Server origin, e.g. https://host.example.ts.net:8502 (no trailing /api). */
  baseUrl: string
  /** Value of OPEN_NOTEBOOK_PASSWORD when the server has auth enabled. */
  password?: string
  /** Per-request timeout. Chat and source processing can be slow on local models. */
  timeoutMs?: number
  fetch?: typeof fetch
}

export class ApiError extends Error {
  readonly status: number
  readonly detail: unknown

  constructor(status: number, message: string, detail?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.detail = detail
  }
}

type Query = Record<string, string | number | boolean | undefined>

const DEFAULT_TIMEOUT_MS = 600_000

/** Strips trailing slashes and a trailing /api so either form of the URL works. */
export function normalizeBaseUrl(raw: string): string {
  let url = raw.trim().replace(/\/+$/, '')
  if (url.toLowerCase().endsWith('/api')) url = url.slice(0, -4)
  if (url && !/^https?:\/\//i.test(url)) url = `https://${url}`
  return url
}

function errorMessage(status: number, body: unknown): string {
  if (body && typeof body === 'object' && 'detail' in body) {
    const detail = (body as { detail: unknown }).detail
    if (typeof detail === 'string') return detail
    if (Array.isArray(detail) && detail[0]?.msg) return String(detail[0].msg)
  }
  if (typeof body === 'string' && body.trim()) return body.trim().slice(0, 200)
  return `Request failed with status ${status}`
}

export function createClient(config: ClientConfig) {
  const baseUrl = normalizeBaseUrl(config.baseUrl)
  const doFetch = config.fetch ?? fetch
  const timeoutMs = config.timeoutMs ?? DEFAULT_TIMEOUT_MS

  async function request<T>(
    method: string,
    path: string,
    options: { query?: Query; body?: unknown; timeoutMs?: number; raw?: boolean; binary?: boolean } = {},
  ): Promise<T> {
    let url = `${baseUrl}/api${path}`
    if (options.query) {
      const params = new URLSearchParams()
      for (const [key, value] of Object.entries(options.query)) {
        if (value !== undefined) params.append(key, String(value))
      }
      const qs = params.toString()
      if (qs) url += `?${qs}`
    }

    const headers: Record<string, string> = { Accept: 'application/json' }
    const isForm = typeof FormData !== 'undefined' && options.body instanceof FormData
    if (options.body !== undefined && !isForm) headers['Content-Type'] = 'application/json'
    if (config.password) headers.Authorization = `Bearer ${config.password}`

    const controller = new AbortController()
    const limit = options.timeoutMs ?? timeoutMs
    const timer = limit > 0 ? setTimeout(() => controller.abort(), limit) : undefined

    let response: Response
    try {
      response = await doFetch(url, {
        method,
        headers,
        body: options.body === undefined ? undefined : isForm ? (options.body as FormData) : JSON.stringify(options.body),
        signal: controller.signal,
      })
    } catch (error) {
      if (controller.signal.aborted) throw new ApiError(0, 'The server took too long to respond')
      throw new ApiError(0, `Could not reach ${baseUrl}`, error)
    } finally {
      if (timer) clearTimeout(timer)
    }

    if (options.binary && response.ok) {
      const disposition = response.headers.get('content-disposition') ?? ''
      const name = /filename\*?=(?:UTF-8'')?"?([^";]+)"?/i.exec(disposition)?.[1]
      return { data: await response.arrayBuffer(), name: name ? decodeURIComponent(name) : undefined, type: response.headers.get('content-type') ?? '' } as T
    }
    const text = await response.text()
    if (options.raw && response.ok) return text as T
    let body: unknown = undefined
    if (text) {
      try {
        body = JSON.parse(text)
      } catch {
        body = text
      }
    }
    if (!response.ok) throw new ApiError(response.status, errorMessage(response.status, body), body)
    return body as T
  }

  return {
    baseUrl,

    /** Absolute URL for a server-relative path such as an audio-access URL. */
    resolveUrl(path: string): string {
      if (/^https?:\/\//i.test(path)) return path
      return `${baseUrl}${path.startsWith('/') ? '' : '/'}${path}`
    },

    authStatus: () => request<AuthStatus>('GET', '/auth/status', { timeoutMs: 15_000 }),

    notebooks: {
      list: (params: { archived?: boolean } = {}) =>
        request<Notebook[]>('GET', '/notebooks', { query: { ...params, order_by: 'updated desc' } }),
      get: (id: string) => request<Notebook>('GET', `/notebooks/${encodeURIComponent(id)}`),
      create: (data: CreateNotebookRequest) => request<Notebook>('POST', '/notebooks', { body: data }),
      update: (id: string, data: UpdateNotebookRequest) =>
        request<Notebook>('PUT', `/notebooks/${encodeURIComponent(id)}`, { body: data }),
      /** Deletes the notebook, its notes, and sources that belong to no other notebook. */
      delete: (id: string) =>
        request<unknown>('DELETE', `/notebooks/${encodeURIComponent(id)}`, {
          query: { delete_exclusive_sources: true },
        }),
    },

    sources: {
      list: (notebookId: string) =>
        request<SourceSummary[]>('GET', '/sources', {
          query: { notebook_id: notebookId, sort_by: 'updated', sort_order: 'desc', limit: 100 },
        }),
      get: (id: string) => request<SourceDetail>('GET', `/sources/${encodeURIComponent(id)}`),
      insights: (id: string) =>
        request<SourceInsight[]>('GET', `/sources/${encodeURIComponent(id)}/insights`),
      /** Queues a link or pasted text for background processing and embedding. */
      create: (notebookId: string, source: NewSource) =>
        request<SourceDetail>('POST', '/sources/json', {
          body: { ...source, notebooks: [notebookId], embed: true, async_processing: true },
        }),
      retry: (id: string) => request<SourceDetail>('POST', `/sources/${encodeURIComponent(id)}/retry`),
      /** Uploads a file (PDF, audio, video, Office...) for the server to read. */
      upload: (notebookId: string, file: Blob, fileName: string, extra: { title?: string; transformations?: string[] } = {}) => {
        const form = new FormData()
        form.append('type', 'upload')
        form.append('notebooks', JSON.stringify([notebookId]))
        form.append('file', file, fileName)
        if (extra.title) form.append('title', extra.title)
        if (extra.transformations?.length) form.append('transformations', JSON.stringify(extra.transformations))
        form.append('embed', 'true')
        form.append('async_processing', 'true')
        return request<SourceDetail>('POST', '/sources', { body: form, timeoutMs: 600_000 })
      },
      /** The original uploaded file. */
      download: (id: string) =>
        request<{ data: ArrayBuffer; name?: string; type: string }>('GET', `/sources/${encodeURIComponent(id)}/download`, { binary: true }),
      listAll: () => request<SourceSummary[]>('GET', '/sources', { query: { sort_by: 'updated', sort_order: 'desc', limit: 500 } }),
      update: (id: string, data: { title?: string }) => request<SourceDetail>('PUT', `/sources/${encodeURIComponent(id)}`, { body: data }),
      link: (sourceId: string, notebookId: string) =>
        request<unknown>('POST', `/notebooks/${encodeURIComponent(notebookId)}/sources/${encodeURIComponent(sourceId)}`),
      unlink: (sourceId: string, notebookId: string) =>
        request<unknown>('DELETE', `/notebooks/${encodeURIComponent(notebookId)}/sources/${encodeURIComponent(sourceId)}`),
      delete: (id: string) => request<unknown>('DELETE', `/sources/${encodeURIComponent(id)}`),
    },

    transformations: {
      list: () => request<TransformationRecord[]>('GET', '/transformations'),
      create: (data: TransformationInput) => request<TransformationRecord>('POST', '/transformations', { body: data }),
      update: (id: string, data: Partial<TransformationInput>) =>
        request<TransformationRecord>('PUT', `/transformations/${encodeURIComponent(id)}`, { body: data }),
      delete: (id: string) => request<unknown>('DELETE', `/transformations/${encodeURIComponent(id)}`),
      execute: (transformationId: string, inputText: string) =>
        request<{ output: string }>('POST', '/transformations/execute', {
          body: { transformation_id: transformationId, input_text: inputText },
        }),
      getInstructions: () =>
        request<{ transformation_instructions: string }>('GET', '/transformations/default-prompt'),
      setInstructions: (text: string) =>
        request<unknown>('PUT', '/transformations/default-prompt', { body: { transformation_instructions: text } }),
    },

    insights: {
      create: (sourceId: string, transformationId: string) =>
        request<unknown>('POST', `/sources/${encodeURIComponent(sourceId)}/insights`, {
          body: { transformation_id: transformationId },
        }),
      delete: (id: string) => request<unknown>('DELETE', `/insights/${encodeURIComponent(id)}`),
      saveAsNote: (id: string, notebookId: string) =>
        request<Note>('POST', `/insights/${encodeURIComponent(id)}/save-as-note`, { body: { notebook_id: notebookId } }),
    },

    notes: {
      list: (notebookId: string) => request<Note[]>('GET', '/notes', { query: { notebook_id: notebookId } }),
      get: (id: string) => request<Note>('GET', `/notes/${encodeURIComponent(id)}`),
      create: (data: CreateNoteRequest) => request<Note>('POST', '/notes', { body: data }),
      update: (id: string, data: UpdateNoteRequest) =>
        request<Note>('PUT', `/notes/${encodeURIComponent(id)}`, { body: data }),
      delete: (id: string) => request<unknown>('DELETE', `/notes/${encodeURIComponent(id)}`),
    },

    chat: {
      listSessions: (notebookId: string) =>
        request<ChatSession[]>('GET', '/chat/sessions', { query: { notebook_id: notebookId } }),
      getSession: (id: string) =>
        request<ChatSessionWithMessages>('GET', `/chat/sessions/${encodeURIComponent(id)}`),
      createSession: (notebookId: string, title?: string) =>
        request<ChatSession>('POST', '/chat/sessions', { body: { notebook_id: notebookId, title } }),
      buildContext: (
        notebookId: string,
        config: { sources: Record<string, ContextMode>; notes: Record<string, ContextMode> },
      ) =>
        request<BuildContextResponse>('POST', '/chat/context', {
          body: { notebook_id: notebookId, context_config: config },
        }),
      send: (sessionId: string, message: string, context: ChatContext, modelOverride?: string) =>
        request<{ session_id: string; messages: ChatMessage[] }>('POST', '/chat/execute', {
          body: { session_id: sessionId, message, context, model_override: modelOverride },
        }),
      deleteSession: (id: string) => request<unknown>('DELETE', `/chat/sessions/${encodeURIComponent(id)}`),
      renameSession: (id: string, title: string) =>
        request<ChatSession>('PUT', `/chat/sessions/${encodeURIComponent(id)}`, { body: { title } }),
    },

    sourceChat: {
      listSessions: (sourceId: string) =>
        request<ChatSession[]>('GET', `/sources/${encodeURIComponent(sourceId)}/chat/sessions`),
      createSession: (sourceId: string, title?: string) =>
        request<ChatSession>('POST', `/sources/${encodeURIComponent(sourceId)}/chat/sessions`, {
          body: { source_id: sourceId, title },
        }),
      getSession: (sourceId: string, sessionId: string) =>
        request<ChatSessionWithMessages>(
          'GET',
          `/sources/${encodeURIComponent(sourceId)}/chat/sessions/${encodeURIComponent(sessionId)}`,
        ),
      renameSession: (sourceId: string, sessionId: string, title: string) =>
        request<ChatSession>('PUT', `/sources/${encodeURIComponent(sourceId)}/chat/sessions/${encodeURIComponent(sessionId)}`, {
          body: { title },
        }),
      deleteSession: (sourceId: string, sessionId: string) =>
        request<unknown>('DELETE', `/sources/${encodeURIComponent(sourceId)}/chat/sessions/${encodeURIComponent(sessionId)}`),
      /** The server answers as server-sent events; this collects the reply. */
      send: async (sourceId: string, sessionId: string, message: string, modelOverride?: string) => {
        const text = await request<string>(
          'POST',
          `/sources/${encodeURIComponent(sourceId)}/chat/sessions/${encodeURIComponent(sessionId)}/messages`,
          { body: { message, model_override: modelOverride }, raw: true },
        )
        let reply = ''
        for (const line of text.split('\n')) {
          if (!line.startsWith('data:')) continue
          const event = JSON.parse(line.slice(5).trim()) as { type: string; content?: string; message?: string }
          if (event.type === 'ai_message' && event.content) reply += event.content
          if (event.type === 'error') throw new ApiError(500, event.message ?? 'The source chat failed')
        }
        return reply
      },
    },

    studio: {
      generate: (
        notebookId: string,
        body: { kind: 'flashcards' | 'quiz'; source_ids: string[]; count: number; difficulty: string; language: string; instructions: string },
      ) => request<unknown>('POST', `/notebooks/${encodeURIComponent(notebookId)}/studio/generate`, { body, timeoutMs: 600_000 }),
    },

    live: (sdp: string, voice: string) =>
      request<{ session: { id: string }; transport: { sdp: string } }>('POST', '/live/session', { body: { sdp, voice }, timeoutMs: 50_000 }),

    usage: (days = 30) => request<ServerUsage>('GET', '/usage', { query: { days } }),

    settings: {
      get: () => request<Record<string, unknown>>('GET', '/settings'),
      update: (data: Record<string, unknown>) => request<Record<string, unknown>>('PUT', '/settings', { body: data }),
    },

    credentials: {
      list: () => request<ServerCredential[]>('GET', '/credentials'),
      create: (data: { name: string; provider: string; modalities: string[]; api_key?: string; base_url?: string }) =>
        request<ServerCredential>('POST', '/credentials', { body: data }),
      delete: (id: string) => request<unknown>('DELETE', `/credentials/${encodeURIComponent(id)}`),
      test: (id: string) => request<{ success?: boolean; message?: string }>('POST', `/credentials/${encodeURIComponent(id)}/test`),
      registerModels: (id: string) => request<unknown>('POST', `/credentials/${encodeURIComponent(id)}/register-models`),
    },

    jobs: {
      list: () => request<ServerJob[]>('GET', '/commands/jobs', { query: { limit: 50 } }),
      cancel: (id: string) => request<unknown>('DELETE', `/commands/jobs/${encodeURIComponent(id)}`),
    },

    embeddings: {
      rebuild: (mode: 'existing' | 'all') =>
        request<{ command_id?: string; message?: string }>('POST', '/embeddings/rebuild', {
          body: { mode, include_sources: true, include_notes: true, include_insights: true },
        }),
      status: (commandId: string) => request<Record<string, unknown>>('GET', `/embeddings/rebuild/${encodeURIComponent(commandId)}/status`),
    },

    models: {
      create: (data: { name: string; provider: string; type: string }) => request<unknown>('POST', '/models', { body: data }),
      delete: (id: string) => request<unknown>('DELETE', `/models/${encodeURIComponent(id)}`),
      test: (id: string) => request<{ success?: boolean; message?: string }>('POST', `/models/${encodeURIComponent(id)}/test`, { timeoutMs: 60_000 }),
      setDefaults: (data: Record<string, string | null>) => request<unknown>('PUT', '/models/defaults', { body: data }),
      providers: () => request<{ available: string[]; supported_types: Record<string, string[]> }>('GET', '/models/providers'),
      sync: () => request<unknown>('POST', '/models/sync', { timeoutMs: 120_000 }),
      list: () => request<Array<{ id: string; name: string; provider: string; type: string }>>('GET', '/models'),
      defaults: () => request<Record<string, string | null>>('GET', '/models/defaults'),
    },

    search: Object.assign((query: string, options: { notebookId?: string; type?: 'text' | 'vector'; limit?: number } = {}) =>
      request<SearchResponse>('POST', '/search', {
        body: {
          query,
          type: options.type ?? 'text',
          limit: options.limit ?? 30,
          notebook_id: options.notebookId,
          search_sources: true,
          search_notes: true,
        },
      }),
      {
        ask: (body: { question: string; strategy_model: string; answer_model: string; final_answer_model: string; notebook_id?: string }) =>
          request<{ answer: string; question: string }>('POST', '/search/ask/simple', { body, timeoutMs: 600_000 }),
      },
    ),

    podcasts: {
      list: () => request<PodcastEpisode[]>('GET', '/podcasts/episodes'),
      get: (id: string) => request<PodcastEpisode>('GET', `/podcasts/episodes/${encodeURIComponent(id)}`),
      delete: (id: string) => request<unknown>('DELETE', `/podcasts/episodes/${encodeURIComponent(id)}`),
      generate: (data: PodcastGenerationRequest) =>
        request<PodcastGenerationResponse>('POST', '/podcasts/generate', { body: data }),
      episodeProfiles: () => request<ServerEpisodeProfile[]>('GET', '/episode-profiles'),
      speakerProfiles: () => request<ServerSpeakerProfile[]>('GET', '/speaker-profiles'),
      saveEpisodeProfile: (body: Omit<ServerEpisodeProfile, 'id' | 'speaker_config_name'>, id?: string) =>
        id
          ? request<ServerEpisodeProfile>('PUT', `/episode-profiles/${encodeURIComponent(id)}`, { body })
          : request<ServerEpisodeProfile>('POST', '/episode-profiles', { body }),
      deleteEpisodeProfile: (id: string) => request<unknown>('DELETE', `/episode-profiles/${encodeURIComponent(id)}`),
      saveSpeakerProfile: (body: Omit<ServerSpeakerProfile, 'id'>, id?: string) =>
        id
          ? request<ServerSpeakerProfile>('PUT', `/speaker-profiles/${encodeURIComponent(id)}`, { body })
          : request<ServerSpeakerProfile>('POST', '/speaker-profiles', { body }),
      deleteSpeakerProfile: (id: string) => request<unknown>('DELETE', `/speaker-profiles/${encodeURIComponent(id)}`),
      retry: (id: string) => request<unknown>('POST', `/podcasts/episodes/${encodeURIComponent(id)}/retry`),
      languages: () => request<Array<{ code: string; name: string }>>('GET', '/languages'),
      /** Returns a short-lived URL the audio player can stream without an auth header. */
      audioUrl: async (episodeId: string) => {
        const { url } = await request<{ url: string }>(
          'POST',
          `/podcasts/episodes/${encodeURIComponent(episodeId)}/audio-access`,
        )
        return `${baseUrl}${url}`
      },
    },
  }
}

export type OpenNotebookClient = ReturnType<typeof createClient>

/**
 * Default chat context, matching the web app: each source contributes its
 * insights when it has some and its full text otherwise; every note is included.
 */
export function defaultContextConfig(
  sources: { id: string; insights_count: number }[],
  notes: { id: string }[],
) {
  return {
    sources: Object.fromEntries(
      sources.map((s): [string, ContextMode] => [s.id, s.insights_count > 0 ? 'insights' : 'full content']),
    ),
    notes: Object.fromEntries(notes.map((n): [string, ContextMode] => [n.id, 'full content'])),
  }
}

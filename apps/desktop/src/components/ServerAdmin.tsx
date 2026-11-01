import { RemoteBackend } from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { relativeTime } from '../lib/format.ts'
import { ErrorNote, Icon, Spinner, confirmAction } from './ui.tsx'

const DEFAULT_SLOTS: Array<[key: string, label: string, type: string]> = [
  ['default_chat_model', 'Chat', 'language'],
  ['default_transformation_model', 'Transformations and insights', 'language'],
  ['large_context_model', 'Large documents', 'language'],
  ['default_tools_model', 'Tools', 'language'],
  ['default_embedding_model', 'Embedding', 'embedding'],
  ['default_text_to_speech_model', 'Text to speech', 'text_to_speech'],
  ['default_speech_to_text_model', 'Speech to text', 'speech_to_text'],
]

const TYPE_LABEL: Record<string, string> = {
  language: 'Language',
  embedding: 'Embedding',
  text_to_speech: 'Text to speech',
  speech_to_text: 'Speech to text',
}

/** Server administration, as in the web app: models, keys, processing, jobs and the search index. */
export function ServerAdmin() {
  const backend = useBackend()
  if (!(backend instanceof RemoteBackend)) return null
  return (
    <>
      <section className="settings-section">
        <h2>Server models</h2>
        <p>Which model the server uses for each job, and the models it knows about.</p>
        <Models client={backend.client} />
      </section>
      <section className="settings-section">
        <h2>Server API keys</h2>
        <p>Keys stored on the server, encrypted. The server uses them; this app never sees them.</p>
        <Credentials client={backend.client} />
      </section>
      <section className="settings-section">
        <h2>Processing</h2>
        <p>How the server reads documents and pages.</p>
        <Processing client={backend.client} />
      </section>
      <section className="settings-section">
        <h2>Background jobs</h2>
        <Jobs client={backend.client} />
      </section>
    </>
  )
}

type Client = RemoteBackend['client']

function Models({ client }: { client: Client }) {
  const qc = useQueryClient()
  const models = useQuery({ queryKey: ['server-models'], queryFn: () => client.models.list() })
  const defaults = useQuery({ queryKey: ['server-model-defaults'], queryFn: () => client.models.defaults() })
  const providers = useQuery({ queryKey: ['server-providers'], queryFn: () => client.models.providers() })
  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ['server-models'] })
    void qc.invalidateQueries({ queryKey: ['server-model-defaults'] })
  }
  const setDefault = useMutation({
    mutationFn: (patch: Record<string, string | null>) => client.models.setDefaults({ ...(defaults.data ?? {}), ...patch }),
    onSuccess: refresh,
  })
  const remove = useMutation({ mutationFn: (id: string) => client.models.delete(id), onSuccess: refresh })
  const test = useMutation({ mutationFn: (id: string) => client.models.test(id) })
  const sync = useMutation({ mutationFn: () => client.models.sync(), onSuccess: refresh })
  const [adding, setAdding] = useState({ provider: '', name: '', type: 'language' })
  const add = useMutation({
    mutationFn: () => client.models.create(adding),
    onSuccess: () => {
      setAdding({ ...adding, name: '' })
      refresh()
    },
  })

  const byType = (type: string) => (models.data ?? []).filter((m) => m.type === type)

  return (
    <div className="form">
      {(models.isPending || defaults.isPending) && <Spinner />}
      <div className="defaults-grid">
        {DEFAULT_SLOTS.map(([key, label, type]) => (
          <label key={key} className="field">
            <span>{label}</span>
            <select value={defaults.data?.[key] ?? ''} onChange={(e) => setDefault.mutate({ [key]: e.target.value || null })}>
              <option value="">Not set</option>
              {byType(type).map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name} ({m.provider})
                </option>
              ))}
            </select>
          </label>
        ))}
      </div>
      {Object.keys(TYPE_LABEL).map((type) =>
        byType(type).length ? (
          <div key={type} className="profile-group">
            <div className="provider-head">
              <strong>{TYPE_LABEL[type]}</strong>
            </div>
            {byType(type).map((m) => (
              <div key={m.id} className="admin-row">
                <span className="profile-name">{m.name}</span>
                <span className="muted small">{m.provider}</span>
                <span className="spacer" />
                {test.variables === m.id && (test.isPending ? <Spinner /> : test.data ? <span className={`status ${test.data.success === false ? 'bad' : 'ok'}`}>{test.data.message ?? (test.data.success === false ? 'Failed' : 'Working')}</span> : null)}
                <button className="button quiet small" onClick={() => test.mutate(m.id)}>
                  Test
                </button>
                <button className="icon-button" onClick={() => void confirmAction(`Remove ${m.name}?`, 'Remove').then((ok) => ok && remove.mutate(m.id))} aria-label={`Remove ${m.name}`}>
                  <Icon name="trash" />
                </button>
              </div>
            ))}
          </div>
        ) : null,
      )}
      <div className="field-row">
        <select value={adding.provider} onChange={(e) => setAdding({ ...adding, provider: e.target.value })} aria-label="Provider" style={{ flex: 1 }}>
          <option value="">Provider</option>
          {providers.data?.available.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
        <select value={adding.type} onChange={(e) => setAdding({ ...adding, type: e.target.value })} aria-label="Type" style={{ flex: 1 }}>
          {(providers.data?.supported_types[adding.provider] ?? Object.keys(TYPE_LABEL)).map((t) => (
            <option key={t} value={t}>
              {TYPE_LABEL[t] ?? t}
            </option>
          ))}
        </select>
        <input value={adding.name} onChange={(e) => setAdding({ ...adding, name: e.target.value })} placeholder="Model name" aria-label="Model name" style={{ flex: 2 }} />
        <button className="button" onClick={() => add.mutate()} disabled={!adding.provider || !adding.name.trim() || add.isPending}>
          Add
        </button>
      </div>
      <div className="field-row">
        <button className="button quiet small" onClick={() => sync.mutate()} disabled={sync.isPending}>
          <Icon name="retry" size={14} /> {sync.isPending ? 'Looking for models' : 'Find models from saved keys'}
        </button>
      </div>
      <ErrorNote error={models.error ?? defaults.error ?? setDefault.error ?? remove.error ?? add.error ?? sync.error ?? test.error} />
    </div>
  )
}

const PROVIDERS = ['openai', 'anthropic', 'google', 'groq', 'mistral', 'deepseek', 'xai', 'openrouter', 'voyage', 'elevenlabs', 'ollama', 'openai_compatible']
const MODALITIES = ['language', 'embedding', 'text_to_speech', 'speech_to_text']

function Credentials({ client }: { client: Client }) {
  const qc = useQueryClient()
  const list = useQuery({ queryKey: ['server-credentials'], queryFn: () => client.credentials.list() })
  const refresh = () => void qc.invalidateQueries({ queryKey: ['server-credentials'] })
  const [draft, setDraft] = useState({ name: '', provider: 'openai', api_key: '', base_url: '', modalities: ['language'] })
  const create = useMutation({
    mutationFn: () => client.credentials.create({ ...draft, base_url: draft.base_url || undefined }),
    onSuccess: () => {
      setDraft({ ...draft, name: '', api_key: '', base_url: '' })
      refresh()
    },
  })
  const remove = useMutation({ mutationFn: (id: string) => client.credentials.delete(id), onSuccess: refresh })
  const test = useMutation({ mutationFn: (id: string) => client.credentials.test(id) })
  const register = useMutation({ mutationFn: (id: string) => client.credentials.registerModels(id) })

  return (
    <div className="form">
      {list.isPending && <Spinner />}
      {list.data?.map((c) => (
        <div key={c.id} className="admin-row">
          <span className="profile-name">{c.name}</span>
          <span className="muted small">
            {c.provider}, {c.modalities.join(', ').replace(/_/g, ' ')}
            {c.model_count ? `, ${c.model_count} models` : ''}
          </span>
          <span className="spacer" />
          {c.decryption_error && <span className="status bad">Can’t decrypt</span>}
          {test.variables === c.id && (test.isPending ? <Spinner /> : test.data ? <span className={`status ${test.data.success === false ? 'bad' : 'ok'}`}>{test.data.success === false ? 'Failed' : 'Working'}</span> : null)}
          <button className="button quiet small" onClick={() => test.mutate(c.id)}>
            Test
          </button>
          <button className="button quiet small" onClick={() => register.mutate(c.id)} disabled={register.isPending}>
            Add its models
          </button>
          <button className="icon-button" onClick={() => void confirmAction(`Delete the key “${c.name}”?`).then((ok) => ok && remove.mutate(c.id))} aria-label={`Delete ${c.name}`}>
            <Icon name="trash" />
          </button>
        </div>
      ))}
      <div className="field-row">
        <input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="Name" aria-label="Key name" style={{ flex: 1 }} />
        <select value={draft.provider} onChange={(e) => setDraft({ ...draft, provider: e.target.value })} aria-label="Provider" style={{ flex: 1 }}>
          {PROVIDERS.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
        <input type="password" value={draft.api_key} onChange={(e) => setDraft({ ...draft, api_key: e.target.value })} placeholder="API key" aria-label="API key" autoComplete="off" style={{ flex: 2 }} />
      </div>
      <div className="field-row">
        <div className="checks" style={{ flex: 1 }}>
          {MODALITIES.map((m) => (
            <label key={m} className="check">
              <input
                type="checkbox"
                checked={draft.modalities.includes(m)}
                onChange={(e) => setDraft({ ...draft, modalities: e.target.checked ? [...draft.modalities, m] : draft.modalities.filter((x) => x !== m) })}
              />
              {TYPE_LABEL[m]}
            </label>
          ))}
        </div>
        <button className="button" onClick={() => create.mutate()} disabled={!draft.name.trim() || !draft.modalities.length || create.isPending}>
          Save key on server
        </button>
      </div>
      <input value={draft.base_url} onChange={(e) => setDraft({ ...draft, base_url: e.target.value })} placeholder="Base URL, only for self-hosted or compatible providers" aria-label="Base URL" />
      <ErrorNote error={list.error ?? create.error ?? remove.error ?? test.error ?? register.error} />
    </div>
  )
}

function Processing({ client }: { client: Client }) {
  const qc = useQueryClient()
  const settings = useQuery({ queryKey: ['server-settings'], queryFn: () => client.settings.get() })
  const update = useMutation({
    mutationFn: (patch: Record<string, unknown>) => client.settings.update({ ...(settings.data ?? {}), ...patch }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['server-settings'] }),
  })
  const rebuild = useMutation({ mutationFn: () => client.embeddings.rebuild('all') })
  const s = settings.data ?? {}
  const select = (key: string, label: string, options: Array<[string, string]>) => (
    <label className="field">
      <span>{label}</span>
      <select value={String(s[key] ?? '')} onChange={(e) => update.mutate({ [key]: e.target.value })}>
        {options.map(([v, l]) => (
          <option key={v} value={v}>
            {l}
          </option>
        ))}
      </select>
    </label>
  )
  const toggle = (key: string, label: string) => (
    <label className="check">
      <input type="checkbox" checked={Boolean(s[key])} onChange={(e) => update.mutate({ [key]: e.target.checked })} />
      {label}
    </label>
  )
  return (
    <div className="form">
      {settings.isPending && <Spinner />}
      <div className="defaults-grid">
        {select('default_content_processing_engine_doc', 'Documents', [['auto', 'Automatic'], ['docling', 'Docling'], ['simple', 'Simple']])}
        {select('default_content_processing_engine_url', 'Web pages', [['auto', 'Automatic'], ['firecrawl', 'Firecrawl'], ['jina', 'Jina'], ['crawl4ai', 'Crawl4AI'], ['simple', 'Simple']])}
        {select('default_embedding_option', 'Index new sources', [['ask', 'Ask each time'], ['always', 'Always'], ['never', 'Never']])}
        {select('auto_delete_files', 'Delete uploads after reading', [['yes', 'Yes'], ['no', 'No']])}
      </div>
      <div className="checks">
        {toggle('docling_ocr', 'Read scanned pages (OCR)')}
        {toggle('docling_formulas', 'Read formulas')}
        {toggle('docling_vision', 'Describe images')}
      </div>
      <label className="field">
        <span>YouTube caption languages, in order of preference</span>
        <input
          defaultValue={((s.youtube_preferred_languages as string[] | undefined) ?? []).join(', ')}
          key={JSON.stringify(s.youtube_preferred_languages)}
          onBlur={(e) => update.mutate({ youtube_preferred_languages: e.target.value.split(',').map((x) => x.trim()).filter(Boolean) })}
        />
      </label>
      <div className="field-row">
        <button className="button" onClick={() => void confirmAction('Re-embed every source, note and insight on the server? It runs in the background.', 'Rebuild').then((ok) => ok && rebuild.mutate())} disabled={rebuild.isPending}>
          <Icon name="retry" size={14} /> Rebuild search index
        </button>
        {rebuild.isSuccess && <span className="status ok">Started</span>}
      </div>
      <ErrorNote error={settings.error ?? update.error ?? rebuild.error} />
    </div>
  )
}

function Jobs({ client }: { client: Client }) {
  const qc = useQueryClient()
  const jobs = useQuery({ queryKey: ['server-jobs'], queryFn: () => client.jobs.list(), refetchInterval: 5000 })
  const cancel = useMutation({ mutationFn: (id: string) => client.jobs.cancel(id), onSuccess: () => void qc.invalidateQueries({ queryKey: ['server-jobs'] }) })
  return (
    <div className="form">
      {jobs.isPending && <Spinner />}
      {jobs.data?.length === 0 && <p className="muted">Nothing running</p>}
      {jobs.data?.map((j) => {
        const id = j.job_id ?? j.id ?? ''
        return (
          <div key={id} className="admin-row">
            <span className="profile-name">{(j.command ?? j.name ?? 'Job').replace(/[_:]+/g, ' ')}</span>
            <span className="muted small">
              {j.status}
              {j.created ? `, ${relativeTime(j.created)}` : ''}
            </span>
            <span className="spacer" />
            {j.error_message && <span className="status bad" title={j.error_message}>Error</span>}
            {['new', 'queued', 'running', 'pending'].includes(j.status) && (
              <button className="button quiet small" onClick={() => cancel.mutate(id)}>
                Cancel
              </button>
            )}
          </div>
        )
      })}
      <ErrorNote error={jobs.error ?? cancel.error} />
    </div>
  )
}

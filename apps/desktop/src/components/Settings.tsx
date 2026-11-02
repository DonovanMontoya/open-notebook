import { createClient } from '@open-notebook/api-client'
import {
  createProvider,
  createVoiceProvider,
  DEFAULT_ANTHROPIC_MODEL,
  DEFAULT_TRANSCRIPTION_MODELS,
  DEFAULT_VOICE_MODELS,
  PROVIDER_LABELS,
  type ModelInfo,
  type ProviderKind,
} from '@open-notebook/engine'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState } from 'react'

import { useBackendState } from '../lib/backend.tsx'
import { errorMessage } from '../lib/format.ts'
import {
  getSecret,
  providerConfig,
  secretName,
  setKeySync,
  setSecret,
  updateSettings,
  useSecretVersion,
  useSettings,
  type ModelSetting,
} from '../lib/settings.ts'
import { platformFetch } from '../platform/index.ts'
import { PodcastProfiles } from './Profiles.tsx'
import { ServerAdmin } from './ServerAdmin.tsx'
import { ErrorNote, Icon, Spinner, confirmAction } from './ui.tsx'

const KINDS: ProviderKind[] = ['anthropic', 'openai', 'gemini', 'openai-compatible']

const KEY_HINTS: Record<ProviderKind, string> = {
  anthropic: 'Claude models for answers. Anthropic has no embeddings, so pair it with another provider for search.',
  openai: 'Answers and embeddings from one key.',
  gemini: 'Answers and embeddings from one key.',
  'openai-compatible': 'OpenRouter, Groq, or a model running on this Mac in LM Studio or Ollama. The key is optional for local servers.',
}

const ROLE_LABEL = { chat: 'Chat model', transformation: 'Transformation model', embedding: 'Embedding model', transcription: 'Speech to text' } as const

const DEFAULT_EMBEDDING: Partial<Record<ProviderKind, string>> = {
  openai: 'text-embedding-3-small',
  gemini: 'gemini-embedding-001',
}

export function SettingsView({ onBack }: { onBack?: () => void }) {
  const settings = useSettings()

  return (
    <div className="settings">
      {onBack && (
        <button className="button quiet small back-button" onClick={onBack}>
          <Icon name="back" size={14} /> Notebooks
        </button>
      )}
      <h1>Settings</h1>
      <p className="settings-intro">Choose where notebooks are kept and which models answer your questions.</p>

      <section className="settings-section">
        <h2>Where notebooks live</h2>
        <p>Each place keeps its own notebooks. Switching does not move or delete anything.</p>
        <div className="mode-choice">
          <button className={`mode-option${settings.mode === 'local' ? ' active' : ''}`} onClick={() => updateSettings({ mode: 'local' })}>
            <strong>
              <Icon name="device" /> This device
            </strong>
            <span>Stored here. Questions go straight to the provider you pick, with your key.</span>
          </button>
          <button className={`mode-option${settings.mode === 'remote' ? ' active' : ''}`} onClick={() => updateSettings({ mode: 'remote' })}>
            <strong>
              <Icon name="server" /> Open Notebook server
            </strong>
            <span>Use notebooks and models on your own server, including local models.</span>
          </button>
        </div>
      </section>

      {settings.mode === 'remote' ? (
        <>
          <ServerSettings />
          <ServerPodcasts />
          <ServerAdmin />
        </>
      ) : (
        <LocalSettings />
      )}
    </div>
  )
}

function ServerSettings() {
  const settings = useSettings()
  const state = useBackendState()
  const [url, setUrl] = useState(settings.serverUrl)
  const [password, setPassword] = useState('')
  const [hasPassword, setHasPassword] = useState(false)

  useEffect(() => {
    void getSecret(secretName.serverPassword).then((p) => setHasPassword(!!p))
  }, [])

  const test = useMutation({
    mutationFn: async () => {
      const doFetch = await platformFetch()
      const pw = password || (await getSecret(secretName.serverPassword)) || undefined
      const client = createClient({ baseUrl: url, password: pw, fetch: doFetch, timeoutMs: 15_000 })
      const auth = await client.authStatus()
      const notebooks = await client.notebooks.list()
      return { auth, count: notebooks.length }
    },
  })

  const save = async () => {
    if (password) {
      await setSecret(secretName.serverPassword, password)
      setPassword('')
      setHasPassword(true)
    }
    updateSettings({ serverUrl: url.trim() })
    test.mutate()
  }

  return (
    <section className="settings-section">
      <h2>Server</h2>
      <p>The address you open Open Notebook at, for example its Tailscale address.</p>
      <div className="form">
        <label className="field">
          <span>Address</span>
          <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://notebook.example.ts.net:8502" inputMode="url" />
        </label>
        <label className="field">
          <span>Password</span>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder={hasPassword ? 'Saved on this device' : 'Only if the server has one'}
            autoComplete="off"
          />
        </label>
        <div className="field-row">
          <button className="button primary" onClick={() => void save()} disabled={!url.trim() || test.isPending}>
            Save and connect
          </button>
          {hasPassword && (
            <button
              className="button quiet"
              onClick={() => void setSecret(secretName.serverPassword, '').then(() => setHasPassword(false))}
            >
              Forget password
            </button>
          )}
          <span className="spacer" />
          {test.isPending && <Spinner />}
          {test.isSuccess && (
            <span className="status ok">
              Connected, {test.data.count} {test.data.count === 1 ? 'notebook' : 'notebooks'}
            </span>
          )}
        </div>
        {test.isError && <ErrorNote error={test.error} />}
        {state.status === 'error' && <ErrorNote error={state.error} />}
      </div>
    </section>
  )
}

/** Podcast profiles on the server, edited here like on device. */
function ServerPodcasts() {
  const state = useBackendState()
  if (state.status !== 'ready' || state.backend.kind !== 'remote') return null
  return (
    <section className="settings-section">
      <h2>Podcasts</h2>
      <p>The server’s episode and speaker profiles.</p>
      <PodcastProfiles />
    </section>
  )
}

function LocalSettings() {
  const version = useSecretVersion()
  const keys = useQuery({
    queryKey: ['provider-keys', version],
    queryFn: async () => Object.fromEntries(await Promise.all(KINDS.map(async (k) => [k, !!(await getSecret(secretName.provider(k)))] as const))),
  })
  const configured = KINDS.filter((k) => keys.data?.[k] || k === 'openai-compatible')

  return (
    <>
      <section className="settings-section">
        <h2>Models</h2>
        <p>Answers use the chat model. The embedding model indexes sources so questions find the right passages.</p>
        <div className="form">
          <ModelPicker role="chat" providers={configured} />
          <ModelPicker role="transformation" providers={configured} />
          <ModelPicker role="embedding" providers={configured.filter((k) => k !== 'anthropic')} />
          <ModelPicker role="transcription" providers={configured.filter((k) => k !== 'anthropic')} />
          <RebuildIndex />
        </div>
      </section>

      <section className="settings-section">
        <h2>Podcasts</h2>
        <p>Episode profiles set the angle and length; speaker profiles set who talks and with which voices.</p>
        <PodcastProfiles />
      </section>

      <section className="settings-section">
        <h2>API keys</h2>
        <p>Keys are saved on this device and only sent to their provider. They stay when you update or reinstall the app.</p>
        <KeySync />
        {KINDS.map((kind) => (
          <ProviderKey key={kind} kind={kind} saved={!!keys.data?.[kind]} />
        ))}
        <ElevenLabsKey />
      </section>
    </>
  )
}

function KeySync() {
  const settings = useSettings()
  const toggle = useMutation({ mutationFn: (enabled: boolean) => setKeySync(enabled) })
  const result = toggle.data

  return (
    <div className="sync-row">
      <label className="switch">
        <input
          type="checkbox"
          checked={settings.syncKeys}
          disabled={toggle.isPending}
          onChange={(e) => toggle.mutate(e.target.checked)}
        />
        <span>
          <strong>Sync with iCloud Keychain</strong>
          <small>Your keys follow you to your other Macs and your iPhone.</small>
        </span>
      </label>
      {toggle.isPending && <Spinner />}
      {result && !result.ok && (
        <p className="sync-note">
          {result.reason === 'unsigned'
            ? 'This build is not signed for iCloud Keychain yet, so keys stay in the app on this Mac. Sync turns on once the app is signed with your developer team.'
            : `Keys could not be moved: ${result.message}`}
        </p>
      )}
    </div>
  )
}

function ProviderKey({ kind, saved }: { kind: ProviderKind; saved: boolean }) {
  const settings = useSettings()
  const [value, setValue] = useState('')
  const [baseUrl, setBaseUrl] = useState(settings.compatibleBaseUrl)

  const check = useMutation({
    mutationFn: async () => {
      const provider = createProvider(await providerConfig(kind), await platformFetch())
      return (await provider.listModels()).length
    },
  })

  const save = async () => {
    if (kind === 'openai-compatible') updateSettings({ compatibleBaseUrl: baseUrl.trim() })
    if (value) await setSecret(secretName.provider(kind), value)
    setValue('')
    check.mutate()
  }

  return (
    <div className="provider">
      <div className="provider-head">
        <strong>{PROVIDER_LABELS[kind]}</strong>
        {check.isPending ? (
          <Spinner />
        ) : check.isSuccess ? (
          <span className="status ok">Working, {check.data} models</span>
        ) : check.isError ? (
          <span className="status bad">Not working</span>
        ) : saved ? (
          <span className="status ok">Key saved</span>
        ) : null}
      </div>
      <div className="form" style={{ gap: 8 }}>
        {kind === 'openai-compatible' && (
          <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://openrouter.ai/api/v1" aria-label="Base URL" />
        )}
        <div className="field-row">
          <input
            type="password"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={saved ? 'Saved. Paste a new key to replace it' : 'Paste API key'}
            aria-label={`${PROVIDER_LABELS[kind]} API key`}
            autoComplete="off"
            onKeyDown={(e) => e.key === 'Enter' && void save()}
          />
          <button className="button" onClick={() => void save()} disabled={!value && !(kind === 'openai-compatible' && baseUrl !== settings.compatibleBaseUrl) && !saved}>
            {value ? 'Save' : 'Check'}
          </button>
          {saved && (
            <button className="icon-button" title="Remove key" aria-label="Remove key" onClick={() => void setSecret(secretName.provider(kind), '')}>
              <Icon name="trash" />
            </button>
          )}
        </div>
        <small className="muted">{KEY_HINTS[kind]}</small>
        {check.isError && <ErrorNote error={check.error} />}
      </div>
    </div>
  )
}

function ModelPicker({ role, providers }: { role: 'chat' | 'transformation' | 'embedding' | 'transcription'; providers: ProviderKind[] }) {
  const settings = useSettings()
  const current = settings[role]
  const kind = current?.provider ?? providers[0]
  const version = useSecretVersion()

  const models = useQuery({
    queryKey: ['models', kind, version, settings.compatibleBaseUrl],
    enabled: !!kind,
    staleTime: 5 * 60_000,
    retry: false,
    queryFn: async (): Promise<ModelInfo[]> => createProvider(await providerConfig(kind!), await platformFetch()).listModels(),
  })

  const options = (models.data ?? []).filter((m) =>
    role === 'transcription'
      ? kind === 'gemini'
        ? m.capabilities?.includes('chat')
        : /transcri|whisper/i.test(m.id)
      : !m.capabilities || m.capabilities.includes(role === 'transformation' ? 'chat' : role),
  )

  const set = (next: Partial<ModelSetting>) => {
    const provider = next.provider ?? kind
    if (!provider) return
    const model =
      next.model ??
      (next.provider && next.provider !== current?.provider
        ? role === 'chat' || role === 'transformation'
          ? provider === 'anthropic'
            ? DEFAULT_ANTHROPIC_MODEL
            : ''
          : role === 'transcription'
            ? (DEFAULT_TRANSCRIPTION_MODELS[provider] ?? '')
            : (DEFAULT_EMBEDDING[provider] ?? '')
        : (current?.model ?? ''))
    updateSettings({ [role]: { provider, model } })
  }

  if (!providers.length) {
    return (
      <div className="field">
        <span>{ROLE_LABEL[role]}</span>
        <small>Save an API key below first</small>
      </div>
    )
  }

  return (
    <div className="field">
      <span>{ROLE_LABEL[role]}</span>
      <div className="model-pick">
        <select value={kind} onChange={(e) => set({ provider: e.target.value as ProviderKind })} aria-label="Provider">
          {providers.map((k) => (
            <option key={k} value={k}>
              {PROVIDER_LABELS[k]}
            </option>
          ))}
        </select>
        <input
          list={`models-${role}`}
          value={current?.provider === kind ? current.model : ''}
          placeholder={models.isPending ? 'Loading models' : 'Choose or type a model'}
          onChange={(e) => set({ model: e.target.value.trim() })}
          aria-label="Model"
        />
        <datalist id={`models-${role}`}>
          {options.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label ?? ''}
            </option>
          ))}
        </datalist>
      </div>
      {models.isError ? (
        <small className="danger">Could not list models: {errorMessage(models.error)}</small>
      ) : role === 'embedding' ? (
        <small>Without one, search matches words instead of meaning. Changing it re-indexes new sources only.</small>
      ) : role === 'transformation' ? (
        <small>Writes insights and study decks. Leave empty to use the chat model.</small>
      ) : role === 'transcription' ? (
        <small>Turns audio and video sources into text. Up to 25 MB with OpenAI, 19 MB with Gemini.</small>
      ) : null}
    </div>
  )
}

function ElevenLabsKey() {
  const version = useSecretVersion()
  const [value, setValue] = useState('')
  const saved = useQuery({ queryKey: ['elevenlabs-key', version], queryFn: async () => !!(await getSecret(secretName.provider('elevenlabs'))) })
  const check = useMutation({
    mutationFn: async () => {
      const key = (await getSecret(secretName.provider('elevenlabs'))) ?? undefined
      const p = createVoiceProvider({ kind: 'elevenlabs', model: DEFAULT_VOICE_MODELS.elevenlabs, voices: [], apiKey: key }, await platformFetch())
      return (await p.listVoices()).length
    },
  })
  const input = useRef<HTMLInputElement>(null)
  const save = async () => {
    if (value) await setSecret(secretName.provider('elevenlabs'), value)
    setValue('')
    check.mutate()
  }
  return (
    <div className="provider">
      <div className="provider-head">
        <strong>ElevenLabs</strong>
        {check.isPending ? (
          <Spinner />
        ) : check.isSuccess ? (
          <span className="status ok">Working, {check.data} voices</span>
        ) : check.isError ? (
          <span className="status bad">Not working</span>
        ) : saved.data ? (
          <span className="status ok">Key saved</span>
        ) : null}
      </div>
      <div className="form" style={{ gap: 8 }}>
        <div className="field-row">
          <input
            ref={input}
            type="password"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={saved.data ? 'Saved. Paste a new key to replace it' : 'Paste API key'}
            aria-label="ElevenLabs API key"
            autoComplete="off"
            onKeyDown={(e) => e.key === 'Enter' && void save()}
          />
          <button className="button" onClick={() => void save()} disabled={!value && !saved.data}>
            {value ? 'Save' : 'Check'}
          </button>
          {saved.data && (
            <button className="icon-button" title="Remove key" aria-label="Remove key" onClick={() => void setSecret(secretName.provider('elevenlabs'), '')}>
              <Icon name="trash" />
            </button>
          )}
        </div>
        <small className="muted">Only used for podcast voices.</small>
        {check.isError && <ErrorNote error={check.error} />}
      </div>
    </div>
  )
}

/** Re-embeds every source, for after changing the embedding model. */
function RebuildIndex() {
  const state = useBackendState()
  const rebuild = useMutation({
    mutationFn: async () => (state.status === 'ready' ? state.backend.rebuildIndex() : 0),
  })
  return (
    <div className="field-row">
      <button className="button" onClick={() => void confirmAction('Read and index every source again with the current embedding model?', 'Rebuild').then((ok) => ok && rebuild.mutate())} disabled={rebuild.isPending}>
        <Icon name="retry" size={14} /> Rebuild search index
      </button>
      {rebuild.isSuccess && <span className="status ok">Re-indexing {rebuild.data} sources</span>}
      {rebuild.isError && <ErrorNote error={rebuild.error} />}
    </div>
  )
}

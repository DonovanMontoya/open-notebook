import {
  createVoiceProvider,
  DEFAULT_VOICE_MODELS,
  SAMPLE_RATE,
  VOICE_LABELS,
  WORDS_PER_SEGMENT,
  type EpisodeProfile,
  type Speaker,
  type SpeakerProfile,
  type VoiceKind,
} from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { errorMessage } from '../lib/format.ts'
import { useSecretVersion, voiceConfig } from '../lib/settings.ts'
import { platformFetch } from '../platform/index.ts'
import { openSocket } from '../platform/media.ts'
import { ErrorNote, Icon, Modal, Spinner, confirmAction } from './ui.tsx'

export const profilesKey = ['podcast-profiles'] as const

const VOICE_KINDS: VoiceKind[] = ['openai-live', 'openai', 'gemini', 'elevenlabs']
const LANGUAGES = [
  ['', 'Same as the sources'],
  ['en', 'English'],
  ['es', 'Spanish'],
  ['fr', 'French'],
  ['de', 'German'],
  ['it', 'Italian'],
  ['pt', 'Portuguese'],
  ['nl', 'Dutch'],
  ['pl', 'Polish'],
  ['tr', 'Turkish'],
  ['ru', 'Russian'],
  ['ja', 'Japanese'],
  ['zh', 'Chinese'],
  ['hi', 'Hindi'],
  ['bn', 'Bengali'],
  ['ca', 'Catalan'],
]
const SAMPLE_LINE = 'Here is the short version: the report found four tricks that keep people subscribed, and the cancel button is the worst of them.'

const humanize = (name: string) => name.replace(/[_-]+/g, ' ').replace(/^\w/, (c) => c.toUpperCase())

/** Episode and speaker profiles: the same editor for on-device and server podcasts. */
export function PodcastProfiles() {
  const backend = useBackend()
  const profiles = useQuery({ queryKey: profilesKey, queryFn: () => backend.podcastProfiles() })
  const [editing, setEditing] = useState<{ kind: 'episode'; profile?: EpisodeProfile } | { kind: 'speaker'; profile?: SpeakerProfile }>()

  return (
    <div className="form">
      {profiles.isPending && <Spinner />}
      <ErrorNote error={profiles.error} />
      {profiles.data && (
        <>
          <div className="profile-group">
            <div className="provider-head">
              <strong>Episode profiles</strong>
              <span className="spacer" />
              <button className="button small" onClick={() => setEditing({ kind: 'episode' })}>
                <Icon name="plus" size={14} /> New
              </button>
            </div>
            {profiles.data.episode.map((p) => (
              <button key={p.id} className="profile-row" onClick={() => setEditing({ kind: 'episode', profile: p })}>
                <span className="profile-name">{humanize(p.name)}</span>
                <span className="profile-meta">
                  {p.segments} segments, about {Math.round((p.segments * WORDS_PER_SEGMENT) / 150)} min
                  {p.description ? `. ${p.description}` : ''}
                </span>
              </button>
            ))}
          </div>
          <div className="profile-group">
            <div className="provider-head">
              <strong>Speaker profiles</strong>
              <span className="spacer" />
              <button className="button small" onClick={() => setEditing({ kind: 'speaker' })}>
                <Icon name="plus" size={14} /> New
              </button>
            </div>
            {profiles.data.speaker.map((p) => (
              <button key={p.id} className="profile-row" onClick={() => setEditing({ kind: 'speaker', profile: p })}>
                <span className="profile-name">{humanize(p.name)}</span>
                <span className="profile-meta">
                  {p.speakers.map((s) => s.name).join(', ')}
                  {p.voiceKind ? `, ${VOICE_LABELS[p.voiceKind]}` : ''}
                </span>
              </button>
            ))}
          </div>
        </>
      )}
      {editing?.kind === 'episode' && profiles.data && (
        <EpisodeEditor profile={editing.profile} speakers={profiles.data.speaker} onClose={() => setEditing(undefined)} />
      )}
      {editing?.kind === 'speaker' && <SpeakerEditor profile={editing.profile} onClose={() => setEditing(undefined)} />}
    </div>
  )
}

function EpisodeEditor({ profile, speakers, onClose }: { profile?: EpisodeProfile; speakers: SpeakerProfile[]; onClose: () => void }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const [draft, setDraft] = useState<Omit<EpisodeProfile, 'id'>>(
    profile ?? { name: '', description: '', speakerProfileId: speakers[0]?.id ?? '', language: null, briefing: '', segments: 5 },
  )
  const set = (patch: Partial<typeof draft>) => setDraft((d) => ({ ...d, ...patch }))
  const done = () => {
    void qc.invalidateQueries({ queryKey: profilesKey })
    onClose()
  }
  const save = useMutation({ mutationFn: () => backend.saveEpisodeProfile({ ...draft, id: profile?.id }), onSuccess: done })
  const remove = useMutation({ mutationFn: () => backend.deleteEpisodeProfile(profile!.id), onSuccess: done })

  return (
    <Modal
      wide
      title={profile ? humanize(profile.name) : 'New episode profile'}
      onClose={onClose}
      footer={
        <>
          {profile && (
            <button className="button quiet danger" onClick={() => void confirmAction('Delete this episode profile?').then((ok) => ok && remove.mutate())}>
              Delete
            </button>
          )}
          <span className="spacer" />
          <button className="button" onClick={onClose}>
            Cancel
          </button>
          <button className="button primary" onClick={() => save.mutate()} disabled={!draft.name.trim() || !draft.briefing.trim() || !draft.speakerProfileId || save.isPending}>
            Save profile
          </button>
        </>
      }
    >
      <div className="form">
        <label className="field">
          <span>Name</span>
          <input value={draft.name} onChange={(e) => set({ name: e.target.value })} placeholder="Weekly briefing" />
        </label>
        <label className="field">
          <span>Description</span>
          <input value={draft.description} onChange={(e) => set({ description: e.target.value })} />
        </label>
        <div className="field-row">
          <label className="field" style={{ flex: 2 }}>
            <span>Speakers</span>
            <select value={draft.speakerProfileId} onChange={(e) => set({ speakerProfileId: e.target.value })}>
              {speakers.map((s) => (
                <option key={s.id} value={s.id}>
                  {humanize(s.name)} ({s.speakers.map((x) => x.name.split(' ')[0]).join(', ')})
                </option>
              ))}
            </select>
          </label>
          <label className="field" style={{ flex: 1 }}>
            <span>Segments</span>
            <input type="number" min={1} max={12} value={draft.segments} onChange={(e) => set({ segments: Number(e.target.value) })} />
          </label>
          <label className="field" style={{ flex: 1.4 }}>
            <span>Language</span>
            <select value={draft.language ?? ''} onChange={(e) => set({ language: e.target.value || null })}>
              {LANGUAGES.map(([code, name]) => (
                <option key={code} value={code}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        </div>
        <label className="field">
          <span>Briefing</span>
          <textarea rows={6} value={draft.briefing} onChange={(e) => set({ briefing: e.target.value })} placeholder="The angle and style of the episode, e.g. explain the ideas for a newcomer with examples" />
          <small>About {Math.round((draft.segments * WORDS_PER_SEGMENT) / 150)} minutes. Each episode can add its own focus on top.</small>
        </label>
        <ErrorNote error={save.error ?? remove.error} />
      </div>
    </Modal>
  )
}

const EMPTY_SPEAKER: Speaker = { name: '', voice: '', backstory: '', personality: '' }

async function playPcm(pcm: Int16Array): Promise<void> {
  const ctx = new AudioContext({ sampleRate: SAMPLE_RATE })
  const buffer = ctx.createBuffer(1, pcm.length, SAMPLE_RATE)
  const channel = buffer.getChannelData(0)
  for (let i = 0; i < pcm.length; i++) channel[i] = pcm[i] / 32768
  const source = ctx.createBufferSource()
  source.buffer = buffer
  source.connect(ctx.destination)
  source.start()
  await new Promise((resolve) => (source.onended = resolve))
  await ctx.close()
}

function SpeakerEditor({ profile, onClose }: { profile?: SpeakerProfile; onClose: () => void }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const local = backend.podcastMode === 'local'
  const version = useSecretVersion()
  const [draft, setDraft] = useState<Omit<SpeakerProfile, 'id'>>(
    profile ?? {
      name: '',
      description: '',
      voiceKind: local ? 'openai' : undefined,
      voiceModel: local ? DEFAULT_VOICE_MODELS.openai : '',
      speakers: [{ ...EMPTY_SPEAKER, voice: 'marin' }],
    },
  )
  const set = (patch: Partial<typeof draft>) => setDraft((d) => ({ ...d, ...patch }))
  const kind = draft.voiceKind ?? 'openai'

  // On device, voices and models come from the chosen provider. A server keeps its own list.
  const provider = useQuery({
    queryKey: ['voice-provider', kind, draft.voiceModel, version],
    enabled: local,
    queryFn: async () => createVoiceProvider({ ...(await voiceConfig(kind, draft.voiceModel)) }, await platformFetch(), openSocket),
  })
  const voices = useQuery({ queryKey: ['voices', kind, version], enabled: !!provider.data, retry: false, queryFn: () => provider.data!.listVoices() })
  const models = useQuery({ queryKey: ['voice-models', kind, version], enabled: !!provider.data, retry: false, staleTime: 300_000, queryFn: () => provider.data!.listModels() })
  const serverModels = useQuery({
    queryKey: ['server-tts-models'],
    enabled: !local,
    queryFn: async () => (await backend.listModels()).filter((m) => m.type === 'text_to_speech'),
  })

  const done = () => {
    void qc.invalidateQueries({ queryKey: profilesKey })
    onClose()
  }
  const save = useMutation({ mutationFn: () => backend.saveSpeakerProfile({ ...draft, id: profile?.id }), onSuccess: done })
  const remove = useMutation({ mutationFn: () => backend.deleteSpeakerProfile(profile!.id), onSuccess: done })
  const setSpeaker = (i: number, patch: Partial<Speaker>) => set({ speakers: draft.speakers.map((s, j) => (j === i ? { ...s, ...patch } : s)) })

  return (
    <Modal
      wide
      title={profile ? humanize(profile.name) : 'New speaker profile'}
      onClose={onClose}
      footer={
        <>
          {profile && (
            <button className="button quiet danger" onClick={() => void confirmAction('Delete this speaker profile?').then((ok) => ok && remove.mutate())}>
              Delete
            </button>
          )}
          <span className="spacer" />
          <button className="button" onClick={onClose}>
            Cancel
          </button>
          <button
            className="button primary"
            onClick={() => save.mutate()}
            disabled={!draft.name.trim() || !draft.speakers.length || draft.speakers.some((s) => !s.name.trim() || !s.voice.trim()) || save.isPending}
          >
            Save profile
          </button>
        </>
      }
    >
      <div className="form">
        <div className="field-row">
          <label className="field" style={{ flex: 1 }}>
            <span>Name</span>
            <input value={draft.name} onChange={(e) => set({ name: e.target.value })} placeholder="Two analysts" />
          </label>
          <label className="field" style={{ flex: 2 }}>
            <span>Description</span>
            <input value={draft.description} onChange={(e) => set({ description: e.target.value })} />
          </label>
        </div>
        {local ? (
          <div className="field-row">
            <label className="field" style={{ flex: 1 }}>
              <span>Voice provider</span>
              <select
                value={kind}
                onChange={(e) => {
                  const k = e.target.value as VoiceKind
                  set({ voiceKind: k, voiceModel: DEFAULT_VOICE_MODELS[k], speakers: draft.speakers.map((s) => ({ ...s, voice: '' })) })
                }}
              >
                {VOICE_KINDS.map((k) => (
                  <option key={k} value={k}>
                    {VOICE_LABELS[k]}
                  </option>
                ))}
              </select>
            </label>
            <label className="field" style={{ flex: 1 }}>
              <span>Voice model</span>
              <input list="speaker-voice-models" value={draft.voiceModel} onChange={(e) => set({ voiceModel: e.target.value.trim() })} />
              <datalist id="speaker-voice-models">
                {models.data?.map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            </label>
          </div>
        ) : (
          <label className="field">
            <span>Voice model on the server</span>
            <select value={draft.voiceModel} onChange={(e) => set({ voiceModel: e.target.value })}>
              <option value="">Server default</option>
              {serverModels.data?.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name} ({m.provider})
                </option>
              ))}
            </select>
          </label>
        )}
        {kind === 'openai-live' && local && <small>OpenAI Live records in real time, so episodes take about one and a half times their length to make.</small>}
        {voices.isError && <small className="danger">Could not load voices: {errorMessage(voices.error)}</small>}

        {draft.speakers.map((sp, i) => (
          <SpeakerRow
            key={i}
            index={i}
            speaker={sp}
            voiceOptions={local ? (voices.data ?? []) : undefined}
            onChange={(patch) => setSpeaker(i, patch)}
            onRemove={draft.speakers.length > 1 ? () => set({ speakers: draft.speakers.filter((_, j) => j !== i) }) : undefined}
            onPreview={
              local && provider.data
                ? async () => {
                    await playPcm(await provider.data!.speakLine(SAMPLE_LINE, sp.voice))
                  }
                : undefined
            }
          />
        ))}
        {draft.speakers.length < 4 && (
          <button className="button small" style={{ alignSelf: 'flex-start' }} onClick={() => set({ speakers: [...draft.speakers, { ...EMPTY_SPEAKER }] })}>
            <Icon name="plus" size={14} /> Add speaker
          </button>
        )}
        <ErrorNote error={save.error ?? remove.error} />
      </div>
    </Modal>
  )
}

function SpeakerRow({
  index,
  speaker,
  voiceOptions,
  onChange,
  onRemove,
  onPreview,
}: {
  index: number
  speaker: Speaker
  voiceOptions?: Array<{ id: string; label: string }>
  onChange: (patch: Partial<Speaker>) => void
  onRemove?: () => void
  onPreview?: () => Promise<void>
}) {
  const preview = useMutation({ mutationFn: () => onPreview!() })
  return (
    <fieldset className="speaker-card">
      <legend>Speaker {index + 1}</legend>
      <div className="field-row">
        <input value={speaker.name} onChange={(e) => onChange({ name: e.target.value })} placeholder="Name, e.g. Maya Chen" aria-label="Name" style={{ flex: 1 }} />
        {voiceOptions ? (
          <select value={speaker.voice} onChange={(e) => onChange({ voice: e.target.value })} aria-label="Voice" style={{ flex: 1 }}>
            {!speaker.voice && <option value="">Choose a voice</option>}
            {speaker.voice && !voiceOptions.some((o) => o.id === speaker.voice) && <option value={speaker.voice}>{speaker.voice}</option>}
            {voiceOptions.map((o) => (
              <option key={o.id} value={o.id}>
                {o.label}
              </option>
            ))}
          </select>
        ) : (
          <input value={speaker.voice} onChange={(e) => onChange({ voice: e.target.value })} placeholder="Voice id, e.g. marin" aria-label="Voice" style={{ flex: 1 }} />
        )}
        {onPreview && (
          <button className="button" onClick={() => preview.mutate()} disabled={!speaker.voice || preview.isPending} title="Hear this voice">
            {preview.isPending ? <Spinner /> : <Icon name="play" size={14} />}
          </button>
        )}
        {onRemove && (
          <button className="icon-button" onClick={onRemove} aria-label={`Remove speaker ${index + 1}`}>
            <Icon name="trash" />
          </button>
        )}
      </div>
      <input value={speaker.backstory} onChange={(e) => onChange({ backstory: e.target.value })} placeholder="Background: who they are and what they know" aria-label="Background" />
      <input value={speaker.personality} onChange={(e) => onChange({ personality: e.target.value })} placeholder="Personality: how they talk" aria-label="Personality" />
      {preview.isError && <small className="danger">{errorMessage(preview.error)}</small>}
    </fieldset>
  )
}

import { VOICE_LABELS, WORDS_PER_SEGMENT, type Episode, type PodcastEffort } from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { relativeTime } from '../lib/format.ts'
import { formatTime, usePlayer } from '../lib/player.tsx'
import { useSettings } from '../lib/settings.ts'
import { ClaimCheckLine, useClaimCheck } from './ClaimCheck.tsx'
import { LiveTalk } from './LiveTalk.tsx'
import { profilesKey } from './Profiles.tsx'
import { EmptyState, ErrorNote, Icon, Modal, Spinner, confirmAction } from './ui.tsx'
import { platformFetch } from '../platform/index.ts'
import { saveDownload } from '../platform/media.ts'

const IN_PROGRESS = new Set(['queued', 'researching', 'writing', 'checking', 'revising', 'voicing', 'saving'])

const EFFORTS: Array<{ value: PodcastEffort; label: string; detail: string }> = [
  { value: 'quick', label: 'Quick', detail: 'Writes the script and records it, with no claim check. 1 model pass.' },
  { value: 'standard', label: 'Standard', detail: 'Checks every claim against your sources while the voices record. 2 model passes.' },
  { value: 'careful', label: 'Careful', detail: 'Checks claims, fixes the lines that overstate or stray from your sources, and checks again before recording. Up to 4 model passes.' },
  { value: 'thorough', label: 'Thorough', detail: 'Writes study notes on each source first, including its limits, then up to two rounds of fixes. Up to 7 model passes.' },
]
const EFFORT_KEY = 'podcast-effort'

function savedEffort(): PodcastEffort {
  try {
    const v = localStorage.getItem(EFFORT_KEY)
    if (EFFORTS.some((e) => e.value === v)) return v as PodcastEffort
  } catch {
    // Storage can be unavailable; the default is fine.
  }
  return 'standard'
}

export const episodesKey = (notebookId: string) => ['episodes', notebookId] as const

export function Listen({ notebookId, notebookName, onSettings }: { notebookId: string; notebookName?: string; onSettings: () => void }) {
  const backend = useBackend()
  const episodes = useQuery({
    queryKey: episodesKey(notebookId),
    queryFn: () => backend.listEpisodes(notebookId),
    refetchInterval: (q) => (q.state.data?.some((e) => IN_PROGRESS.has(e.status) || e.check?.status === 'checking') ? 1500 : false),
  })

  return (
    <div className="column listen">
      <Composer notebookId={notebookId} onSettings={onSettings} />
      <LiveTalk notebookId={notebookId} />

      <h3 className="listen-heading">Episodes</h3>
      {episodes.isPending && <Spinner />}
      <ErrorNote error={episodes.error} />
      {episodes.data?.length === 0 && (
        <EmptyState title="No episodes yet">Turn this notebook into something you can listen to on a walk.</EmptyState>
      )}
      <ul className="episodes">
        {episodes.data?.map((e) => (
          <EpisodeRow key={e.id} episode={e} notebookId={notebookId} notebookName={notebookName} />
        ))}
      </ul>
    </div>
  )
}

/** Pick an episode profile and speakers, add a focus, and make the episode. */
function Composer({ notebookId, onSettings }: { notebookId: string; onSettings: () => void }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const settings = useSettings()
  const profiles = useQuery({ queryKey: profilesKey, queryFn: () => backend.podcastProfiles() })
  const [episodeProfile, setEpisodeProfile] = useState('')
  const [speakerProfile, setSpeakerProfile] = useState('')
  const [focus, setFocus] = useState('')
  const [effort, setEffortState] = useState<PodcastEffort>(savedEffort)
  const setEffort = (value: PodcastEffort) => {
    setEffortState(value)
    try {
      localStorage.setItem(EFFORT_KEY, value)
    } catch {
      // Not remembered; still used for this episode.
    }
  }
  const effortIndex = EFFORTS.findIndex((e) => e.value === effort)
  const local = backend.podcastMode === 'local'
  const ep = profiles.data?.episode.find((p) => p.id === episodeProfile) ?? profiles.data?.episode[0]
  const sp = profiles.data?.speaker.find((p) => p.id === (speakerProfile || ep?.speakerProfileId))
  const needsChat = local && !settings.chat?.model

  const create = useMutation({
    mutationFn: () => backend.createEpisode({ notebookId, episodeProfile: ep!.id, speakerProfile: sp?.id, focus, effort: local ? effort : undefined }),
    onSuccess: () => {
      setFocus('')
      void qc.invalidateQueries({ queryKey: episodesKey(notebookId) })
    },
  })

  return (
    <div className="composer-card">
      <div className="composer-row">
        <label className="inline-select">
          <span>Episode</span>
          <select value={ep?.id ?? ''} onChange={(e) => setEpisodeProfile(e.target.value)}>
            {profiles.data?.episode.map((p) => (
              <option key={p.id} value={p.id}>
                {humanize(p.name)}
              </option>
            ))}
          </select>
        </label>
        <label className="inline-select">
          <span>Speakers</span>
          <select value={sp?.id ?? ''} onChange={(e) => setSpeakerProfile(e.target.value)}>
            {profiles.data?.speaker.map((p) => (
              <option key={p.id} value={p.id}>
                {humanize(p.name)}
              </option>
            ))}
          </select>
        </label>
        {ep && <span className="voice-line">about {Math.round((ep.segments * WORDS_PER_SEGMENT) / 150)} min</span>}
      </div>
      <textarea
        className="focus-input"
        rows={2}
        value={focus}
        onChange={(e) => setFocus(e.target.value)}
        placeholder="What should this episode focus on? Optional"
        aria-label="Episode focus"
      />
      {local && (
        <div className="effort">
          <label className="effort-control">
            <span>Checking</span>
            <input
              type="range"
              min={0}
              max={EFFORTS.length - 1}
              step={1}
              value={effortIndex}
              onChange={(e) => setEffort(EFFORTS[Number(e.target.value)].value)}
              aria-valuetext={EFFORTS[effortIndex].label}
            />
            <strong>{EFFORTS[effortIndex].label}</strong>
          </label>
          <p className="effort-detail">{EFFORTS[effortIndex].detail} Each pass reads the notebook material once.</p>
        </div>
      )}
      <div className="composer-row">
        <span className="voice-line">
          {sp ? `${sp.speakers.map((s) => s.name.split(' ')[0]).join(', ')}${sp.voiceKind ? `, ${VOICE_LABELS[sp.voiceKind]}` : ''}` : 'Loading profiles'}
          <button className="link-button" onClick={onSettings}>
            Edit profiles
          </button>
        </span>
        <span className="spacer" />
        <button className="button primary" onClick={() => create.mutate()} disabled={!ep || !sp || needsChat || create.isPending}>
          <Icon name="headphones" size={14} /> {create.isPending ? 'Starting' : 'Create episode'}
        </button>
      </div>
      {sp?.voiceKind === 'openai-live' && ep && (
        <p className="composer-hint-line">
          OpenAI Live records in real time, so this episode takes about {Math.round(((ep.segments * WORDS_PER_SEGMENT) / 150) * 1.5)} minutes to make.
        </p>
      )}
      {needsChat && <p className="composer-hint-line">Choose a chat model in Settings; it writes the script.</p>}
      <ErrorNote error={profiles.error ?? create.error} />
    </div>
  )
}

const humanize = (name: string) => name.replace(/[_-]+/g, ' ').replace(/^\w/, (c) => c.toUpperCase())

const STATUS_TEXT: Record<string, string> = {
  queued: 'Waiting to start',
  researching: 'Taking notes on each source',
  writing: 'Writing the script',
  checking: 'Checking claims against your sources',
  revising: 'Fixing flagged lines',
  voicing: 'Recording voices',
  saving: 'Finishing the audio',
}

function EpisodeRow({ episode: e, notebookId, notebookName }: { episode: Episode; notebookId: string; notebookName?: string }) {
  const backend = useBackend()
  const player = usePlayer()
  const qc = useQueryClient()
  const remove = useMutation({
    mutationFn: () => backend.deleteEpisode(e.id),
    onSuccess: () => {
      if (player.episode?.id === e.id) player.close()
      void qc.invalidateQueries({ queryKey: episodesKey(notebookId) })
    },
  })
  const retry = useMutation({
    mutationFn: () => backend.retryEpisode(e.id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: episodesKey(notebookId) }),
  })
  const download = useMutation({
    mutationFn: async () => {
      const url = await backend.episodeAudioUrl(e.id)
      // Audio saved on this device is an asset: or blob: URL that only the webview's own fetch can read.
      const get = /^https?:/.test(url) && !url.startsWith('http://asset.localhost') ? await platformFetch() : fetch
      const response = await get(url)
      if (!response.ok) throw new Error(`Could not download the audio (${response.status})`)
      const type = response.headers.get('content-type') ?? 'audio/mp4'
      const ext = type.includes('mpeg') ? 'mp3' : type.includes('wav') ? 'wav' : 'm4a'
      await saveDownload(`${e.name.replace(/[\\/:*?"<>|]+/g, ' ').trim()}.${ext}`, await response.arrayBuffer(), type)
    },
  })
  const [showPrompt, setShowPrompt] = useState(false)
  const local = backend.podcastMode === 'local'
  const check = useClaimCheck(e.id, episodesKey(notebookId))
  const busy = IN_PROGRESS.has(e.status)
  const isCurrent = player.episode?.id === e.id
  const progress = e.progress && e.progress.total ? e.progress.done / e.progress.total : undefined

  return (
    <li className={`episode${isCurrent ? ' current' : ''}`}>
      <button
        className="episode-play"
        disabled={e.status !== 'completed'}
        onClick={() => (isCurrent ? player.toggle() : void player.play(e, notebookName))}
        aria-label={isCurrent && player.playing ? `Pause ${e.name}` : `Play ${e.name}`}
      >
        {busy ? <Spinner /> : <Icon name={isCurrent && player.playing ? 'pause' : 'play'} size={16} />}
      </button>
      <div className="episode-main">
        <button className="episode-title" disabled={e.status !== 'completed'} onClick={() => void player.play(e, notebookName).then(() => player.setExpanded(true))}>
          {e.name}
        </button>
        {busy ? (
          <div className="episode-progress">
            <span>
              {STATUS_TEXT[e.status]}
              {e.status === 'voicing' && e.progress ? `, ${e.progress.done} of ${e.progress.total}` : ''}
            </span>
            {progress !== undefined && (
              <div className="bar" aria-hidden>
                <span style={{ transform: `scaleX(${progress})` }} />
              </div>
            )}
          </div>
        ) : e.status === 'failed' ? (
          <div>
            <p className="danger small">{e.error ?? 'This episode could not be made.'}</p>
            <button className="button small" style={{ marginTop: 8 }} onClick={() => retry.mutate()} disabled={retry.isPending}>
              <Icon name="retry" size={14} /> Try again
            </button>
          </div>
        ) : (
          <p className="episode-meta">
            {e.duration ? `${formatTime(e.duration)}, ` : ''}
            {e.chapters.length ? `${e.chapters.length} chapters, ` : ''}
            {e.voiceLabel ? `${e.voiceLabel}, ` : ''}
            {relativeTime(e.created)}
          </p>
        )}
        {e.summary && !busy && e.status !== 'failed' && <p className="episode-summary">{e.summary}</p>}
        {local && e.status === 'completed' && (
          <ClaimCheckLine episode={e} checking={check.isPending} onCheck={() => check.mutate()} />
        )}
        {local && !busy && (
          <button className="link-button episode-prompt" onClick={() => setShowPrompt(true)}>
            See the prompt
          </button>
        )}
        {showPrompt && <PromptDialog episode={e} onClose={() => setShowPrompt(false)} />}
        <ErrorNote error={retry.error ?? download.error ?? remove.error ?? check.error} />
      </div>
      {!busy && e.status === 'completed' && (
        <button className="icon-button" onClick={() => download.mutate()} disabled={download.isPending} aria-label={`Download ${e.name}`} title="Download audio">
          {download.isPending ? <Spinner /> : <Icon name="file" />}
        </button>
      )}
      {!busy && (
        <button className="icon-button" onClick={() => void confirmAction(`Delete “${e.name}”?`).then((ok) => ok && remove.mutate())} aria-label={`Delete ${e.name}`} title="Delete episode">
          <Icon name="trash" />
        </button>
      )}
    </li>
  )
}

/** The exact system and user prompt the script writer received. */
function PromptDialog({ episode, onClose }: { episode: Episode; onClose: () => void }) {
  const backend = useBackend()
  const prompt = useQuery({ queryKey: ['episode-prompt', episode.id], queryFn: () => backend.episodePrompt(episode.id) })
  const [copied, setCopied] = useState(false)
  const p = prompt.data
  const copy = () => {
    if (!p) return
    void navigator.clipboard.writeText(`${p.system}\n\n${p.prompt}`).then(() => setCopied(true))
  }
  return (
    <Modal
      title="Script prompt"
      onClose={onClose}
      wide
      footer={
        <>
          {p && (
            <button className="button" onClick={copy}>
              {copied ? 'Copied' : 'Copy prompt'}
            </button>
          )}
          <button className="button primary" onClick={onClose}>
            Done
          </button>
        </>
      }
    >
      {prompt.isPending && <Spinner />}
      <ErrorNote error={prompt.error} />
      {prompt.data === null && <p className="muted">This episode was made before prompts were saved. Episodes you make from now on keep theirs.</p>}
      {p && (
        <div className="prompt-view">
          <p className="muted">
            Sent to {p.model}, {p.prompt.length.toLocaleString()} characters. The notebook material is included as it was when the episode was made.
          </p>
          <h3>System</h3>
          <pre>{p.system}</pre>
          <h3>Message</h3>
          <pre>{p.prompt}</pre>
        </div>
      )}
    </Modal>
  )
}

import { VOICE_LABELS, WORDS_PER_SEGMENT, type Episode } from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { relativeTime } from '../lib/format.ts'
import { formatTime, usePlayer } from '../lib/player.tsx'
import { useSettings } from '../lib/settings.ts'
import { LiveTalk } from './LiveTalk.tsx'
import { profilesKey } from './Profiles.tsx'
import { EmptyState, ErrorNote, Icon, Spinner, confirmAction } from './ui.tsx'
import { platformFetch } from '../platform/index.ts'
import { saveDownload } from '../platform/media.ts'

const IN_PROGRESS = new Set(['queued', 'writing', 'voicing', 'saving'])

export const episodesKey = (notebookId: string) => ['episodes', notebookId] as const

export function Listen({ notebookId, notebookName, onSettings }: { notebookId: string; notebookName?: string; onSettings: () => void }) {
  const backend = useBackend()
  const episodes = useQuery({
    queryKey: episodesKey(notebookId),
    queryFn: () => backend.listEpisodes(notebookId),
    refetchInterval: (q) => (q.state.data?.some((e) => IN_PROGRESS.has(e.status)) ? 1500 : false),
  })

  return (
    <div className="column listen">
      <Composer notebookId={notebookId} onSettings={onSettings} />
      <LiveTalk />

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
  const ep = profiles.data?.episode.find((p) => p.id === episodeProfile) ?? profiles.data?.episode[0]
  const sp = profiles.data?.speaker.find((p) => p.id === (speakerProfile || ep?.speakerProfileId))
  const needsChat = backend.podcastMode === 'local' && !settings.chat?.model

  const create = useMutation({
    mutationFn: () => backend.createEpisode({ notebookId, episodeProfile: ep!.id, speakerProfile: sp?.id, focus }),
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
  writing: 'Writing the script',
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
      const response = await (await platformFetch())(url)
      if (!response.ok) throw new Error(`Could not download the audio (${response.status})`)
      const type = response.headers.get('content-type') ?? 'audio/mp4'
      const ext = type.includes('mpeg') ? 'mp3' : type.includes('wav') ? 'wav' : 'm4a'
      await saveDownload(`${e.name.replace(/[\\/:*?"<>|]+/g, ' ').trim()}.${ext}`, await response.arrayBuffer(), type)
    },
  })
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
        <ErrorNote error={retry.error ?? download.error ?? remove.error} />
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

'use client'

import { createContext, useContext, useEffect, useRef, useState, type CSSProperties } from 'react'
import { useMutation } from '@tanstack/react-query'
import { toast } from 'sonner'
import { Download, Pause, Play, RotateCcw, RotateCw, X } from 'lucide-react'
import apiClient from '@/lib/api/client'
import { resolvePodcastAssetUrl } from '@/lib/api/podcasts'
import { PodcastEpisode } from '@/lib/types/podcasts'
import { useTranslation } from '@/lib/hooks/use-translation'
import { Button } from '@/components/ui/button'

type Episode = Pick<PodcastEpisode, 'id' | 'name'>
type Player = {
  episode: Episode | null
  playing: boolean
  play: (episode: Episode) => Promise<void>
  download: (episode: Episode) => Promise<void>
}
const PlayerContext = createContext<Player | null>(null)
function usePlayer() {
  const context = useContext(PlayerContext)
  if (!context) throw new Error('PodcastPlayerProvider is required')
  return context
}
export function formatTime(seconds: number) {
  const value = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0))
  const hours = Math.floor(value / 3600)
  return `${hours ? `${hours}:` : ''}${hours ? String(Math.floor(value / 60) % 60).padStart(2, '0') : Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}`
}

export function PodcastPlayerProvider({ children }: { children: React.ReactNode }) {
  const { t } = useTranslation()
  const audio = useRef<HTMLAudioElement>(null)
  const generation = useRef(0)
  const [episode, setEpisode] = useState<Episode | null>(null)
  const [playing, setPlaying] = useState(false)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState(false)
  const [time, setTime] = useState(0)
  const [duration, setDuration] = useState(0)
  const [buffered, setBuffered] = useState(0)
  const [scrub, setScrub] = useState<number | null>(null)
  const [speed, setSpeed] = useState(1)
  const access = useMutation({
    mutationFn: async (id: string) => {
      const { data } = await apiClient.post<{ url: string }>(`/podcasts/episodes/${encodeURIComponent(id)}/audio-access`)
      const url = await resolvePodcastAssetUrl(data.url)
      if (!url) throw new Error('Missing media URL')
      return url
    },
    retry: false,
  })
  const resume = async () => {
    try { await audio.current?.play() } catch { setLoading(false); setError(true) }
  }
  const play = async (next: Episode) => {
    if (episode?.id === next.id && audio.current?.getAttribute('src') && !error) {
      if (audio.current.paused) await resume()
      else audio.current.pause()
      return
    }
    const version = ++generation.current
    audio.current?.pause()
    audio.current?.removeAttribute('src')
    audio.current?.load()
    setEpisode(next); setError(false); setLoading(true); setTime(0); setDuration(0); setBuffered(0); setScrub(null)
    try {
      const url = await access.mutateAsync(next.id)
      if (version !== generation.current || !audio.current) return
      audio.current.src = url
      audio.current.playbackRate = speed
      await resume()
    } catch { if (version === generation.current) { setLoading(false); setError(true) } }
  }
  const download = async (item: Episode) => {
    try {
      const url = await access.mutateAsync(item.id)
      const link = document.createElement('a')
      link.href = `${url}&download=true`
      link.download = `${item.name}.mp3`
      link.referrerPolicy = 'no-referrer'
      document.body.appendChild(link); link.click(); link.remove()
    } catch { toast.error(t('podcasts.audioUnavailable')) }
  }
  const seek = (value: number) => {
    if (!audio.current || !Number.isFinite(audio.current.duration)) return
    audio.current.currentTime = Math.max(0, Math.min(audio.current.duration, value))
    setTime(audio.current.currentTime)
  }
  const close = () => {
    generation.current++
    audio.current?.pause(); audio.current?.removeAttribute('src'); audio.current?.load()
    setEpisode(null); setLoading(false); setError(false)
  }
  useEffect(() => {
    const element = audio.current
    return () => { generation.current++; element?.pause(); element?.removeAttribute('src'); element?.load() }
  }, [])
  useEffect(() => {
    if (!('mediaSession' in navigator)) return
    navigator.mediaSession.metadata = episode ? new MediaMetadata({ title: episode.name }) : null
    navigator.mediaSession.playbackState = episode ? (playing ? 'playing' : 'paused') : 'none'
    const handlers: [MediaSessionAction, MediaSessionActionHandler][] = [
      ['play', () => { void audio.current?.play().catch(() => setError(true)) }],
      ['pause', () => audio.current?.pause()],
      ['seekbackward', (event) => seek((audio.current?.currentTime || 0) - (event.seekOffset || 15))],
      ['seekforward', (event) => seek((audio.current?.currentTime || 0) + (event.seekOffset || 15))],
      ['seekto', (event) => { if (event.seekTime !== undefined) seek(event.seekTime) }],
    ]
    for (const [action, handler] of handlers) { try { navigator.mediaSession.setActionHandler(action, handler) } catch { /* Optional browser capability. */ } }
    return () => { for (const [action] of handlers) { try { navigator.mediaSession.setActionHandler(action, null) } catch { /* Optional browser capability. */ } } }
  }, [episode, playing])
  return <PlayerContext.Provider value={{ episode, playing, play, download }}>
    <div style={{ '--podcast-player-height': episode ? '164px' : '0px' } as CSSProperties}>{children}</div>
    <audio ref={audio} preload="metadata" aria-label={t('player.audio')} onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onPlaying={() => { setLoading(false); setError(false) }} onWaiting={() => setLoading(true)} onCanPlay={() => setLoading(false)} onEnded={() => { setPlaying(false); setLoading(false) }} onError={() => { if (audio.current?.getAttribute('src')) { setError(true); setLoading(false) } }} onTimeUpdate={() => setTime(audio.current?.currentTime || 0)} onDurationChange={() => { const value = audio.current?.duration; setDuration(value && Number.isFinite(value) ? value : 0) }} onProgress={() => { const ranges = audio.current?.buffered; if (ranges?.length) setBuffered(ranges.end(ranges.length - 1)) }} />
    {episode && <section aria-label={t('player.title')} className="fixed inset-x-0 bottom-0 z-50 h-[164px] border-t bg-card px-4 py-3 shadow-lg" style={{ paddingBottom: 'max(12px, env(safe-area-inset-bottom))' }}>
      <div className="mx-auto flex max-w-5xl flex-col gap-2">
        <div className="flex items-center gap-3"><div className="min-w-0 flex-1"><p className="truncate text-sm font-semibold">{episode.name}</p><p className="text-xs text-muted-foreground" role="status">{error ? t('player.failed') : loading ? t('player.buffering') : t('player.keepListening')}</p></div><Button size="icon" variant="ghost" title={t('player.download')} aria-label={t('player.download')} onClick={() => void download(episode)}><Download className="size-4" /></Button><Button size="icon" variant="ghost" aria-label={t('player.close')} onClick={close}><X className="size-4" /></Button></div>
        <div className="flex items-center gap-3"><span className="w-12 text-right font-mono text-xs tabular-nums">{formatTime(scrub ?? time)}</span><div className="relative flex flex-1 items-center"><div aria-hidden className="pointer-events-none absolute inset-x-0 h-1 rounded bg-muted"><div className="h-full rounded bg-foreground/20" style={{ width: `${duration ? Math.min(100, buffered / duration * 100) : 0}%` }} /></div><input className="relative h-6 w-full cursor-pointer accent-primary" type="range" min={0} max={duration || 1} step={0.1} value={scrub ?? Math.min(time, duration)} disabled={!duration} aria-label={t('player.seek')} aria-valuetext={`${formatTime(scrub ?? time)} / ${formatTime(duration)}`} onChange={e => setScrub(Number(e.target.value))} onPointerUp={e => { seek(Number(e.currentTarget.value)); setScrub(null) }} onKeyUp={e => { seek(Number(e.currentTarget.value)); setScrub(null) }} onBlur={e => { if (scrub !== null) { seek(Number(e.currentTarget.value)); setScrub(null) } }} /></div><span className="w-12 font-mono text-xs tabular-nums">{formatTime(duration)}</span></div>
        <div className="flex items-center justify-center gap-4"><Button size="icon" variant="ghost" aria-label={t('player.back')} onClick={() => seek(time - 15)} disabled={!duration}><RotateCcw className="size-5" /></Button><Button size="icon" className="rounded-full" aria-label={playing ? t('player.pause') : t('player.play')} onClick={() => void play(episode)}>{playing ? <Pause className="size-5" /> : <Play className="size-5" />}</Button><Button size="icon" variant="ghost" aria-label={t('player.forward')} onClick={() => seek(time + 15)} disabled={!duration}><RotateCw className="size-5" /></Button><select aria-label={t('player.speed')} className="rounded-md border bg-background p-1 text-sm" value={speed} onChange={e => { const rate = Number(e.target.value); setSpeed(rate); if (audio.current) audio.current.playbackRate = rate }}>{[0.75, 1, 1.25, 1.5, 1.75, 2].map(rate => <option key={rate} value={rate}>{rate}×</option>)}</select></div>
      </div>
    </section>}
  </PlayerContext.Provider>
}

export function EpisodePlayback({ episode }: { episode: PodcastEpisode }) {
  const player = usePlayer()
  const { t } = useTranslation()
  if (!episode.audio_url && !episode.audio_file) return null
  const active = player.episode?.id === episode.id && player.playing
  return <div className="flex items-center gap-2"><Button variant="outline" onClick={() => void player.play(episode)}>{active ? <Pause className="mr-2 size-4" /> : <Play className="mr-2 size-4" />}{t(active ? 'player.pause' : 'player.play')}</Button><Button variant="ghost" onClick={() => void player.download(episode)}><Download className="mr-2 size-4" />{t('player.download')}</Button></div>
}

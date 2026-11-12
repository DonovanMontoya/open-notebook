import type { Episode } from '@open-notebook/engine'
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'

import { useBackend } from './backend.tsx'

export const SPEEDS = [0.8, 1, 1.1, 1.2, 1.35, 1.5, 1.75, 2]
export const SKIP_BACK = 15
export const SKIP_FORWARD = 30

export type Sleep = { kind: 'off' } | { kind: 'chapter' } | { kind: 'minutes'; endsAt: number; minutes: number }

interface PlayerState {
  episode?: Episode
  notebookName?: string
  playing: boolean
  loading: boolean
  error?: string
  time: number
  duration: number
  rate: number
  volume: number
  sleep: Sleep
  expanded: boolean
}

interface PlayerApi extends PlayerState {
  play: (episode: Episode, notebookName?: string) => Promise<void>
  toggle: () => void
  seek: (seconds: number) => void
  skip: (delta: number) => void
  setRate: (rate: number) => void
  setVolume: (volume: number) => void
  setSleep: (sleep: Sleep) => void
  chapterIndex: number
  previousChapter: () => void
  nextChapter: () => void
  setExpanded: (expanded: boolean) => void
  close: () => void
}

const PlayerContext = createContext<PlayerApi | null>(null)

const POSITION_KEY = (id: string) => `open-notebook-position:${id}`
const RATE_KEY = 'open-notebook-rate'

/** The chapter playing at a time, or -1 before the first. */
export function chapterAt(episode: Episode | undefined, time: number): number {
  const chapters = episode?.chapters ?? []
  let index = -1
  for (let i = 0; i < chapters.length; i++) if (chapters[i].start <= time + 0.05) index = i
  return index
}

/**
 * Line start times for the transcript. Measured times are used as they are;
 * episodes without them get times spread by text length over the duration.
 */
export function lineTimes(episode: Episode | undefined, duration: number): Array<{ start: number; end: number }> {
  const lines = episode?.transcript ?? []
  if (lines.every((l) => l.start !== undefined && l.end !== undefined)) {
    return lines.map((l) => ({ start: l.start!, end: l.end! }))
  }
  const total = lines.reduce((n, l) => n + l.text.length, 0) || 1
  let at = 0
  return lines.map((l) => {
    const span = (duration * l.text.length) / total
    const t = { start: at, end: at + span }
    at += span
    return t
  })
}

export function PlayerProvider({ children }: { children: ReactNode }) {
  const backend = useBackend()
  const audio = useRef<HTMLAudioElement>(null)
  const [state, setState] = useState<PlayerState>(() => ({
    playing: false,
    loading: false,
    time: 0,
    duration: 0,
    rate: Number(localStorage.getItem(RATE_KEY)) || 1,
    volume: 1,
    sleep: { kind: 'off' },
    expanded: false,
  }))
  const stateRef = useRef(state)
  stateRef.current = state
  const patch = useCallback((p: Partial<PlayerState>) => setState((s) => ({ ...s, ...p })), [])

  // Smooth position while playing: timeupdate fires only a few times a second.
  useEffect(() => {
    if (!state.playing) return
    let frame = 0
    const tick = () => {
      const el = audio.current
      if (el) setState((s) => (Math.abs(s.time - el.currentTime) > 0.03 ? { ...s, time: el.currentTime } : s))
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [state.playing])

  // Remember where each episode was left, every few seconds and on pause.
  useEffect(() => {
    const id = state.episode?.id
    if (!id) return
    const save = () => {
      const el = audio.current
      if (!el || !el.duration) return
      if (el.currentTime > el.duration - 10) localStorage.removeItem(POSITION_KEY(id))
      else localStorage.setItem(POSITION_KEY(id), String(Math.floor(el.currentTime)))
    }
    const timer = setInterval(save, 5000)
    return () => {
      clearInterval(timer)
      save()
    }
  }, [state.episode?.id])

  // Sleep timer.
  useEffect(() => {
    const sleep = state.sleep
    if (sleep.kind === 'minutes') {
      const timer = setTimeout(() => {
        audio.current?.pause()
        patch({ sleep: { kind: 'off' } })
      }, Math.max(0, sleep.endsAt - Date.now()))
      return () => clearTimeout(timer)
    }
  }, [state.sleep, patch])
  const chapterIndex = chapterAt(state.episode, state.time)
  const lastChapter = useRef(chapterIndex)
  useEffect(() => {
    if (state.sleep.kind === 'chapter' && lastChapter.current !== chapterIndex && lastChapter.current >= 0) {
      audio.current?.pause()
      patch({ sleep: { kind: 'off' } })
    }
    lastChapter.current = chapterIndex
  }, [chapterIndex, state.sleep.kind, patch])

  const play = useCallback(
    async (episode: Episode, notebookName?: string) => {
      const el = audio.current
      if (!el) return
      if (stateRef.current.episode?.id === episode.id) {
        patch({ episode, notebookName: notebookName ?? stateRef.current.notebookName })
        if (el.paused) await el.play().catch(() => undefined)
        return
      }
      patch({ episode, notebookName, loading: true, error: undefined, time: 0, duration: episode.duration ?? 0 })
      try {
        el.src = await backend.episodeAudioUrl(episode.id)
        el.playbackRate = stateRef.current.rate
        const saved = Number(localStorage.getItem(POSITION_KEY(episode.id)))
        if (saved > 0) el.currentTime = saved
        await el.play()
      } catch (error) {
        patch({ loading: false, error: error instanceof Error ? error.message : String(error) })
      }
    },
    [backend, patch],
  )

  const api = useMemo<PlayerApi>(() => {
    const el = () => audio.current
    const seek = (seconds: number) => {
      const a = el()
      if (!a) return
      const max = Number.isFinite(a.duration) ? a.duration : stateRef.current.duration
      a.currentTime = Math.max(0, Math.min(max || 0, seconds))
      patch({ time: a.currentTime })
    }
    const chapters = state.episode?.chapters ?? []
    return {
      ...state,
      chapterIndex,
      play,
      toggle: () => {
        const a = el()
        if (!a?.src) return
        if (a.paused) void a.play()
        else a.pause()
      },
      seek,
      skip: (delta) => seek((el()?.currentTime ?? 0) + delta),
      setRate: (rate) => {
        const a = el()
        if (a) a.playbackRate = rate
        localStorage.setItem(RATE_KEY, String(rate))
        patch({ rate })
      },
      setVolume: (volume) => {
        const a = el()
        if (a) a.volume = volume
        patch({ volume })
      },
      setSleep: (sleep) => patch({ sleep }),
      previousChapter: () => {
        const t = el()?.currentTime ?? 0
        const current = chapters[chapterIndex]
        // Like a CD player: restart this chapter unless you are at its very start.
        if (current && t - current.start > 3) seek(current.start)
        else seek(chapters[Math.max(0, chapterIndex - 1)]?.start ?? 0)
      },
      nextChapter: () => {
        const next = chapters[chapterIndex + 1]
        if (next) seek(next.start)
      },
      setExpanded: (expanded) => patch({ expanded }),
      close: () => {
        const a = el()
        if (a) {
          a.pause()
          a.removeAttribute('src')
          a.load()
        }
        patch({ episode: undefined, playing: false, expanded: false, time: 0, duration: 0, sleep: { kind: 'off' } })
      },
    }
  }, [state, chapterIndex, play, patch])

  // macOS Now Playing, media keys and headphone controls.
  useEffect(() => {
    const session = navigator.mediaSession
    if (!session || !state.episode) return
    session.metadata = new MediaMetadata({
      title: state.episode.name,
      artist: state.notebookName ?? 'Open Notebook',
      album: state.episode.chapters[chapterIndex]?.title ?? 'Open Notebook',
    })
    const handlers: Array<[MediaSessionAction, MediaSessionActionHandler]> = [
      ['play', () => api.toggle()],
      ['pause', () => api.toggle()],
      ['seekbackward', (d) => api.skip(-(d.seekOffset ?? SKIP_BACK))],
      ['seekforward', (d) => api.skip(d.seekOffset ?? SKIP_FORWARD)],
      ['seekto', (d) => d.seekTime !== undefined && api.seek(d.seekTime)],
      ['previoustrack', () => api.previousChapter()],
      ['nexttrack', () => api.nextChapter()],
    ]
    for (const [action, handler] of handlers) {
      try {
        session.setActionHandler(action, handler)
      } catch {
        // Unsupported action on this platform.
      }
    }
  }, [api, state.episode, state.notebookName, chapterIndex])

  useEffect(() => {
    const session = navigator.mediaSession
    if (!session?.setPositionState || !state.duration) return
    try {
      session.setPositionState({ duration: state.duration, playbackRate: state.rate, position: Math.min(state.time, state.duration) })
    } catch {
      // Ignore transient invalid states while seeking.
    }
  }, [Math.floor(state.time), state.duration, state.rate])

  // Keyboard: space, arrows and speed, unless you are typing.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!stateRef.current.episode) return
      const target = e.target as HTMLElement
      if (target.closest('input, textarea, select, [contenteditable="true"]')) return
      if (e.metaKey || e.ctrlKey || e.altKey) return
      if (e.key === ' ') {
        e.preventDefault()
        api.toggle()
      } else if (e.key === 'ArrowLeft') api.skip(-SKIP_BACK)
      else if (e.key === 'ArrowRight') api.skip(SKIP_FORWARD)
      else if (e.key === '>' || e.key === '.') api.setRate(SPEEDS[Math.min(SPEEDS.length - 1, SPEEDS.indexOf(api.rate) + 1)] ?? api.rate)
      else if (e.key === '<' || e.key === ',') api.setRate(SPEEDS[Math.max(0, SPEEDS.indexOf(api.rate) - 1)] ?? api.rate)
      else if (e.key === 'Escape' && stateRef.current.expanded) patch({ expanded: false })
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [api, patch])

  return (
    <PlayerContext.Provider value={api}>
      {children}
      <audio
        ref={audio}
        preload="auto"
        onPlay={() => patch({ playing: true, loading: false })}
        onPause={() => patch({ playing: false })}
        onWaiting={() => patch({ loading: true })}
        onPlaying={() => patch({ loading: false })}
        onLoadedMetadata={(e) => patch({ duration: e.currentTarget.duration || stateRef.current.duration })}
        onTimeUpdate={(e) => !stateRef.current.playing && patch({ time: e.currentTarget.currentTime })}
        onEnded={() => {
          const id = stateRef.current.episode?.id
          if (id) localStorage.removeItem(POSITION_KEY(id))
          patch({ playing: false })
        }}
        onError={() => patch({ loading: false, playing: false, error: 'This episode’s audio could not be played.' })}
      />
    </PlayerContext.Provider>
  )
}

export function usePlayer(): PlayerApi {
  const api = useContext(PlayerContext)
  if (!api) throw new Error('PlayerProvider missing')
  return api
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) seconds = 0
  const s = Math.floor(seconds)
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = String(s % 60).padStart(2, '0')
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`
}

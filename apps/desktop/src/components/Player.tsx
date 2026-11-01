import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'

import { formatTime, lineTimes, SKIP_BACK, SKIP_FORWARD, SPEEDS, usePlayer, type Sleep } from '../lib/player.tsx'
import { Icon, Menu, SkipIcon, Spinner } from './ui.tsx'

function PlayGlyph({ playing, size = 18 }: { playing: boolean; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden fill="currentColor">
      {playing ? (
        <>
          <rect x="6" y="4.5" width="4" height="15" rx="1.2" />
          <rect x="14" y="4.5" width="4" height="15" rx="1.2" />
        </>
      ) : (
        <path d="M8 4.8v14.4c0 .8.86 1.28 1.54.86l11.5-7.2a1 1 0 0 0 0-1.72L9.54 3.94C8.86 3.52 8 4 8 4.8z" />
      )}
    </svg>
  )
}

/** Docked player that follows you around the app. */
export function MiniPlayer() {
  const p = usePlayer()
  if (!p.episode || p.expanded) return null
  const duration = p.duration || p.episode.duration || 0
  const chapter = p.episode.chapters[p.chapterIndex]
  return (
    <div className="mini-player" role="region" aria-label="Now playing">
      <button className="mini-play" onClick={p.toggle} aria-label={p.playing ? 'Pause' : 'Play'}>
        {p.loading ? <Spinner /> : <PlayGlyph playing={p.playing} size={16} />}
      </button>
      <button className="mini-info" onClick={() => p.setExpanded(true)} title="Open player">
        <span className="mini-title">{p.episode.name}</span>
        <span className="mini-meta">
          {chapter ? chapter.title : p.notebookName}
          <span className="mini-time">{formatTime(Math.max(0, duration - p.time))} left</span>
        </span>
      </button>
      <div className="mini-progress" aria-hidden>
        <span style={{ transform: `scaleX(${duration ? p.time / duration : 0})` }} />
      </div>
    </div>
  )
}

/** Chapter-segmented scrubber with a hover and drag preview. */
function Scrubber({ time, duration, chapters, onSeek }: { time: number; duration: number; chapters: Array<{ title: string; start: number }>; onSeek: (t: number) => void }) {
  const ref = useRef<HTMLDivElement>(null)
  const [hover, setHover] = useState<number>()
  const [dragging, setDragging] = useState<number>()
  const shown = dragging ?? time
  const segments = useMemo(() => {
    if (!duration) return [{ start: 0, end: 1 }]
    const starts = chapters.length ? chapters.map((c) => c.start) : [0]
    return starts.map((start, i) => ({ start, end: starts[i + 1] ?? duration }))
  }, [chapters, duration])

  const timeAt = (clientX: number) => {
    const rect = ref.current!.getBoundingClientRect()
    return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width)) * (duration || 0)
  }
  const preview = dragging ?? hover
  const previewChapter = preview !== undefined ? chapters.filter((c) => c.start <= preview + 0.05).at(-1)?.title : undefined

  return (
    <div className="scrubber">
      <div
        ref={ref}
        className={`scrub-track${dragging !== undefined ? ' dragging' : ''}`}
        role="slider"
        tabIndex={0}
        aria-label="Position"
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(time)}
        aria-valuetext={`${formatTime(time)} of ${formatTime(duration)}`}
        onKeyDown={(e) => {
          if (e.key === 'ArrowLeft') onSeek(time - 5)
          if (e.key === 'ArrowRight') onSeek(time + 5)
        }}
        onPointerMove={(e) => setHover(timeAt(e.clientX))}
        onPointerLeave={() => setHover(undefined)}
        onPointerDown={(e: ReactPointerEvent) => {
          e.currentTarget.setPointerCapture(e.pointerId)
          setDragging(timeAt(e.clientX))
        }}
        onPointerMoveCapture={(e) => dragging !== undefined && setDragging(timeAt(e.clientX))}
        onPointerUp={(e) => {
          if (dragging !== undefined) onSeek(timeAt(e.clientX))
          setDragging(undefined)
        }}
      >
        {segments.map((seg, i) => {
          const fill = duration ? Math.max(0, Math.min(1, (shown - seg.start) / (seg.end - seg.start || 1))) : 0
          return (
            <div key={i} className="scrub-segment" style={{ flexGrow: Math.max(0.001, seg.end - seg.start) }}>
              <span style={{ transform: `scaleX(${fill})` }} />
            </div>
          )
        })}
        {preview !== undefined && duration > 0 && (
          <div className="scrub-tip" style={{ left: `${(preview / duration) * 100}%` }}>
            {formatTime(preview)}
            {previewChapter && <span>{previewChapter}</span>}
          </div>
        )}
      </div>
      <div className="scrub-times">
        <span>{formatTime(shown)}</span>
        <span>−{formatTime(Math.max(0, duration - shown))}</span>
      </div>
    </div>
  )
}

const SLEEP_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'chapter', label: 'End of chapter' },
  { value: '15', label: '15 minutes' },
  { value: '30', label: '30 minutes' },
  { value: '60', label: '1 hour' },
]

function sleepValue(sleep: Sleep): string {
  return sleep.kind === 'minutes' ? String(sleep.minutes) : sleep.kind
}

function sleepLabel(sleep: Sleep, now: number): string {
  if (sleep.kind === 'chapter') return 'End of chapter'
  if (sleep.kind === 'minutes') return `${Math.max(1, Math.ceil((sleep.endsAt - now) / 60000))} min`
  return 'Sleep'
}

/** Full player: the episode card, transport, and a transcript that follows along. */
export function NowPlaying() {
  const p = usePlayer()
  const [tab, setTab] = useState<'transcript' | 'chapters'>('transcript')
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    if (p.sleep.kind !== 'minutes') return
    const t = setInterval(() => setNow(Date.now()), 15000)
    return () => clearInterval(t)
  }, [p.sleep.kind])

  if (!p.episode || !p.expanded) return null
  const e = p.episode
  const duration = p.duration || e.duration || 0

  return (
    <section className="now-playing" aria-label="Now playing">
      <div className="drag" data-tauri-drag-region />
      <header className="np-header" data-tauri-drag-region>
        <button className="icon-button" onClick={() => p.setExpanded(false)} aria-label="Minimize player" title="Minimize (Esc)">
          <Icon name="chevron-down" />
        </button>
        <span className="np-context">{p.notebookName}</span>
        <button className="button quiet small" onClick={p.close}>
          Stop
        </button>
      </header>

      <div className="np-body">
        <div className="np-stage">
          <div className="sleeve">
            <span className="sleeve-mark">Open Notebook</span>
            <h2 className="sleeve-title">{e.name}</h2>
            <span className="sleeve-meta">
              {e.chapters.length ? `${e.chapters.length} chapters, ` : ''}
              {formatTime(duration)}
            </span>
          </div>

          <div className="np-now">
            <p className="np-chapter">{e.chapters[p.chapterIndex]?.title ?? e.summary ?? ' '}</p>
            {e.voiceLabel && <p className="np-voices">{e.voiceLabel}</p>}
          </div>

          <Scrubber time={p.time} duration={duration} chapters={e.chapters} onSeek={p.seek} />

          <div className="transport">
            <button className="icon-button transport-small" onClick={p.previousChapter} disabled={!e.chapters.length} aria-label="Previous chapter" title="Previous chapter">
              <Icon name="chapter-back" size={18} />
            </button>
            <button className="icon-button transport-skip" onClick={() => p.skip(-SKIP_BACK)} aria-label={`Back ${SKIP_BACK} seconds`} title={`Back ${SKIP_BACK} seconds (←)`}>
              <SkipIcon seconds={SKIP_BACK} />
            </button>
            <button className="transport-play" onClick={p.toggle} aria-label={p.playing ? 'Pause' : 'Play'} title="Play or pause (Space)">
              {p.loading ? <Spinner /> : <PlayGlyph playing={p.playing} size={26} />}
            </button>
            <button className="icon-button transport-skip" onClick={() => p.skip(SKIP_FORWARD)} aria-label={`Forward ${SKIP_FORWARD} seconds`} title={`Forward ${SKIP_FORWARD} seconds (→)`}>
              <SkipIcon seconds={SKIP_FORWARD} forward />
            </button>
            <button className="icon-button transport-small" onClick={p.nextChapter} disabled={p.chapterIndex >= e.chapters.length - 1} aria-label="Next chapter" title="Next chapter">
              <Icon name="chapter-next" size={18} />
            </button>
          </div>

          <div className="np-options">
            <Menu
              title="Playback speed"
              label={<span className="tabular">{p.rate}×</span>}
              value={p.rate}
              options={SPEEDS.map((s) => ({ value: s, label: s === 1 ? '1× normal' : `${s}×` }))}
              onChange={p.setRate}
            />
            <Menu
              title="Sleep timer"
              icon="moon"
              label={sleepLabel(p.sleep, now)}
              value={sleepValue(p.sleep)}
              options={SLEEP_OPTIONS}
              onChange={(v) =>
                p.setSleep(
                  v === 'off'
                    ? { kind: 'off' }
                    : v === 'chapter'
                      ? { kind: 'chapter' }
                      : { kind: 'minutes', minutes: Number(v), endsAt: Date.now() + Number(v) * 60000 },
                )
              }
            />
            <label className="volume" title="Volume">
              <Icon name="volume" size={14} />
              <input type="range" min={0} max={1} step={0.01} value={p.volume} onChange={(ev) => p.setVolume(Number(ev.target.value))} aria-label="Volume" />
            </label>
          </div>
          {p.error && <p className="error-note">{p.error}</p>}
        </div>

        <div className="np-side">
          <div className="segmented" role="tablist">
            <button role="tab" aria-selected={tab === 'transcript'} className={tab === 'transcript' ? 'active' : ''} onClick={() => setTab('transcript')}>
              Transcript
            </button>
            <button role="tab" aria-selected={tab === 'chapters'} className={tab === 'chapters' ? 'active' : ''} onClick={() => setTab('chapters')}>
              Chapters
            </button>
          </div>
          {tab === 'transcript' ? <Transcript /> : <Chapters />}
        </div>
      </div>
    </section>
  )
}

function Chapters() {
  const p = usePlayer()
  const chapters = p.episode?.chapters ?? []
  if (!chapters.length) return <p className="muted np-empty">This episode has no chapters.</p>
  return (
    <ol className="chapters">
      {chapters.map((c, i) => (
        <li key={i}>
          <button className={i === p.chapterIndex ? 'current' : ''} onClick={() => p.seek(c.start)}>
            <span className="chapter-title">{c.title}</span>
            <span className="chapter-time">{formatTime(c.start)}</span>
          </button>
        </li>
      ))}
    </ol>
  )
}

/** Follows playback, lighting the current line; scrolling yourself pauses the follow for a moment. */
function Transcript() {
  const p = usePlayer()
  const duration = p.duration || p.episode?.duration || 0
  const times = useMemo(() => lineTimes(p.episode, duration), [p.episode, duration])
  const lines = p.episode?.transcript ?? []
  const current = times.findIndex((t) => p.time >= t.start && p.time < t.end + 0.3)
  const box = useRef<HTMLDivElement>(null)
  const userScrolled = useRef(0)
  const [following, setFollowing] = useState(true)

  useEffect(() => {
    if (!following || current < 0) return
    // Scroll only the transcript itself, never the page around it.
    const container = box.current
    const el = container?.querySelector<HTMLElement>(`[data-line="${current}"]`)
    if (!container || !el) return
    const target = el.offsetTop - container.clientHeight / 2 + el.offsetHeight / 2
    container.scrollTo({ top: Math.max(0, target), behavior: 'smooth' })
  }, [current, following])

  if (!lines.length) return <p className="muted np-empty">No transcript for this episode.</p>

  const chapters = p.episode?.chapters ?? []
  return (
    <div className="transcript-wrap">
      <div
        className="transcript"
        ref={box}
        onWheel={() => {
          userScrolled.current = Date.now()
          setFollowing(false)
        }}
        onTouchMove={() => setFollowing(false)}
      >
        {lines.map((line, i) => {
          const chapterStart = chapters.findIndex((c) => Math.abs(c.start - times[i].start) < 0.05)
          return (
            <div key={i}>
              {chapterStart >= 0 && <p className="transcript-chapter">{chapters[chapterStart].title}</p>}
              <button
                data-line={i}
                className={`line${i === current ? ' current' : ''}${current >= 0 && i < current ? ' past' : ''}`}
                onClick={() => {
                  p.seek(times[i].start)
                  setFollowing(true)
                }}
              >
                <span className="line-speaker">{line.speaker}</span>
                <span className="line-text">{line.text}</span>
              </button>
            </div>
          )
        })}
      </div>
      {!following && (
        <button className="button small follow" onClick={() => setFollowing(true)}>
          Follow along
        </button>
      )}
      {!p.episode?.exactTiming && <p className="transcript-note">Line timing is estimated for this episode</p>}
    </div>
  )
}

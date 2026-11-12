// YouTube captions, read from the watch page the way the server's
// youtube-transcript-api does: find the caption tracks, prefer the user's
// languages, and turn the timed text into paragraphs.

export function youtubeId(url: string): string | undefined {
  try {
    const u = new URL(url)
    if (u.hostname === 'youtu.be') return u.pathname.slice(1) || undefined
    if (/(^|\.)youtube\.com$/.test(u.hostname)) {
      if (u.pathname === '/watch') return u.searchParams.get('v') ?? undefined
      const m = /^\/(shorts|embed|live)\/([\w-]{6,})/.exec(u.pathname)
      if (m) return m[2]
    }
  } catch {
    // Not a URL.
  }
  return undefined
}

export interface CaptionTrack {
  baseUrl: string
  languageCode: string
  name?: string
  /** "asr" for automatic captions. */
  kind?: string
}

/** Pulls ytInitialPlayerResponse out of a watch page. */
export function parseWatchPage(html: string): { title: string; tracks: CaptionTrack[] } {
  const marker = html.indexOf('ytInitialPlayerResponse')
  if (marker < 0) throw new Error('This YouTube page could not be read.')
  const start = html.indexOf('{', marker)
  let depth = 0
  let end = start
  let inString = false
  for (; end < html.length; end++) {
    const c = html[end]
    if (inString) {
      if (c === '\\') end++
      else if (c === '"') inString = false
    } else if (c === '"') inString = true
    else if (c === '{') depth++
    else if (c === '}' && --depth === 0) break
  }
  const player = JSON.parse(html.slice(start, end + 1)) as {
    videoDetails?: { title?: string }
    captions?: { playerCaptionsTracklistRenderer?: { captionTracks?: Array<{ baseUrl: string; languageCode: string; name?: { simpleText?: string }; kind?: string }> } }
  }
  const tracks = (player.captions?.playerCaptionsTracklistRenderer?.captionTracks ?? []).map((t) => ({
    baseUrl: t.baseUrl,
    languageCode: t.languageCode,
    name: t.name?.simpleText,
    kind: t.kind,
  }))
  return { title: player.videoDetails?.title ?? 'YouTube video', tracks }
}

/** Manual captions in a preferred language first, then automatic ones, then anything. */
export function pickTrack(tracks: CaptionTrack[], languages: string[] = ['en']): CaptionTrack | undefined {
  const lang = (t: CaptionTrack) => t.languageCode.toLowerCase().split('-')[0]
  for (const want of languages.map((l) => l.toLowerCase())) {
    const manual = tracks.find((t) => lang(t) === want && t.kind !== 'asr')
    if (manual) return manual
    const auto = tracks.find((t) => lang(t) === want)
    if (auto) return auto
  }
  return tracks.find((t) => t.kind !== 'asr') ?? tracks[0]
}

/** YouTube's json3 timed text as readable paragraphs, breaking on long pauses. */
export function captionsToText(json3: { events?: Array<{ tStartMs?: number; dDurationMs?: number; segs?: Array<{ utf8?: string }> }> }): string {
  const paragraphs: string[] = []
  let current = ''
  let lastEnd = 0
  for (const e of json3.events ?? []) {
    const text = (e.segs ?? []).map((s) => s.utf8 ?? '').join('').replace(/\s+/g, ' ').trim()
    if (!text) continue
    const start = e.tStartMs ?? lastEnd
    if (current && (start - lastEnd > 2500 || current.length > 600)) {
      paragraphs.push(current.trim())
      current = ''
    }
    current += `${text} `
    lastEnd = start + (e.dDurationMs ?? 0)
  }
  if (current.trim()) paragraphs.push(current.trim())
  return paragraphs.join('\n\n')
}

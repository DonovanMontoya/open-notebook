// Citations in answers look like [Source title]. These helpers turn them into
// links the UI can render as tabs, and find the passage a citation points at.

export interface CitableSource {
  id: string
  title: string | null
}

const normalize = (s: string) =>
  s
    .toLowerCase()
    .replace(/\.(pdf|docx?|md|txt|html?)$/i, '')
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()

/** Finds the source a bracketed label refers to: exact title, then prefix, then containment. */
export function matchSource(label: string, sources: CitableSource[]): CitableSource | undefined {
  // Open Notebook servers cite by record id, like [source:abc123].
  const byId = sources.find((s) => s.id === label.trim())
  if (byId) return byId
  const want = normalize(label)
  if (want.length < 2) return undefined
  const titled = sources.filter((s) => s.title).map((s) => ({ s, t: normalize(s.title!) }))
  return (
    titled.find((x) => x.t === want)?.s ??
    titled.find((x) => x.t.startsWith(want) || want.startsWith(x.t))?.s ??
    titled.find((x) => want.length >= 6 && (x.t.includes(want) || want.includes(x.t)))?.s
  )
}

/**
 * Rewrites [Title] citations as Markdown links to `cite:<index>` so a renderer
 * can draw them as tabs. Links, images and footnote-style [1] markers are left alone.
 */
export function linkCitations(markdown: string, sources: CitableSource[]): { markdown: string; cited: CitableSource[] } {
  const cited: CitableSource[] = []
  const out = markdown.replace(/(!?)\[([^\]\n]{2,160})\](?![(:[])/g, (whole, bang: string, inner: string) => {
    if (bang) return whole
    // One bracket may hold several citations: [A; B] or [A, B].
    const labels = inner.split(/\s*;\s*/)
    const parts = labels.map((label) => {
      const source = matchSource(label, sources)
      if (!source) return null
      let index = cited.indexOf(source)
      if (index < 0) index = cited.push(source) - 1
      // The prompt labels insight-only context "Title (insights)"; the tab shows just the title.
      const text = source.id === label.trim() ? (source.title ?? 'Source') : label.replace(/\s*\(insights\)\s*$/i, '')
      return `[${text.replace(/[[\]]/g, '')}](cite:${index})`
    })
    return parts.every(Boolean) ? parts.join(' ') : whole
  })
  return { markdown: out, cited }
}

const terms = (s: string) => new Set(s.toLowerCase().match(/[\p{L}\p{N}]{4,}/gu) ?? [])

/** The paragraph of a source that best matches some text, as character offsets. */
export function bestPassage(fullText: string, query: string): { start: number; end: number } | undefined {
  const want = terms(query)
  if (!want.size || !fullText) return undefined
  let best: { start: number; end: number; score: number } | undefined
  const re = /[^\n]+(?:\n(?!\n)[^\n]+)*/g
  for (let m = re.exec(fullText); m; m = re.exec(fullText)) {
    const have = terms(m[0])
    let hits = 0
    for (const t of want) if (have.has(t)) hits++
    // Prefer dense matches over long paragraphs that match by sheer size.
    const score = hits / Math.sqrt(Math.max(have.size, 8))
    if (hits && (!best || score > best.score)) best = { start: m.index, end: m.index + m[0].length, score }
  }
  return best && { start: best.start, end: best.end }
}

/** The sentence around a position in some text, for use as a passage query. */
export function sentenceAround(text: string, index: number): string {
  const before = text.slice(0, index)
  const start = Math.max(before.lastIndexOf('. '), before.lastIndexOf('\n')) + 1
  const afterDot = text.indexOf('. ', index)
  const afterLine = text.indexOf('\n', index)
  const ends = [afterDot, afterLine].filter((i) => i >= 0)
  const end = ends.length ? Math.min(...ends) + 1 : text.length
  return text.slice(start, end).trim()
}

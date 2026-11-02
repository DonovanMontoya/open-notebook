import type { Notebook } from '@open-notebook/api-client'

export interface ContextSource {
  title: string
  text: string
}

export interface ContextChunk {
  title: string
  text: string
}

export interface ContextNote {
  title: string
  content: string
}

const NOTES_BUDGET = 40_000

/** System prompt for notebook chat: instructions first, then the notebook material. */
export function buildChatPrompt(input: {
  notebook: Pick<Notebook, 'name' | 'description'>
  sources: ContextSource[]
  chunks: ContextChunk[]
  notes: ContextNote[]
}): string {
  const parts: string[] = [
    `You are a research assistant working inside the notebook "${input.notebook.name}".`,
    input.notebook.description ? `Notebook description: ${input.notebook.description}` : '',
    [
      'Answer from the notebook material below. When you use it, cite the source title in square brackets, like [Title].',
      'If the material does not cover the question, say so plainly, then answer from general knowledge and mark that part as such.',
      'Use Markdown for structure when it helps.',
    ].join(' '),
  ]

  if (input.sources.length) {
    parts.push('# Sources')
    for (const s of input.sources) parts.push(`<source title="${escapeAttr(s.title)}">\n${s.text.trim()}\n</source>`)
  }
  if (input.chunks.length) {
    parts.push(
      input.sources.length
        ? '# Excerpts from longer sources (each is too long to include whole, so it is represented by its opening and the passages most relevant to the question)'
        : '# Source excerpts (each source is represented by its opening and the passages most relevant to the question)',
    )
    for (const [title, texts] of groupByTitle(input.chunks)) {
      parts.push(`<source title="${escapeAttr(title)}" excerpts="true">\n${texts.map((t) => t.trim()).join('\n[…]\n')}\n</source>`)
    }
  }
  if (!input.sources.length && !input.chunks.length) parts.push('This notebook has no processed sources yet.')

  const notes = input.notes.filter((n) => n.content.trim())
  if (notes.length) {
    parts.push('# Notes the user wrote or saved')
    let used = 0
    for (const n of notes) {
      const body = n.content.trim()
      if (used + body.length > NOTES_BUDGET) break
      used += body.length
      parts.push(`<note title="${escapeAttr(n.title)}">\n${body}\n</note>`)
    }
  }

  return parts.filter(Boolean).join('\n\n')
}

function escapeAttr(value: string): string {
  return value.replace(/"/g, "'").replace(/[\r\n]+/g, ' ')
}

function groupByTitle(chunks: ContextChunk[]): Map<string, string[]> {
  const groups = new Map<string, string[]>()
  for (const c of chunks) groups.set(c.title, [...(groups.get(c.title) ?? []), c.text])
  return groups
}

/**
 * Splits a character budget across sources so each gets an equal share, and short sources hand
 * what they don't use to the longer ones. Returns each source's allowance, in input order.
 */
export function fairShare(lengths: number[], budget: number): number[] {
  const order = lengths.map((_, i) => i).sort((a, b) => lengths[a] - lengths[b])
  const out = lengths.map(() => 0)
  let left = Math.max(0, budget)
  order.forEach((i, k) => {
    out[i] = Math.min(lengths[i], Math.floor(left / (order.length - k)))
    left -= out[i]
  })
  return out
}

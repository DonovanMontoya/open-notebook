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
  } else if (input.chunks.length) {
    parts.push('# Source excerpts (the passages most relevant to the question)')
    for (const c of input.chunks) parts.push(`<excerpt source="${escapeAttr(c.title)}">\n${c.text.trim()}\n</excerpt>`)
  } else {
    parts.push('This notebook has no processed sources yet.')
  }

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

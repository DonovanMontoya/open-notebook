// Writes the episode script from the notebook with the chosen chat model.
import { WORDS_PER_SEGMENT } from './profiles.ts'

export interface ScriptLine {
  /** Index of the speaker in the speaker profile. */
  host: number
  text: string
}

export interface ScriptChapter {
  title: string
  lines: ScriptLine[]
}

export interface PodcastScript {
  title: string
  summary: string
  chapters: ScriptChapter[]
}

export function scriptPrompt(input: {
  notebookName: string
  material: string
  speakers: Array<{ name: string; backstory?: string; personality?: string }>
  briefing: string
  segments: number
  language?: string | null
  focus?: string
  /** Study notes on the sources, from the research pass. */
  notes?: string
}): string {
  const { speakers } = input
  const cast =
    speakers.length === 1
      ? `one host, ${speakers[0].name} (host 0), speaking directly to the listener.`
      : `${speakers.length} speakers who talk with each other, not at the listener: ${speakers.map((s, i) => `${s.name} (host ${i})`).join(', ')}.`
  const personas = speakers
    .map((s, i) => [`- ${s.name} (host ${i})`, s.backstory && `  Background: ${s.backstory}`, s.personality && `  Personality: ${s.personality}`].filter(Boolean).join('\n'))
    .join('\n')
  const words = input.segments * WORDS_PER_SEGMENT

  return [
    `Write the script for an audio episode about the research notebook "${input.notebookName}". The show has ${cast}`,
    `# Speakers\n${personas}`,
    `# Briefing\n${input.briefing}`,
    input.focus ? `# Focus of this episode\n${input.focus}` : '',
    `Length: about ${words} spoken words in ${input.segments} chapters.`,
    input.language ? `Write the whole script in this language: ${input.language}.` : '',
    [
      'Rules:',
      '- Base every claim on the material below. Name the source when it helps the listener ("the FTC report says…"). Never invent facts, numbers or quotes.',
      '- Write for the ear: short sentences, plain words, contractions, no lists, no Markdown, no URLs, no stage directions or sound effects.',
      '- Each line is one turn of speech, usually one to four sentences. Open with a hook, not a greeting. End with a short takeaway.',
      '- Keep each speaker in character. Chapter titles are short and specific, like a table of contents.',
    ].join('\n'),
    'Answer with JSON only, in exactly this shape:',
    '{"title": "Episode title", "summary": "One sentence about the episode", "chapters": [{"title": "Chapter title", "lines": [{"host": 0, "text": "Spoken words"}]}]}',
    input.notes
      ? `# Research notes\nThese notes were made from the material below. Use them to keep each finding at the strength its source states, with its caveats.\n\n${input.notes}`
      : '',
    '# Material',
    input.material,
  ]
    .filter(Boolean)
    .join('\n\n')
}

/** Reads the model's JSON, tolerating code fences and stray text around it. */
export function parseScript(raw: string, hostCount: number): PodcastScript {
  const text = raw.replace(/```(?:json)?/gi, '')
  const start = text.indexOf('{')
  const end = text.lastIndexOf('}')
  if (start < 0 || end <= start) throw new Error('The script came back without the expected structure. Try again.')
  let data: unknown
  try {
    data = JSON.parse(text.slice(start, end + 1))
  } catch {
    throw new Error('The script came back as invalid JSON. Try again, or pick a stronger chat model.')
  }
  const d = data as { title?: unknown; summary?: unknown; chapters?: unknown }
  const chapters = (Array.isArray(d.chapters) ? d.chapters : [])
    .map((c: { title?: unknown; lines?: unknown }, i: number) => ({
      title: typeof c?.title === 'string' && c.title.trim() ? c.title.trim() : `Part ${i + 1}`,
      lines: (Array.isArray(c?.lines) ? c.lines : [])
        .map((l: { host?: unknown; speaker?: unknown; text?: unknown }) => ({
          host: Math.min(hostCount - 1, Math.max(0, Number(l?.host ?? l?.speaker ?? 0) || 0)),
          text: typeof l?.text === 'string' ? cleanSpoken(l.text) : '',
        }))
        .filter((l: ScriptLine) => l.text),
    }))
    .filter((c: ScriptChapter) => c.lines.length)
  if (!chapters.length) throw new Error('The script had no spoken lines. Try again.')
  return {
    title: typeof d.title === 'string' && d.title.trim() ? d.title.trim() : 'Untitled episode',
    summary: typeof d.summary === 'string' ? d.summary.trim() : '',
    chapters,
  }
}

/** Removes Markdown and bracketed directions a model may slip into spoken text. */
function cleanSpoken(text: string): string {
  return text
    .replace(/\[[^\]]*\]|\([^)]*(laughs|pause|music|sighs)[^)]*\)/gi, '')
    .replace(/[*_#`>]/g, '')
    .replace(/\s+/g, ' ')
    .trim()
}

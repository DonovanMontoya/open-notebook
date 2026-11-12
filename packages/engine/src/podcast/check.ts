// Checks an episode script against the material it was written from.
//
// A model lists the script's factual claims, rules on each one and copies the
// passage that backs it. The quotes are then looked up in the material here,
// in code, so a claim only counts as supported when its evidence is really in
// the user's sources.
import { jsonObject } from '../text.ts'
import type { PodcastScript } from './script.ts'

export type ClaimVerdict = 'supported' | 'overstated' | 'unsupported'

export interface CheckedClaim {
  /** 1-based number of the spoken line the claim comes from. */
  line: number
  /** The spoken line, as written. */
  said: string
  /** The claim, restated briefly. */
  claim: string
  verdict: ClaimVerdict
  /** The passage the checker gave as evidence. */
  quote?: string
  /** The source the quote was found in, when it was found. */
  source?: string
  /** Whether the quote was found in the material. */
  quoteFound: boolean
  /** Why the claim is overstated or unsupported. */
  note?: string
}

export interface ScriptCheck {
  claims: CheckedClaim[]
  supported: number
  overstated: number
  unsupported: number
  /** Distinct sources that back at least one claim. */
  sourcesUsed: number
  /** Distinct sources in the material (notes not counted). */
  sourcesTotal: number
}

export const CHECK_SYSTEM =
  'You are a careful fact-checker. You compare a podcast script with the research material it was written from and report exactly what the material does and does not support. You reply with JSON only.'

/** Spoken lines in order, numbered from 1 across chapters. */
function scriptLines(script: PodcastScript): string[] {
  return script.chapters.flatMap((c) => c.lines.map((l) => l.text))
}

export function checkPrompt(script: PodcastScript, material: string): string {
  let n = 0
  const numbered = script.chapters
    .map((c) => [`## ${c.title}`, ...c.lines.map((l) => `[${++n}] ${l.text}`)].join('\n'))
    .join('\n\n')
  return [
    'Check this podcast script against the research material below.',
    [
      'List every factual claim the script makes: findings, numbers, what a study did or concluded, causes and effects, definitions, and descriptions of what a source says. Skip greetings, transitions, questions, jokes and opinions the host clearly frames as their own.',
      'For each claim give a verdict:',
      '- "supported": the material says this, at the same strength.',
      '- "overstated": the material says something related but weaker, narrower or hedged, or the script drops a caveat, generalizes beyond what was studied, or treats a correlation or proposal as an established result.',
      '- "unsupported": the material does not say this, even if it may be true elsewhere.',
      'For "supported" and "overstated", copy the passage from the material that the claim rests on, word for word, at most 40 words. Do not paraphrase, fix typos or join separate passages. For "unsupported" leave the quote empty.',
      'For "overstated" and "unsupported", add one plain sentence saying what is wrong.',
    ].join('\n'),
    'Answer with JSON only, in exactly this shape:',
    '{"claims": [{"line": 12, "claim": "Short restatement", "verdict": "supported", "quote": "Exact words from the material", "note": ""}]}',
    `# Script\n\n${numbered}`,
    `# Material\n\n${material}`,
  ].join('\n\n')
}

/** Lowercase words and numbers only, so quotes match across punctuation, case and line breaks. */
function normalize(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()
}

/** One piece of the material an episode is written from. */
export interface MaterialPart {
  title: string
  text: string
  /** A passage from a source too long to send whole. */
  excerpt?: boolean
  /** One of the user's notes rather than a source. */
  note?: boolean
}

/** The material as the models read it, one headed section per part. */
export function materialText(parts: MaterialPart[]): string {
  return parts.map((p) => `## ${p.note ? 'Note: ' : p.excerpt ? 'Excerpt from ' : ''}${p.title}\n${p.text}`).join('\n\n')
}

interface Section {
  title: string
  isNote: boolean
  text: string
}

export function materialSections(parts: MaterialPart[]): Section[] {
  return parts.map((p) => ({ title: p.title, isNote: Boolean(p.note), text: normalize(p.text) }))
}

const WINDOW = 4

/**
 * Finds the section a quote comes from. Short quotes must appear whole.
 * Longer ones may differ slightly (hyphenation, a dropped or misspelled word
 * from a PDF), so 80% of their words must be covered by four-word runs that
 * appear in one section.
 */
export function findQuote(quote: string, sections: Section[]): Section | undefined {
  const fragments = quote
    .split(/\.\.\.|…/)
    .map(normalize)
    .filter((f) => f.split(' ').length >= 3)
  if (!fragments.length) return undefined
  return sections.find((s) =>
    fragments.every((f) => {
      if (s.text.includes(f)) return true
      const words = f.split(' ')
      if (words.length < 8) return false
      const covered = new Array<boolean>(words.length).fill(false)
      for (let i = 0; i + WINDOW <= words.length; i++) {
        if (s.text.includes(words.slice(i, i + WINDOW).join(' '))) covered.fill(true, i, i + WINDOW)
      }
      return covered.filter(Boolean).length / words.length >= 0.8
    }),
  )
}

/** Reads the checker's JSON and verifies every quote against the material. */
export function parseCheck(raw: string, script: PodcastScript, material: MaterialPart[]): ScriptCheck {
  let data: { claims?: unknown }
  try {
    data = jsonObject(raw) as { claims?: unknown }
  } catch {
    throw new Error('The claim check came back as invalid JSON.')
  }
  if (!Array.isArray(data.claims)) throw new Error('The claim check came back without a list of claims.')

  const lines = scriptLines(script)
  const sections = materialSections(material)
  const claims: CheckedClaim[] = []
  for (const c of data.claims as Array<Record<string, unknown>>) {
    const claim = typeof c?.claim === 'string' ? c.claim.trim() : ''
    if (!claim) continue
    const line = Math.min(lines.length, Math.max(1, Math.round(Number(c.line)) || 1))
    const quote = typeof c.quote === 'string' && c.quote.trim() ? c.quote.trim() : undefined
    let verdict: ClaimVerdict = c.verdict === 'supported' || c.verdict === 'overstated' ? c.verdict : 'unsupported'
    let note = typeof c.note === 'string' && c.note.trim() ? c.note.trim() : undefined
    const found = quote ? findQuote(quote, sections) : undefined
    if (verdict === 'supported' && !found) {
      verdict = 'unsupported'
      note = 'The checker cited a passage that is not actually there, so this claim has no evidence.'
    }
    claims.push({ line, said: lines[line - 1] ?? '', claim, verdict, quote, source: found?.title, quoteFound: Boolean(found), note })
  }
  claims.sort((a, b) => a.line - b.line)

  const count = (v: ClaimVerdict) => claims.filter((c) => c.verdict === v).length
  const sources = new Set(sections.filter((s) => !s.isNote).map((s) => s.title))
  const used = new Set(claims.filter((c) => c.verdict === 'supported' && c.source && sources.has(c.source)).map((c) => c.source))
  return {
    claims,
    supported: count('supported'),
    overstated: count('overstated'),
    unsupported: count('unsupported'),
    sourcesUsed: used.size,
    sourcesTotal: sources.size,
  }
}

export const RESEARCH_SYSTEM =
  'You are a research assistant who reads sources closely and reports what they actually show, including their limits. You write plain text.'

/** Study notes on each source, written before the script so findings keep their strength and caveats. */
export function researchPrompt(material: string, focus?: string): string {
  return [
    'Read the research material below and write study notes on each source, to be used by someone writing a podcast about it.',
    focus ? `The episode will focus on: ${focus}` : '',
    [
      'For each source, under a heading with its title:',
      '- The question it asks and why it matters.',
      '- What was done: method, data, sample size, setting.',
      '- The main findings, with exact numbers, each with a short quote copied word for word.',
      '- The limitations and caveats the authors state, and what the source does not show.',
      '- How strong the evidence is: an experiment, an observational study, a model or simulation, a survey, an opinion piece or a proposal.',
      'Then note where sources agree, disagree or build on each other.',
      'Do not add anything the material does not say.',
    ].join('\n'),
    `# Material\n\n${material}`,
  ]
    .filter(Boolean)
    .join('\n\n')
}

export const REVISE_SYSTEM =
  'You are an editor who corrects podcast scripts so every claim matches the research it is based on. You reply with JSON only.'

/** Asks for the script back with the flagged lines corrected and everything else left alone. */
export function revisePrompt(script: PodcastScript, flagged: CheckedClaim[], material: string): string {
  const problems = flagged
    .map((c) =>
      [
        `- Line ${c.line} (${c.verdict}): "${c.said}"`,
        `  Claim: ${c.claim}`,
        c.note ? `  Problem: ${c.note}` : '',
        c.quote && c.quoteFound ? `  What the source says: "${c.quote}"` : '',
      ]
        .filter(Boolean)
        .join('\n'),
    )
    .join('\n')
  return [
    'A fact-check found problems in this podcast script. Fix them and return the whole script.',
    [
      'Rules:',
      '- For an overstated claim, say what the source says at the strength it says it, and keep its caveats.',
      '- For a claim that is not in the sources, remove it, or have the speaker frame it clearly as their own view or an open question.',
      '- Change only what you need to. Keep the title, the chapters, the speakers, the voice and the length close to the original.',
      '- Write for the ear, as before: no lists, no Markdown, no stage directions.',
    ].join('\n'),
    `# Problems\n${problems}`,
    'Answer with JSON only, in the same shape as the script:',
    `# Script\n${JSON.stringify(script)}`,
    `# Material\n\n${material}`,
  ].join('\n\n')
}

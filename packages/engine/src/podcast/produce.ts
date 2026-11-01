import { Semaphore } from '../semaphore.ts'
import { Timeline } from './audio.ts'
import type { PodcastScript } from './script.ts'
import type { DialogueLine, VoiceProvider } from './voices.ts'

export interface TimedLine {
  speaker: string
  host: number
  text: string
  chapter: number
  start: number
  end: number
}

export interface Chapter {
  title: string
  start: number
}

export interface Production {
  pcm: Int16Array
  duration: number
  lines: TimedLine[]
  chapters: Chapter[]
  /** False when line timing inside a dialogue request was estimated. */
  exactTiming: boolean
}

const BETWEEN_LINES = 0.32
const BETWEEN_CHAPTERS = 0.9
const DIALOGUE_CHARS = 2800

/**
 * Voices every line and joins them into one track, recording when each line
 * and chapter starts so the player can follow along.
 */
export async function produce(options: {
  script: PodcastScript
  hosts: string[]
  voices: string[]
  provider: VoiceProvider
  onProgress?: (done: number, total: number) => void | Promise<void>
  signal?: AbortSignal
}): Promise<Production> {
  const { script, hosts, voices, provider, signal } = options
  const flat = script.chapters.flatMap((c, chapter) => c.lines.map((l) => ({ ...l, chapter, speaker: hosts[l.host] ?? hosts[0] })))
  const timeline = new Timeline()
  const lines: TimedLine[] = []
  const chapters: Chapter[] = []

  // Two-speaker dialogue requests can't voice three or four people, so larger casts go line by line.
  if (provider.mode === 'dialogue' && hosts.length <= 2) {
    // Whole stretches of conversation per request; line times are spread by length.
    const groups: Array<typeof flat> = []
    for (let c = 0; c < script.chapters.length; c++) {
      let group: typeof flat = []
      let size = 0
      for (const line of flat.filter((l) => l.chapter === c)) {
        if (group.length && size + line.text.length > DIALOGUE_CHARS) {
          groups.push(group)
          group = []
          size = 0
        }
        group.push(line)
        size += line.text.length
      }
      if (group.length) groups.push(group)
    }
    const slots = new Semaphore(provider.concurrency)
    let done = 0
    const audio = await Promise.all(
      groups.map((group) =>
        slots.run(async () => {
          const pcm = await provider.speakDialogue(group as DialogueLine[], voices, signal)
          done++
          await options.onProgress?.(done, groups.length)
          return pcm
        }),
      ),
    )
    let lastChapter = -1
    groups.forEach((group, i) => {
      if (group[0].chapter !== lastChapter) {
        if (lastChapter >= 0) timeline.pause(BETWEEN_CHAPTERS)
        lastChapter = group[0].chapter
        chapters.push({ title: script.chapters[lastChapter].title, start: timeline.seconds })
      } else {
        timeline.pause(BETWEEN_LINES)
      }
      const span = timeline.append(audio[i])
      const total = group.reduce((n, l) => n + l.text.length, 0)
      let at = span.start
      for (const l of group) {
        const share = ((span.end - span.start) * l.text.length) / total
        lines.push({ speaker: l.speaker, host: l.host, text: l.text, chapter: l.chapter, start: at, end: at + share })
        at += share
      }
    })
    return { pcm: timeline.toPcm(), duration: timeline.seconds, lines, chapters, exactTiming: false }
  }

  const slots = new Semaphore(provider.concurrency)
  let done = 0
  const audio = await Promise.all(
    flat.map((line) =>
      slots.run(async () => {
        const pcm = await provider.speakLine(line.text, voices[line.host] ?? voices[0], signal)
        done++
        await options.onProgress?.(done, flat.length)
        return pcm
      }),
    ),
  )
  flat.forEach((line, i) => {
    const previous = flat[i - 1]
    if (!previous || previous.chapter !== line.chapter) {
      if (previous) timeline.pause(BETWEEN_CHAPTERS)
      chapters.push({ title: script.chapters[line.chapter].title, start: timeline.seconds })
    } else {
      timeline.pause(BETWEEN_LINES)
    }
    const span = timeline.append(audio[i])
    lines.push({ speaker: line.speaker, host: line.host, text: line.text, chapter: line.chapter, ...span })
  })
  return { pcm: timeline.toPcm(), duration: timeline.seconds, lines, chapters, exactTiming: true }
}

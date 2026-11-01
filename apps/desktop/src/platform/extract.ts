// Turns a URL or a picked file into readable text on device.
import { captionsToText, isMedia, parseWatchPage, pickTrack, youtubeId } from '@open-notebook/engine'
import { Readability } from '@mozilla/readability'

import { OFFICE_FILE, readOffice } from './office.ts'

const USER_AGENT =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15'

export interface Extracted {
  title: string
  text: string
}

export async function readPdf(data: ArrayBuffer): Promise<Extracted> {
  const pdfjs = await import('pdfjs-dist')
  const { default: workerUrl } = await import('pdfjs-dist/build/pdf.worker.min.mjs?url')
  pdfjs.GlobalWorkerOptions.workerSrc = workerUrl
  const task = pdfjs.getDocument({ data: new Uint8Array(data) })
  const doc = await task.promise
  try {
    const pages: string[] = []
    for (let n = 1; n <= doc.numPages; n++) {
      const page = await doc.getPage(n)
      const content = await page.getTextContent()
      let line = ''
      const lines: string[] = []
      for (const item of content.items) {
        if (!('str' in item)) continue
        line += item.str
        if (item.hasEOL) {
          lines.push(line)
          line = ''
        } else if (item.str && !item.str.endsWith(' ')) {
          line += ' '
        }
      }
      if (line) lines.push(line)
      pages.push(lines.map((l) => l.trimEnd()).join('\n'))
      page.cleanup()
    }
    const meta = await doc.getMetadata().catch(() => null)
    const info = meta?.info as { Title?: string } | undefined
    return { title: info?.Title?.trim() ?? '', text: pages.join('\n\n').trim() }
  } finally {
    await task.destroy()
  }
}

export function readHtml(html: string, url: string): Extracted {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  const base = doc.createElement('base')
  base.href = url
  doc.head.prepend(base)
  const fallbackTitle = doc.title.trim()
  const article = new Readability(doc).parse()
  if (article?.textContent?.trim()) {
    return { title: article.title?.trim() || fallbackTitle, text: tidy(article.textContent) }
  }
  const fresh = new DOMParser().parseFromString(html, 'text/html')
  fresh.querySelectorAll('script, style, noscript, nav, footer, header, svg').forEach((el) => el.remove())
  return { title: fallbackTitle, text: tidy(fresh.body?.textContent ?? '') }
}

function tidy(text: string): string {
  return text
    .replace(/[ \t ]+/g, ' ')
    .replace(/ *\n */g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

/** Downloads a page or PDF with the given fetch and extracts its text. */
export function makeUrlExtractor(doFetch: typeof fetch) {
  return async (url: string, signal?: AbortSignal): Promise<Extracted> => {
    const video = youtubeId(url)
    if (video) return readYouTube(doFetch, video, signal)
    const response = await doFetch(url, {
      signal,
      headers: { 'User-Agent': USER_AGENT, Accept: 'text/html,application/xhtml+xml,application/pdf;q=0.9,*/*;q=0.8' },
    })
    if (!response.ok) throw new Error(`Could not download the page (${response.status} ${response.statusText})`.trim())
    const type = response.headers.get('content-type') ?? ''
    if (type.includes('application/pdf') || /\.pdf($|\?)/i.test(url)) {
      const pdf = await readPdf(await response.arrayBuffer())
      return { title: pdf.title || decodeURIComponent(url.split('/').pop() ?? url), text: pdf.text }
    }
    const body = await response.text()
    if (type.startsWith('text/plain') || type.includes('markdown')) {
      return { title: decodeURIComponent(url.split('/').pop() ?? url), text: body.trim() }
    }
    return readHtml(body, url)
  }
}

/** Reads a file the user picked: PDF, Markdown, plain text or HTML. */
export async function readFile(file: File): Promise<Extracted> {
  const name = file.name.replace(/\.[^.]+$/, '')
  // Audio and video are transcribed by the backend, not read here.
  if (isMedia(file.type, file.name)) return { title: name, text: '' }
  if (file.type === 'application/pdf' || /\.pdf$/i.test(file.name)) {
    const pdf = await readPdf(await file.arrayBuffer())
    return { title: pdf.title || name, text: pdf.text }
  }
  if (OFFICE_FILE.test(file.name)) {
    const office = readOffice(file.name, await file.arrayBuffer())
    return { title: office.title || name, text: office.text }
  }
  const text = await file.text()
  if (/\.html?$/i.test(file.name) || file.type === 'text/html') {
    const html = readHtml(text, 'file:///' + encodeURIComponent(file.name))
    return { title: html.title || name, text: html.text }
  }
  return { title: name, text: text.trim() }
}

/** Captions of a YouTube video, in the browser's languages when available. */
async function readYouTube(doFetch: typeof fetch, id: string, signal?: AbortSignal): Promise<Extracted> {
  const languages = [...new Set([...(navigator.languages ?? []), 'en'].map((l) => l.split('-')[0]))]
  const page = await doFetch(`https://www.youtube.com/watch?v=${encodeURIComponent(id)}&hl=en`, {
    signal,
    headers: { 'User-Agent': USER_AGENT, 'Accept-Language': languages.join(',') },
  })
  if (!page.ok) throw new Error(`YouTube did not answer (${page.status})`)
  const { title, tracks } = parseWatchPage(await page.text())
  const track = pickTrack(tracks, languages)
  if (!track) throw new Error('This video has no captions to read.')
  const captions = await doFetch(`${track.baseUrl}&fmt=json3`, { signal, headers: { 'User-Agent': USER_AGENT } })
  if (!captions.ok) throw new Error(`Could not download the captions (${captions.status})`)
  const text = captionsToText(await captions.json())
  if (!text) throw new Error('The captions for this video are empty.')
  return { title, text }
}

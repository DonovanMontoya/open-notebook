// Reads text out of Word, PowerPoint, Excel and EPUB files on device. All four are zip archives of XML.
import { strFromU8, unzipSync } from 'fflate'

export interface OfficeText {
  title: string
  text: string
}

export function readOffice(name: string, data: ArrayBuffer): OfficeText {
  let files: Record<string, Uint8Array>
  try {
    files = unzipSync(new Uint8Array(data))
  } catch {
    throw new Error('This file could not be opened. Older .doc, .ppt and .xls files are not supported; save it in the newer format.')
  }
  const read = (path: string) => (files[path] ? strFromU8(files[path]) : undefined)
  const ext = name.split('.').pop()!.toLowerCase()
  const text = ext === 'docx' ? readDocx(read) : ext === 'pptx' ? readPptx(files, read) : ext === 'xlsx' ? readXlsx(files, read) : readEpub(read)
  return { title: coreTitle(read('docProps/core.xml')) ?? epubTitle(read) ?? '', text: text.replace(/\n{3,}/g, '\n\n').trim() }
}

type Read = (path: string) => string | undefined

const xml = (source: string) => new DOMParser().parseFromString(source, 'application/xml')

/** Every element with this local name, ignoring namespace prefixes. */
const all = (root: Document | Element, local: string) => [...root.getElementsByTagName('*')].filter((e) => e.localName === local)

/** The text of every <t> element under root, joined. */
const texts = (root: Document | Element, sep = '') => all(root, 't').map((t) => t.textContent ?? '').join(sep)

const byNumber = (a: string, b: string) => Number(/(\d+)\.xml$/.exec(a)?.[1] ?? 0) - Number(/(\d+)\.xml$/.exec(b)?.[1] ?? 0)

function coreTitle(core?: string) {
  if (!core) return undefined
  return all(xml(core), 'title')[0]?.textContent?.trim() || undefined
}

/** Text of a Word paragraph, keeping tabs and line breaks. */
function runText(p: Element): string {
  let out = ''
  for (const el of p.getElementsByTagName('*')) {
    if (el.localName === 't') out += el.textContent ?? ''
    else if (el.localName === 'tab') out += '\t'
    else if (el.localName === 'br' || el.localName === 'cr') out += '\n'
  }
  return out
}

function readDocx(read: Read): string {
  const source = read('word/document.xml')
  if (!source) throw new Error('This Word file has no document body.')
  const body = all(xml(source), 'body')[0]
  const out: string[] = []
  // Walk the body in order so tables stay where they are in the document.
  for (const block of body ? [...body.children] : []) {
    if (block.localName === 'p') {
      const text = runText(block).trim()
      if (text) out.push(text)
    } else if (block.localName === 'tbl') {
      const rows = all(block, 'tr')
        .map((row) => all(row, 'tc').map((c) => all(c, 'p').map(runText).join(' ').trim()))
        .filter((cells) => cells.some(Boolean))
        .map((cells) => cells.join(' | '))
      if (rows.length) out.push(rows.join('\n'))
    } else if (block.localName === 'sdt') {
      const text = all(block, 'p').map(runText).join('\n').trim()
      if (text) out.push(text)
    }
  }
  return out.join('\n\n')
}

function readPptx(files: Record<string, Uint8Array>, read: Read): string {
  const slides = Object.keys(files).filter((f) => /^ppt\/slides\/slide\d+\.xml$/.test(f)).sort(byNumber)
  return slides
    .map((path, i) => {
      const lines = all(xml(read(path)!), 'p')
        .map((p) => texts(p).trim())
        .filter(Boolean)
      const notes = read(path.replace('slides/slide', 'notesSlides/notesSlide'))
      const noteText = notes ? texts(xml(notes), ' ').trim() : ''
      return [`Slide ${i + 1}`, ...lines, noteText && `Notes: ${noteText}`].filter(Boolean).join('\n')
    })
    .join('\n\n')
}

function readXlsx(files: Record<string, Uint8Array>, read: Read): string {
  const shared = read('xl/sharedStrings.xml')
  const strings = shared ? all(xml(shared), 'si').map((si) => texts(si)) : []
  const workbook = read('xl/workbook.xml')
  const names = workbook ? all(xml(workbook), 'sheet').map((s) => s.getAttribute('name') ?? '') : []
  const sheets = Object.keys(files).filter((f) => /^xl\/worksheets\/sheet\d+\.xml$/.test(f)).sort(byNumber)
  return sheets
    .map((path, i) => {
      const rows = all(xml(read(path)!), 'row').map((row) =>
        all(row, 'c')
          .map((c) => {
            const type = c.getAttribute('t')
            if (type === 'inlineStr') return texts(c)
            const v = all(c, 'v')[0]?.textContent ?? ''
            return type === 's' ? (strings[Number(v)] ?? '') : v
          })
          .join('\t')
          .trimEnd(),
      )
      return [names[i] || `Sheet ${i + 1}`, ...rows.filter(Boolean)].join('\n')
    })
    .join('\n\n')
}

function opfPath(read: Read) {
  const container = read('META-INF/container.xml')
  return container ? all(xml(container), 'rootfile')[0]?.getAttribute('full-path') ?? undefined : undefined
}

function epubTitle(read: Read) {
  const path = opfPath(read)
  const opf = path && read(path)
  return opf ? all(xml(opf), 'title')[0]?.textContent?.trim() || undefined : undefined
}

function readEpub(read: Read): string {
  const path = opfPath(read)
  const opf = path && read(path)
  if (!path || !opf) throw new Error('This EPUB has no table of contents.')
  const doc = xml(opf)
  const dir = path.includes('/') ? path.slice(0, path.lastIndexOf('/') + 1) : ''
  const items = new Map(all(doc, 'item').map((i) => [i.getAttribute('id'), i.getAttribute('href')]))
  return all(doc, 'itemref')
    .map((ref) => items.get(ref.getAttribute('idref')))
    .filter((href): href is string => !!href)
    .map((href) => {
      const page = read(dir + decodeURIComponent(href.split('#')[0]))
      if (!page) return ''
      const html = new DOMParser().parseFromString(page, 'text/html')
      html.querySelectorAll('script, style').forEach((el) => el.remove())
      const blocks = [...html.body.querySelectorAll('h1, h2, h3, h4, p, li, blockquote, pre')].filter((b) => !b.parentElement?.closest('p, li, blockquote, pre'))
      const text = blocks.length ? blocks.map((b) => (b.textContent ?? '').trim()) : [html.body.textContent ?? '']
      return text.filter((t) => t.trim()).join('\n\n')
    })
    .filter(Boolean)
    .join('\n\n')
}

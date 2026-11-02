/** Splits text into overlapping chunks on paragraph, then sentence, boundaries. */
export function chunkText(text: string, maxChars = 1800, overlap = 200): string[] {
  const clean = text.replace(/\r\n?/g, '\n').replace(/\n{3,}/g, '\n\n').trim()
  if (!clean) return []
  if (clean.length <= maxChars) return [clean]

  const pieces: string[] = []
  for (const paragraph of clean.split(/\n\n/)) {
    if (paragraph.length <= maxChars) {
      pieces.push(paragraph)
      continue
    }
    // Long paragraph: split on sentence ends, and hard-split anything still too long.
    for (const sentence of paragraph.split(/(?<=[.!?])\s+/)) {
      for (let i = 0; i < sentence.length; i += maxChars) pieces.push(sentence.slice(i, i + maxChars))
    }
  }

  const chunks: string[] = []
  let current = ''
  for (const piece of pieces) {
    if (current && current.length + piece.length + 2 > maxChars) {
      chunks.push(current)
      const tail = overlap > 0 ? current.slice(-overlap) : ''
      const cut = tail.indexOf(' ')
      current = cut >= 0 ? tail.slice(cut + 1) : ''
    }
    current = current ? `${current}\n\n${piece}` : piece
  }
  if (current) chunks.push(current)
  return chunks
}

export function cosineSimilarity(a: ArrayLike<number>, b: ArrayLike<number>): number {
  if (a.length !== b.length || a.length === 0) return 0
  let dot = 0
  let na = 0
  let nb = 0
  for (let i = 0; i < a.length; i++) {
    dot += a[i] * b[i]
    na += a[i] * a[i]
    nb += b[i] * b[i]
  }
  return na && nb ? dot / Math.sqrt(na * nb) : 0
}

// Embeddings are stored as base64-encoded little-endian float32 arrays:
// about a third the size of JSON and much faster to decode for search.

export function encodeVector(vector: number[]): string {
  const bytes = new Uint8Array(new Float32Array(vector).buffer)
  let binary = ''
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  }
  return btoa(binary)
}

export function decodeVector(encoded: string): Float32Array {
  const binary = atob(encoded)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return new Float32Array(bytes.buffer)
}

/** Short title from the first line of some text. */
export function titleFrom(text: string, max = 60): string {
  const line = text.trim().split('\n')[0]?.trim() ?? ''
  return line.length > max ? `${line.slice(0, max - 1).trimEnd()}…` : line
}

/** Every http(s) address in some pasted text, deduplicated, in order. */
export function parseLinks(text: string): string[] {
  const found = text.match(/https?:\/\/[^\s<>"'),\]]+/gi) ?? []
  const bare = text
    .split(/\s+/)
    .filter((t) => !/^https?:/i.test(t) && /^[\w-]+(\.[\w-]+)+(\/\S*)?$/.test(t))
    .map((t) => `https://${t}`)
  return [...new Set([...found, ...bare].map((u) => u.replace(/[.,;]+$/, '')))]
}

/** The JSON object in a model reply, ignoring code fences and text around it. Throws if there is none. */
export function jsonObject(raw: string): unknown {
  const text = raw.replace(/```(?:json)?/gi, '')
  return JSON.parse(text.slice(text.indexOf('{'), text.lastIndexOf('}') + 1))
}

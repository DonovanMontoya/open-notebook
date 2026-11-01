import { ProviderError, type ProviderKind } from './types.ts'

/** Reads a server-sent-events body and yields each event's data payload. */
export async function* readSse(body: ReadableStream<Uint8Array>, signal?: AbortSignal): AsyncGenerator<string> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let data: string[] = []
  try {
    while (true) {
      if (signal?.aborted) throw signal.reason ?? new DOMException('Aborted', 'AbortError')
      const { value, done } = await reader.read()
      buffer += done ? decoder.decode() : decoder.decode(value, { stream: true })
      let newline: number
      while ((newline = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, newline).replace(/\r$/, '')
        buffer = buffer.slice(newline + 1)
        if (line === '') {
          if (data.length) yield data.join('\n')
          data = []
        } else if (line.startsWith('data:')) {
          data.push(line.slice(5).replace(/^ /, ''))
        }
      }
      if (done) {
        if (buffer.startsWith('data:')) data.push(buffer.slice(5).replace(/^ /, ''))
        if (data.length) yield data.join('\n')
        return
      }
    }
  } finally {
    reader.releaseLock()
  }
}

/** Turns a failed provider response into a readable error. */
export async function providerError(kind: ProviderKind, response: Response): Promise<ProviderError> {
  let message = `${response.status} ${response.statusText}`.trim()
  try {
    const text = await response.text()
    try {
      const body = JSON.parse(text)
      const detail = body?.error?.message ?? body?.error ?? body?.message ?? body?.detail
      if (typeof detail === 'string') message = detail
      else if (Array.isArray(body) && body[0]?.error?.message) message = body[0].error.message
    } catch {
      if (text.trim()) message = text.trim().slice(0, 300)
    }
  } catch {
    // Body unreadable; keep the status line.
  }
  if (response.status === 401 || response.status === 403) message = `API key rejected: ${message}`
  return new ProviderError(kind, response.status, message)
}

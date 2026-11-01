// 16-bit mono PCM helpers. Every voice provider is asked for 24 kHz PCM, so
// lines can be joined with exact timing and written out as one WAV file.

export const SAMPLE_RATE = 24_000

export function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

/** Little-endian 16-bit samples from raw bytes (an odd trailing byte is dropped). */
export function bytesToPcm(bytes: Uint8Array): Int16Array {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const out = new Int16Array(bytes.byteLength >> 1)
  for (let i = 0; i < out.length; i++) out[i] = view.getInt16(i * 2, true)
  return out
}

export function silence(seconds: number, rate = SAMPLE_RATE): Int16Array {
  return new Int16Array(Math.round(seconds * rate))
}

/** Linear resampling, for providers that answer at another rate. */
export function resample(pcm: Int16Array, from: number, to = SAMPLE_RATE): Int16Array {
  if (from === to || pcm.length === 0) return pcm
  const out = new Int16Array(Math.round((pcm.length * to) / from))
  const step = from / to
  for (let i = 0; i < out.length; i++) {
    const pos = i * step
    const a = Math.floor(pos)
    const b = Math.min(a + 1, pcm.length - 1)
    out[i] = Math.round(pcm[a] + (pcm[b] - pcm[a]) * (pos - a))
  }
  return out
}

const AUDIBLE = 120

/** Index of the first and one past the last audible sample, keeping a little air on each side. */
export function trimSilence(pcm: Int16Array, padSeconds = 0.08, rate = SAMPLE_RATE): Int16Array {
  let start = 0
  while (start < pcm.length && Math.abs(pcm[start]) <= AUDIBLE) start++
  if (start === pcm.length) return pcm.subarray(0, 0)
  let end = pcm.length
  while (end > start && Math.abs(pcm[end - 1]) <= AUDIBLE) end--
  const pad = Math.round(padSeconds * rate)
  return pcm.subarray(Math.max(0, start - pad), Math.min(pcm.length, end + pad))
}

/** Appends PCM pieces and reports where each one starts, in seconds. */
export class Timeline {
  private parts: Int16Array[] = []
  private samples = 0
  readonly rate: number

  constructor(rate = SAMPLE_RATE) {
    this.rate = rate
  }

  get seconds(): number {
    return this.samples / this.rate
  }

  append(pcm: Int16Array): { start: number; end: number } {
    const start = this.seconds
    this.parts.push(pcm)
    this.samples += pcm.length
    return { start, end: this.seconds }
  }

  pause(seconds: number): void {
    this.append(silence(seconds, this.rate))
  }

  toPcm(): Int16Array {
    const out = new Int16Array(this.samples)
    let offset = 0
    for (const p of this.parts) {
      out.set(p, offset)
      offset += p.length
    }
    return out
  }
}

/** A standard 16-bit mono WAV file. */
export function encodeWav(pcm: Int16Array, rate = SAMPLE_RATE): Uint8Array {
  const bytes = new Uint8Array(44 + pcm.length * 2)
  const view = new DataView(bytes.buffer)
  const text = (offset: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(offset + i, s.charCodeAt(i))
  }
  text(0, 'RIFF')
  view.setUint32(4, 36 + pcm.length * 2, true)
  text(8, 'WAVE')
  text(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true) // PCM
  view.setUint16(22, 1, true) // mono
  view.setUint32(24, rate, true)
  view.setUint32(28, rate * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  text(36, 'data')
  view.setUint32(40, pcm.length * 2, true)
  for (let i = 0; i < pcm.length; i++) view.setInt16(44 + i * 2, pcm[i], true)
  return bytes
}

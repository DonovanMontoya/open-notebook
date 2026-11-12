// Episode audio storage and the WebSocket used by OpenAI Live voices.
import type { AudioStore, FileStore, LiveSocket, OpenSocket } from '@open-notebook/engine'

import { isTauri } from './index.ts'

/** Episode audio: files in the app data folder in the app, IndexedDB blobs in browser dev. */
export function createAudioStore(): AudioStore {
  return isTauri ? tauriAudioStore() : browserAudioStore()
}

function tauriAudioStore(): AudioStore {
  return {
    async save(episodeId, wav) {
      const { invoke } = await import('@tauri-apps/api/core')
      return invoke<string>('podcast_save', wav, { headers: { 'x-episode-id': episodeId } })
    },
    async url(ref) {
      const { convertFileSrc } = await import('@tauri-apps/api/core')
      return convertFileSrc(ref)
    },
    async remove(ref) {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('podcast_remove', { path: ref })
    },
  }
}

const DB_NAME = 'open-notebook-dev-audio'

function audioDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1)
    req.onupgradeneeded = () => req.result.createObjectStore('audio')
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error)
  })
}

function browserAudioStore(): AudioStore {
  const urls = new Map<string, string>()
  const tx = async <T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>) => {
    const db = await audioDb()
    return new Promise<T>((resolve, reject) => {
      const req = run(db.transaction('audio', mode).objectStore('audio'))
      req.onsuccess = () => resolve(req.result)
      req.onerror = () => reject(req.error)
    })
  }
  return {
    async save(episodeId, wav) {
      await tx('readwrite', (s) => s.put(new Blob([wav as BlobPart], { type: 'audio/wav' }), episodeId))
      return episodeId
    },
    async url(ref) {
      if (!urls.has(ref)) {
        const blob = await tx<Blob | undefined>('readonly', (s) => s.get(ref))
        if (!blob) throw new Error('Audio not found')
        urls.set(ref, URL.createObjectURL(blob))
      }
      return urls.get(ref)!
    },
    async remove(ref) {
      await tx('readwrite', (s) => s.delete(ref))
    },
  }
}

/**
 * Opens a WebSocket from Rust through the Tauri plugin, which, unlike the
 * browser WebSocket, can send the Authorization header OpenAI Live needs.
 */
export const openSocket: OpenSocket | undefined = isTauri
  ? async (url, headers) => {
      const { default: WebSocket } = await import('@tauri-apps/plugin-websocket')
      const ws = await WebSocket.connect(url, { headers })
      const queue: string[] = []
      let closed = false
      let wake: (() => void) | undefined
      ws.addListener((message) => {
        if (message.type === 'Text') queue.push(message.data)
        else if (message.type === 'Close') closed = true
        wake?.()
      })
      const socket: LiveSocket = {
        async send(text) {
          await ws.send(text)
        },
        async next(timeoutMs) {
          if (queue.length) return queue.shift()!
          if (closed) throw new Error('OpenAI Live closed the connection')
          await new Promise<void>((resolve) => {
            const timer = setTimeout(resolve, timeoutMs)
            wake = () => {
              clearTimeout(timer)
              resolve()
            }
          })
          wake = undefined
          if (queue.length) return queue.shift()!
          if (closed) throw new Error('OpenAI Live closed the connection')
          return null
        },
        async close() {
          await ws.disconnect().catch(() => undefined)
        },
      }
      return socket
    }
  : undefined

/** Original source files: the app data folder in the app, IndexedDB in browser dev. */
export function createFileStore(): FileStore {
  if (isTauri) {
    return {
      async save(sourceId, data, name) {
        const { invoke } = await import('@tauri-apps/api/core')
        return invoke<string>('file_save', data, {
          headers: { 'x-source-id': encodeURIComponent(sourceId), 'x-file-name': encodeURIComponent(name) },
        })
      },
      async read(ref) {
        const { invoke } = await import('@tauri-apps/api/core')
        return invoke<ArrayBuffer>('file_read', { path: ref })
      },
      async remove(ref) {
        const { invoke } = await import('@tauri-apps/api/core')
        await invoke('file_remove', { path: ref })
      },
    }
  }
  const store = browserAudioStore()
  return {
    async save(sourceId, data) {
      return store.save(`file:${sourceId}`, data)
    },
    async read(ref) {
      const url = await store.url(ref)
      return (await fetch(url)).arrayBuffer()
    },
    remove: (ref) => store.remove(ref),
  }
}

/** Saves a file to Downloads and shows it in Finder; in browser dev, a normal download. */
export async function saveDownload(name: string, data: ArrayBuffer, type: string): Promise<void> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core')
    const path = await invoke<string>('download_save', new Uint8Array(data), { headers: { 'x-file-name': encodeURIComponent(name) } })
    const { revealItemInDir } = await import('@tauri-apps/plugin-opener')
    await revealItemInDir(path)
    return
  }
  const url = URL.createObjectURL(new Blob([data], { type }))
  const a = document.createElement('a')
  a.href = url
  a.download = name
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 5000)
}

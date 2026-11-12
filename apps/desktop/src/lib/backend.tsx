import { LocalBackend, RemoteBackend, type Backend } from '@open-notebook/engine'
import { useQueryClient } from '@tanstack/react-query'
import { createContext, useContext, useEffect, useState, type ReactNode } from 'react'

import { openDatabase, platformFetch } from '../platform/index.ts'
import { makeUrlExtractor } from '../platform/extract.ts'
import { createAudioStore, createFileStore, openSocket } from '../platform/media.ts'
import { aiConfig, getSecret, secretName, useSecretVersion, useSettings } from './settings.ts'

type BackendState =
  | { status: 'loading' }
  | { status: 'ready'; backend: Backend }
  | { status: 'error'; error: Error }

const BackendContext = createContext<BackendState>({ status: 'loading' })

let localBackend: Promise<LocalBackend> | undefined

/** One LocalBackend per app run: it owns the source-processing queue. */
function getLocalBackend(): Promise<LocalBackend> {
  localBackend ??= (async () => {
    const [db, doFetch] = await Promise.all([openDatabase(), platformFetch()])
    return LocalBackend.open({
      db,
      fetch: doFetch,
      extractUrl: makeUrlExtractor(doFetch),
      getAiConfig: aiConfig,
      audio: createAudioStore(),
      files: createFileStore(),
      openSocket,
    })
  })()
  return localBackend
}

export function BackendProvider({ children }: { children: ReactNode }) {
  const settings = useSettings()
  const secretVersion = useSecretVersion()
  const queryClient = useQueryClient()
  const [state, setState] = useState<BackendState>({ status: 'loading' })

  useEffect(() => {
    let cancelled = false
    setState({ status: 'loading' })
    ;(async () => {
      if (settings.mode === 'local') return getLocalBackend()
      const [doFetch, password] = await Promise.all([platformFetch(), getSecret(secretName.serverPassword)])
      return new RemoteBackend({ baseUrl: settings.serverUrl, password: password ?? undefined, fetch: doFetch })
    })()
      .then((backend) => {
        if (cancelled) return
        queryClient.clear()
        setState({ status: 'ready', backend })
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ status: 'error', error: error instanceof Error ? error : new Error(String(error)) })
      })
    return () => {
      cancelled = true
    }
    // Remote password changes need a new client; local settings are read per call.
  }, [settings.mode, settings.serverUrl, settings.mode === 'remote' ? secretVersion : 0, queryClient])

  return <BackendContext.Provider value={state}>{children}</BackendContext.Provider>
}

export function useBackendState(): BackendState {
  return useContext(BackendContext)
}

/** For components rendered only once the backend is ready. */
export function useBackend(): Backend {
  const state = useContext(BackendContext)
  if (state.status !== 'ready') throw new Error('Backend not ready')
  return state.backend
}

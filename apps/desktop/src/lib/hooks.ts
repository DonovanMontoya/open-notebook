import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from './backend.tsx'

export const keys = {
  notebooks: ['notebooks'] as const,
  notebook: (id: string) => ['notebook', id] as const,
  sources: (notebookId: string) => ['sources', notebookId] as const,
  source: (id: string) => ['source', id] as const,
  notes: (notebookId: string) => ['notes', notebookId] as const,
  sessions: (notebookId: string) => ['sessions', notebookId] as const,
  session: (id: string) => ['session', id] as const,
  transformations: ['transformations'] as const,
  insights: (sourceId: string) => ['insights', sourceId] as const,
}

export function useNotebooks() {
  const backend = useBackend()
  return useQuery({ queryKey: keys.notebooks, queryFn: () => backend.listNotebooks() })
}

export function useNotebook(id: string) {
  const backend = useBackend()
  return useQuery({ queryKey: keys.notebook(id), queryFn: () => backend.getNotebook(id) })
}

const PROCESSING = new Set(['processing', 'queued', 'running', 'new', 'pending'])

export function isProcessing(status?: string) {
  return !!status && PROCESSING.has(status)
}

export function useSources(notebookId: string) {
  const backend = useBackend()
  return useQuery({
    queryKey: keys.sources(notebookId),
    queryFn: () => backend.listSources(notebookId),
    // Poll while anything is still being processed.
    refetchInterval: (query) => (query.state.data?.some((s) => isProcessing(s.status)) ? 2000 : false),
  })
}

export function useSource(id: string | undefined) {
  const backend = useBackend()
  return useQuery({ queryKey: keys.source(id ?? ''), queryFn: () => backend.getSource(id!), enabled: !!id })
}

export function useNotes(notebookId: string) {
  const backend = useBackend()
  return useQuery({ queryKey: keys.notes(notebookId), queryFn: () => backend.listNotes(notebookId) })
}

export function useChatSessions(notebookId: string) {
  const backend = useBackend()
  return useQuery({ queryKey: keys.sessions(notebookId), queryFn: () => backend.listChatSessions(notebookId) })
}

export function useChatSession(id: string | undefined) {
  const backend = useBackend()
  return useQuery({ queryKey: keys.session(id ?? ''), queryFn: () => backend.getChatSession(id!), enabled: !!id })
}

/** Invalidates everything that summarizes a notebook after a change inside it. */
export function useRefreshNotebook() {
  const qc = useQueryClient()
  return (notebookId: string) => {
    void qc.invalidateQueries({ queryKey: keys.notebooks })
    void qc.invalidateQueries({ queryKey: keys.notebook(notebookId) })
    void qc.invalidateQueries({ queryKey: keys.sources(notebookId) })
    void qc.invalidateQueries({ queryKey: keys.notes(notebookId) })
  }
}

export function useCreateNotebook() {
  const backend = useBackend()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; description?: string }) => backend.createNotebook(input.name, input.description),
    onSuccess: () => qc.invalidateQueries({ queryKey: keys.notebooks }),
  })
}

export function useTransformations() {
  const backend = useBackend()
  return useQuery({ queryKey: keys.transformations, queryFn: () => backend.listTransformations() })
}

/** Insights for a source; polls while one is being written or was just requested. */
export function useInsights(sourceId: string, expectingUntil = 0) {
  const backend = useBackend()
  return useQuery({
    queryKey: keys.insights(sourceId),
    queryFn: () => backend.listInsights(sourceId),
    refetchInterval: (q) => (q.state.data?.some((i) => i.status === 'running') || Date.now() < expectingUntil ? 2500 : false),
  })
}

/** State kept in localStorage under a key, such as per-notebook chat context. */
export function useStoredState<T>(key: string, initial: T): [T, (value: T) => void] {
  const [value, setValue] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key)
      return raw ? (JSON.parse(raw) as T) : initial
    } catch {
      return initial
    }
  })
  const set = (next: T) => {
    setValue(next)
    localStorage.setItem(key, JSON.stringify(next))
  }
  return [value, set]
}

/** Every source on this device or server, for the library and the add-from-library tab. */
export const librarySourcesKey = ['library-sources'] as const

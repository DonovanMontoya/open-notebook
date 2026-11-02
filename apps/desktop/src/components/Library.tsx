import type { SourceSummary } from '@open-notebook/api-client'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { hostOf, relativeTime } from '../lib/format.ts'
import { librarySourcesKey, useNotebooks } from '../lib/hooks.ts'
import { EmptyState, ErrorNote, Icon, Menu, Spinner } from './ui.tsx'


/** Every source in one place: which notebooks use it, and adding it to more. */
export function LibraryView({ onOpen, onBack }: { onOpen: (sourceId: string, notebookId: string) => void; onBack?: () => void }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const notebooks = useNotebooks()
  const sources = useQuery({ queryKey: librarySourcesKey, queryFn: () => backend.listAllSources() })
  const [filter, setFilter] = useState('')
  const names = useMemo(() => new Map((notebooks.data ?? []).map((n) => [n.id, n.name])), [notebooks.data])

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: librarySourcesKey })
    void qc.invalidateQueries({ queryKey: ['sources'] })
    void qc.invalidateQueries({ queryKey: ['notebooks'] })
  }
  const link = useMutation({ mutationFn: (v: { sourceId: string; notebookId: string }) => backend.linkSource(v.sourceId, v.notebookId), onSuccess: refresh })
  const unlink = useMutation({ mutationFn: (v: { sourceId: string; notebookId: string }) => backend.unlinkSource(v.sourceId, v.notebookId), onSuccess: refresh })

  const shown = (sources.data ?? []).filter((s) => !filter || (s.title ?? '').toLowerCase().includes(filter.toLowerCase()))

  return (
    <div className="column library">
      {onBack && (
        <button className="button quiet small back-button" onClick={onBack}>
          <Icon name="back" size={14} /> Notebooks
        </button>
      )}
      <h1 className="page-title">All sources</h1>
      <p className="page-intro">A source can be in more than one notebook. Add it where it helps; removing it from a notebook keeps it here.</p>
      <div className="search-box library-filter">
        <Icon name="search" size={14} />
        <input type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter by title" aria-label="Filter sources" />
      </div>
      {sources.isPending && <Spinner />}
      <ErrorNote error={sources.error ?? link.error ?? unlink.error} />
      {sources.data?.length === 0 && <EmptyState title="No sources yet">Sources you add to any notebook show up here.</EmptyState>}
      <ul className="library-list">
        {shown.map((s) => (
          <LibraryRow
            key={s.id}
            source={s}
            names={names}
            allNotebooks={(notebooks.data ?? []).map((n) => ({ id: n.id, name: n.name }))}
            onOpen={onOpen}
            onLink={(notebookId) => link.mutate({ sourceId: s.id, notebookId })}
            onUnlink={(notebookId) => unlink.mutate({ sourceId: s.id, notebookId })}
          />
        ))}
      </ul>
    </div>
  )
}

function LibraryRow({
  source,
  names,
  allNotebooks,
  onOpen,
  onLink,
  onUnlink,
}: {
  source: SourceSummary
  names: Map<string, string>
  allNotebooks: Array<{ id: string; name: string }>
  onOpen: (sourceId: string, notebookId: string) => void
  onLink: (notebookId: string) => void
  onUnlink: (notebookId: string) => void
}) {
  const inNotebooks = source.notebooks ?? []
  const addable = allNotebooks.filter((n) => !inNotebooks.includes(n.id))
  return (
    <li className="library-row">
      <button className="library-title" onClick={() => inNotebooks[0] && onOpen(source.id, inNotebooks[0])} disabled={!inNotebooks[0]}>
        {source.title || 'Untitled source'}
      </button>
      <p className="episode-meta">
        {source.asset?.url ? `${hostOf(source.asset.url)}, ` : ''}
        added {relativeTime(source.created)}
        {source.insights_count ? `, ${source.insights_count} ${source.insights_count === 1 ? 'insight' : 'insights'}` : ''}
      </p>
      <div className="chips">
        {inNotebooks.map((id) => (
          <span key={id} className="chip">
            <button onClick={() => onOpen(source.id, id)}>{names.get(id) ?? 'Notebook'}</button>
            {inNotebooks.length > 1 && (
              <button className="chip-remove" onClick={() => onUnlink(id)} aria-label={`Remove from ${names.get(id)}`} title="Remove from this notebook">
                <Icon name="close" size={12} />
              </button>
            )}
          </span>
        ))}
        {addable.length > 0 && (
          <Menu title="Add to notebook" icon="plus" label="Add to" value="" options={addable.map((n) => ({ value: n.id, label: n.name }))} onChange={onLink} />
        )}
      </div>
    </li>
  )
}

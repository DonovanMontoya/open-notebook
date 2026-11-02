import type { Note } from '@open-notebook/api-client'
import { parseLinks, type ContextSelection, type SearchHit } from '@open-notebook/engine'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { useNotebook, useNotes, useRefreshNotebook, useSources, useStoredState } from '../lib/hooks.ts'
import type { OpenTarget } from './Ask.tsx'
import { Chat } from './Chat.tsx'
import { Listen } from './Listen.tsx'
import { Study } from './Study.tsx'
import { Notes, type NoteDraft } from './Notes.tsx'
import { AddSourceDialog, SourcesRail, type RailView } from './Sources.tsx'
import { ErrorNote, Icon, Modal, Spinner, confirmAction } from './ui.tsx'

type Tab = 'ask' | 'notes' | 'study' | 'listen' | 'sources'

export function NotebookView({
  id,
  open,
  onBack,
  onDeleted,
  onSettings,
}: {
  id: string
  open?: OpenTarget
  onBack: () => void
  onDeleted: () => void
  onSettings: () => void
}) {
  const notebook = useNotebook(id)
  const sources = useSources(id)
  const [tab, setTab] = useState<Tab>(open?.kind === 'note' ? 'notes' : 'ask')
  const [rail, setRail] = useState<RailView>(open?.kind === 'source' ? { mode: 'source', id: open.id, query: open.query } : { mode: 'list' })
  const [context, setContext] = useStoredState<ContextSelection>(`open-notebook-context:${id}`, {})
  const [activeCite, setActiveCite] = useState<string>()
  const [editingNote, setEditingNote] = useState<Note | NoteDraft | NoteRef | undefined>(
    open?.kind === 'note' ? { noteId: open.id } : undefined,
  )
  const [adding, setAdding] = useState<false | { files?: File[]; links?: string[] }>(false)
  const [dragging, setDragging] = useState(false)
  const dragDepth = useRef(0)
  const [editingDetails, setEditingDetails] = useState(false)

  const sourceList = sources.data ?? []
  const citedIds = useMemo(() => new Set(rail.mode === 'source' ? [rail.id] : []), [rail])

  const openSource = (sourceId: string, query?: string) => {
    setRail({ mode: 'source', id: sourceId, query })
    if (window.matchMedia('(max-width: 860px)').matches) setTab('sources')
  }

  const tabs: Array<[Tab, string]> = [
    ['ask', 'Ask'],
    ['notes', 'Notes'],
    ['study', 'Study'],
    ['listen', 'Listen'],
  ]

  return (
    <div
      className={`notebook tab-${tab}`}
      style={{ height: '100%' }}
      onDragEnter={(e) => {
        if (!hasDroppable(e.dataTransfer)) return
        dragDepth.current++
        setDragging(true)
      }}
      onDragLeave={() => {
        dragDepth.current = Math.max(0, dragDepth.current - 1)
        if (!dragDepth.current) setDragging(false)
      }}
      onDragOver={(e) => {
        if (hasDroppable(e.dataTransfer)) e.preventDefault()
      }}
      onDrop={(e) => {
        dragDepth.current = 0
        setDragging(false)
        if (adding) return
        e.preventDefault()
        const files = [...e.dataTransfer.files]
        const links = parseLinks(e.dataTransfer.getData('text/uri-list') || e.dataTransfer.getData('text/plain'))
        if (files.length || links.length) setAdding({ files, links })
      }}
    >
      {dragging && !adding && (
        <div className="drop-overlay" aria-hidden>
          <p>Drop to add to {notebook.data?.name ?? 'this notebook'}</p>
        </div>
      )}
      <section className="desk" aria-label={notebook.data?.name}>
        <div className="drag" />
        <header className="desk-header" data-drag>
          <button className="icon-button back-button" onClick={onBack} aria-label="All notebooks">
            <Icon name="back" />
          </button>
          <div className="desk-title">
            {notebook.isPending ? <Spinner /> : <h1 data-own-dblclick onDoubleClick={() => setEditingDetails(true)}>{notebook.data?.name}</h1>}
            {notebook.data?.description && <p>{notebook.data.description}</p>}
          </div>
          <button className="icon-button" onClick={() => setEditingDetails(true)} aria-label="Notebook settings" title="Notebook settings">
            <Icon name="settings" />
          </button>
          <div className="desk-tools">
            <div className="segmented desk-tabs-wide" role="tablist">
              {tabs.map(([t, label]) => (
                <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? 'active' : ''} onClick={() => setTab(t)}>
                  {label}
                </button>
              ))}
            </div>
            <NotebookSearch
              notebookId={id}
              onSource={(hit, q) => openSource(hit.id, q)}
              onNote={(noteId) => {
                setTab('notes')
                setEditingNote({ noteId })
              }}
            />
          </div>
        </header>
        <div className="mobile-tabs">
          <div className="segmented" role="tablist">
            {[...tabs, ['sources', 'Sources'] as [Tab, string]].map(([t, label]) => (
              <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? 'active' : ''} onClick={() => setTab(t)}>
                {label}
              </button>
            ))}
          </div>
        </div>
        <ErrorNote error={notebook.error} />

        <div className="desk-body" style={tab === 'ask' ? { overflow: 'hidden' } : undefined}>
          {tab === 'notes' ? (
            <NotesTab notebookId={id} editing={editingNote} onEdit={setEditingNote} context={context} onContext={setContext} />
          ) : tab === 'study' ? (
            <Study notebookId={id} sources={sourceList} onOpenSource={(sourceId) => openSource(sourceId)} />
          ) : tab === 'listen' ? (
            <Listen notebookId={id} notebookName={notebook.data?.name} onSettings={onSettings} />
          ) : (
            <Chat
              scope={{ kind: 'notebook', notebookId: id, context }}
              sources={sourceList}
              activeCite={activeCite}
              onCite={(target) => {
                setActiveCite(target.key)
                openSource(target.sourceId, target.query)
              }}
              onSaveNote={(content) => {
                setEditingNote({ content, note_type: 'ai' })
                setTab('notes')
              }}
              onAddSource={() => setAdding({})}
            />
          )}
        </div>
      </section>

      <SourcesRail
        notebookId={id}
        sources={sourceList}
        loading={sources.isPending}
        error={sources.error}
        view={rail}
        citedIds={citedIds}
        onView={(v) => {
          setRail(v)
          if (v.mode === 'list') setActiveCite(undefined)
        }}
        onAdd={() => setAdding({})}
        context={context}
        onContext={setContext}
      />

      {adding && (
        <AddSourceDialog notebookId={id} initialFiles={adding.files} initialLinks={adding.links} onClose={() => setAdding(false)} />
      )}
      {editingDetails && notebook.data && (
        <NotebookDetails
          id={id}
          name={notebook.data.name}
          description={notebook.data.description}
          archived={notebook.data.archived}
          onClose={() => setEditingDetails(false)}
          onDeleted={onDeleted}
        />
      )}
    </div>
  )
}

/** A note picked from search: only its id is known until the list loads. */
type NoteRef = { noteId: string }

function NotesTab({
  notebookId,
  editing,
  onEdit,
  context,
  onContext,
}: {
  notebookId: string
  editing?: Note | NoteDraft | NoteRef
  onEdit: (n?: Note | NoteDraft) => void
  context: ContextSelection
  onContext: (c: ContextSelection) => void
}) {
  const notes = useNotes(notebookId)
  const resolved = editing && 'noteId' in editing ? notes.data?.find((n) => n.id === editing.noteId) : editing
  if (editing && !resolved) return <Spinner />
  return (
    <Notes
      notebookId={notebookId}
      editing={resolved}
      onEdit={onEdit}
      included={(noteId) => context.notes?.[noteId] !== false}
      onInclude={(noteId, on) => onContext({ ...context, notes: { ...context.notes, [noteId]: on } })}
    />
  )
}

function NotebookSearch({
  notebookId,
  onSource,
  onNote,
}: {
  notebookId: string
  onSource: (hit: SearchHit, query: string) => void
  onNote: (id: string) => void
}) {
  const backend = useBackend()
  const [query, setQuery] = useState('')
  const [debounced, setDebounced] = useState('')
  const [open, setOpen] = useState(false)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const t = setTimeout(() => setDebounced(query.trim()), 300)
    return () => clearTimeout(t)
  }, [query])

  useEffect(() => {
    const close = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [])

  const results = useQuery({
    queryKey: ['search', notebookId, debounced],
    queryFn: () => backend.search(debounced, notebookId),
    enabled: debounced.length >= 2,
  })

  return (
    <div className="search-box" ref={box}>
      <Icon name="search" size={14} />
      <input
        type="search"
        value={query}
        placeholder="Search notebook"
        aria-label="Search this notebook"
        onFocus={() => setOpen(true)}
        onChange={(e) => {
          setQuery(e.target.value)
          setOpen(true)
        }}
        onKeyDown={(e) => e.key === 'Escape' && setOpen(false)}
      />
      {open && debounced.length >= 2 && (
        <div className="search-results" role="listbox">
          {results.isPending && <Spinner />}
          <ErrorNote error={results.error} />
          {results.data?.length === 0 && <p className="muted" style={{ padding: 8 }}>Nothing matches “{debounced}”</p>}
          {results.data?.map((hit) => (
            <button
              key={`${hit.kind}:${hit.id}`}
              className="search-hit"
              onClick={() => {
                setOpen(false)
                if (hit.kind === 'source') onSource(hit, debounced)
                else onNote(hit.id)
              }}
            >
              <span className="search-hit-title">
                <Icon name={hit.kind === 'note' ? 'note' : 'file'} size={14} />
                {hit.title}
              </span>
              {hit.snippet && <span className="search-hit-snippet">{hit.snippet}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

function NotebookDetails(props: {
  id: string
  name: string
  description: string
  archived: boolean
  onClose: () => void
  onDeleted: () => void
}) {
  const backend = useBackend()
  const refresh = useRefreshNotebook()
  const [name, setName] = useState(props.name)
  const [description, setDescription] = useState(props.description)

  const save = useMutation({
    mutationFn: (archived?: boolean) => backend.updateNotebook(props.id, { name, description, archived }),
    onSuccess: () => {
      refresh(props.id)
      props.onClose()
    },
  })
  const remove = useMutation({
    mutationFn: () => backend.deleteNotebook(props.id),
    onSuccess: () => {
      refresh(props.id)
      props.onDeleted()
    },
  })

  return (
    <Modal
      title="Notebook"
      onClose={props.onClose}
      footer={
        <>
          <button
            className="button quiet danger"
            onClick={() =>
              void confirmAction(`Delete “${props.name}”? Its sources, notes and conversations are deleted too.`).then((ok) => ok && remove.mutate())
            }
          >
            Delete
          </button>
          <button className="button quiet" onClick={() => save.mutate(!props.archived)}>
            {props.archived ? 'Unarchive' : 'Archive'}
          </button>
          <span className="spacer" />
          <button className="button" onClick={props.onClose}>
            Cancel
          </button>
          <button className="button primary" onClick={() => save.mutate(undefined)} disabled={!name.trim() || save.isPending}>
            Save
          </button>
        </>
      }
    >
      <div className="form">
        <label className="field">
          <span>Name</span>
          <input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span>What it's for</span>
          <textarea rows={3} value={description} onChange={(e) => setDescription(e.target.value)} />
        </label>
        <ErrorNote error={save.error ?? remove.error} />
      </div>
    </Modal>
  )
}

function hasDroppable(data: DataTransfer | null): boolean {
  return !!data && [...data.types].some((t) => t === 'Files' || t === 'text/uri-list')
}

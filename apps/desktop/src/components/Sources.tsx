import type { SourceSummary } from '@open-notebook/api-client'
import { bestPassage, isMedia, parseLinks, type ContextSelection } from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { hostOf, relativeTime } from '../lib/format.ts'
import { isProcessing, keys, librarySourcesKey, useInsights, useRefreshNotebook, useSource, useTransformations } from '../lib/hooks.ts'
import { readFile } from '../platform/extract.ts'
import { OFFICE_FILE } from '../platform/extract.ts'
import { saveDownload } from '../platform/media.ts'
import { Chat } from './Chat.tsx'
import { EmptyState, ErrorNote, Icon, Markdown, Menu, Modal, Spinner, openExternal, confirmAction } from './ui.tsx'

export type RailView = { mode: 'list' } | { mode: 'source'; id: string; query?: string }

export function SourcesRail({
  notebookId,
  sources,
  loading,
  error,
  view,
  citedIds,
  onView,
  onAdd,
  context,
  onContext,
}: {
  notebookId: string
  sources: SourceSummary[]
  loading: boolean
  error: unknown
  view: RailView
  citedIds: Set<string>
  onView: (view: RailView) => void
  onAdd: () => void
  context: ContextSelection
  onContext: (context: ContextSelection) => void
}) {
  if (view.mode === 'source') {
    return <SourceReader key={view.id} id={view.id} query={view.query} notebookId={notebookId} onBack={() => onView({ mode: 'list' })} />
  }
  return (
    <aside className="rail" aria-label="Sources">
      <div className="drag" />
      <header className="rail-header">
        <h2>
          {sources.length ? `${sources.length} ${sources.length === 1 ? 'source' : 'sources'}` : 'Sources'}
        </h2>
        <button className="button small" onClick={onAdd}>
          <Icon name="plus" size={14} /> Add
        </button>
      </header>
      <div className="rail-body">
        {loading && <Spinner />}
        <ErrorNote error={error} />
        {!loading && sources.length === 0 && (
          <EmptyState title="Nothing to read yet">Add a web page, a PDF or pasted text. Everything is read and indexed on this device.</EmptyState>
        )}
        {sources.map((s) => (
          <SourceRow
            key={s.id}
            source={s}
            cited={citedIds.has(s.id)}
            mode={context.sources?.[s.id] ?? (s.insights_count > 0 ? 'insights' : 'full')}
            onMode={(mode) => onContext({ ...context, sources: { ...context.sources, [s.id]: mode } })}
            onOpen={() => onView({ mode: 'source', id: s.id })}
          />
        ))}
      </div>
    </aside>
  )
}

type Mode = 'full' | 'insights' | 'off'

const MODE_LABEL: Record<Mode, string> = { full: 'Full text', insights: 'Insights', off: 'Left out' }
const MODE_HINT: Record<Mode, string> = {
  full: 'Chat reads the whole source',
  insights: 'Chat reads only this source’s insights',
  off: 'Chat ignores this source',
}

function SourceRow({
  source,
  cited,
  mode,
  onMode,
  onOpen,
}: {
  source: SourceSummary
  cited: boolean
  mode: Mode
  onMode: (mode: Mode) => void
  onOpen: () => void
}) {
  // Insights are only an option once the source has some.
  const cycle: Mode[] = source.insights_count > 0 ? ['insights', 'full', 'off'] : ['full', 'off']
  const next = cycle[(cycle.indexOf(mode) + 1) % cycle.length]
  const backend = useBackend()
  const qc = useQueryClient()
  const retry = useMutation({
    mutationFn: () => backend.retrySource(source.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['sources'] }),
  })
  const failed = source.status === 'failed' || source.status === 'error'
  const error = (source.processing_info as { error?: string } | undefined)?.error
  const icon = source.asset?.url ? 'link' : source.asset?.file_path ? 'file' : 'text'

  return (
    <div className={`source-line${mode === 'off' && !failed ? ' off' : ''}`}>
      <button className={`source-row${cited ? ' cited' : ''}`} onClick={onOpen}>
        <span className="source-row-icon">
          <Icon name={icon} />
        </span>
        <span className="source-row-text">
          <span className="source-row-title">{source.title || 'Untitled source'}</span>
          <span className="source-row-meta">
            {isProcessing(source.status) ? (
              <>
                <Spinner /> Reading and indexing
              </>
            ) : failed ? (
              <span className="danger" title={error}>
                {error ?? 'Could not be read'}
              </span>
            ) : (
              <span>
                {source.asset?.url ? `${hostOf(source.asset.url)}, ` : ''}
                added {relativeTime(source.created)}
              </span>
            )}
          </span>
        </span>
      </button>
      {failed ? (
        <button className="icon-button" onClick={() => retry.mutate()} disabled={retry.isPending} title="Try again" aria-label="Try again">
          <Icon name="retry" />
        </button>
      ) : (
        <button
          className={`context-mode ${mode}`}
          onClick={() => onMode(next)}
          title={`${MODE_HINT[mode]}. Click for ${MODE_LABEL[next].toLowerCase()}.`}
          aria-label={`${source.title ?? 'Source'} in chat: ${MODE_LABEL[mode]}`}
        >
          {MODE_LABEL[mode]}
        </button>
      )}
    </div>
  )
}

function SourceReader({ id, query, notebookId, onBack }: { id: string; query?: string; notebookId: string; onBack: () => void }) {
  const backend = useBackend()
  const refresh = useRefreshNotebook()
  const qc = useQueryClient()
  const source = useSource(id)
  const [tab, setTab] = useState<'read' | 'ask'>('read')
  const [renaming, setRenaming] = useState<string>()
  const rename = useMutation({
    mutationFn: (title: string) => backend.updateSource(id, { title }),
    onSuccess: () => {
      setRenaming(undefined)
      void qc.invalidateQueries({ queryKey: keys.source(id) })
      refresh(notebookId)
    },
  })
  const [focus, setFocus] = useState(query)
  const mark = useRef<HTMLElement>(null)
  const s = source.data
  const url = s?.asset?.url
  const text = s?.full_text ?? ''
  const passage = useMemo(() => (focus ? bestPassage(text, focus) : undefined), [text, focus])

  useEffect(() => {
    mark.current?.scrollIntoView({ block: 'center' })
  }, [passage, text])

  const shared = (source.data?.notebooks?.length ?? 0) > 1
  const download = useMutation({
    mutationFn: async () => {
      const file = await backend.downloadSource(id)
      if (!file) throw new Error('This source has no original file.')
      await saveDownload(file.name, file.data, file.type)
    },
  })
  const remove = useMutation({
    // A source shared with other notebooks only leaves this one.
    mutationFn: () => (shared ? backend.unlinkSource(id, notebookId) : backend.deleteSource(id)),
    onSuccess: () => {
      qc.removeQueries({ queryKey: keys.source(id) })
      refresh(notebookId)
      onBack()
    },
  })

  return (
    <aside className="rail" aria-label="Source">
      <div className="drag" />
      <header className="rail-header">
        <button className="icon-button" onClick={onBack} aria-label="All sources" title="All sources">
          <Icon name="back" />
        </button>
        {renaming !== undefined ? (
          <form
            className="rename"
            onSubmit={(e) => {
              e.preventDefault()
              rename.mutate(renaming)
            }}
          >
            <input autoFocus value={renaming} onChange={(e) => setRenaming(e.target.value)} aria-label="Source title" onKeyDown={(e) => e.key === 'Escape' && setRenaming(undefined)} />
          </form>
        ) : (
          <h2 title="Double-click to rename" onDoubleClick={() => setRenaming(s?.title ?? '')}>
            {s?.title || 'Source'}
          </h2>
        )}
        {url && (
          <button className="icon-button" onClick={() => void openExternal(url)} title="Open original" aria-label="Open original">
            <Icon name="external" />
          </button>
        )}
      </header>
      <div className="rail-tabs">
        <div className="segmented" role="tablist">
          <button role="tab" aria-selected={tab === 'read'} className={tab === 'read' ? 'active' : ''} onClick={() => setTab('read')}>
            Read
          </button>
          <button role="tab" aria-selected={tab === 'ask'} className={tab === 'ask' ? 'active' : ''} onClick={() => setTab('ask')}>
            Ask
          </button>
        </div>
      </div>
      {tab === 'ask' ? (
        <div className="rail-chat">
          <Chat
            scope={{ kind: 'source', sourceId: id }}
            sources={s ? [s] : []}
            onCite={(target) => {
              setFocus(target.query)
              setTab('read')
            }}
            onSaveNote={(content) => void backend.createNote(notebookId, { content, note_type: 'ai' }).then(() => refresh(notebookId))}
            onAddSource={() => undefined}
          />
        </div>
      ) : (
      <div className="rail-body">
        {source.isPending && <Spinner />}
        <ErrorNote error={source.error ?? remove.error ?? download.error} />
        {s && (
          <div className="reader">
            <p className="reader-meta">
              {url ? `${hostOf(url)}, ` : ''}
              added {relativeTime(s.created)}
              {s.embedded_chunks ? `, ${s.embedded_chunks} ${s.embedded_chunks === 1 ? 'passage' : 'passages'} indexed` : ''}
            </p>
            <Insights sourceId={id} notebookId={notebookId} hasText={!!text} />
            {text ? (
              <div className="reader-text">
                {passage ? (
                  <>
                    {text.slice(0, passage.start)}
                    <mark ref={mark}>{text.slice(passage.start, passage.end)}</mark>
                    {text.slice(passage.end)}
                  </>
                ) : (
                  text
                )}
              </div>
            ) : (
              <EmptyState title={isProcessing(s.status) ? 'Still reading this source' : 'No text was found'} />
            )}
            <div className="reader-actions">
              {s.file_available && (
                <button className="button quiet small" onClick={() => download.mutate()} disabled={download.isPending}>
                  <Icon name="file" size={14} /> {download.isPending ? 'Saving' : 'Download original'}
                </button>
              )}
              <button
                className="button quiet small danger"
                onClick={() =>
                  void confirmAction(shared ? 'Remove this source from this notebook? It stays in your other notebooks.' : 'Delete this source? Its text, insights and chats are removed.', shared ? 'Remove' : 'Delete').then((ok) => ok && remove.mutate())
                }
                disabled={remove.isPending}
              >
                <Icon name="trash" size={14} /> {shared ? 'Remove from this notebook' : 'Delete source'}
              </button>
            </div>
          </div>
        )}
      </div>
      )}
    </aside>
  )
}

type Tab = 'link' | 'file' | 'text' | 'library'

const ACCEPT = [
  '.pdf,.md,.markdown,.txt,.html,.htm,.docx,.pptx,.xlsx,.epub,application/pdf,text/*,audio/*,video/*',
  'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  'application/epub+zip',
].join(',')
const isReadable = (f: File) =>
  /\.(pdf|md|markdown|txt|html?)$/i.test(f.name) || OFFICE_FILE.test(f.name) || f.type === 'application/pdf' || f.type.startsWith('text/') || isMedia(f.type, f.name)

type ItemState = { label: string; state: 'waiting' | 'adding' | 'added' | 'failed'; error?: string }

export function AddSourceDialog({
  notebookId,
  onClose,
  initialFiles,
  initialLinks,
}: {
  notebookId: string
  onClose: () => void
  initialFiles?: File[]
  initialLinks?: string[]
}) {
  const backend = useBackend()
  const refresh = useRefreshNotebook()
  const [tab, setTab] = useState<Tab>(initialFiles?.length ? 'file' : 'link')
  const [links, setLinks] = useState((initialLinks ?? []).join('\n'))
  const [title, setTitle] = useState('')
  const [text, setText] = useState('')
  const [files, setFiles] = useState<File[]>(initialFiles?.filter(isReadable) ?? [])
  const [items, setItems] = useState<ItemState[]>()
  const transformations = useTransformations()
  const [chosen, setChosen] = useState<Set<string>>()
  const selected = chosen ?? new Set((transformations.data ?? []).filter((t) => t.applyDefault).map((t) => t.id))
  const apply = selected.size ? [...selected] : undefined
  const [busy, setBusy] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const parsed = useMemo(() => parseLinks(links), [links])
  const library = useQuery({ queryKey: librarySourcesKey, enabled: tab === 'library', queryFn: () => backend.listAllSources() })
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const elsewhere = (library.data ?? []).filter((s) => !(s.notebooks ?? []).includes(notebookId))
  const count = tab === 'link' ? parsed.length : tab === 'file' ? files.length : tab === 'library' ? picked.size : text.trim() ? 1 : 0

  const addFiles = (list: FileList | File[] | null) => {
    if (!list) return
    const incoming = [...list].filter(isReadable)
    setFiles((current) => {
      const seen = new Set(current.map((f) => `${f.name}:${f.size}`))
      return [...current, ...incoming.filter((f) => !seen.has(`${f.name}:${f.size}`))]
    })
  }

  /** Adds each source in turn; one failure does not stop the rest. */
  const submit = async () => {
    if (!count || busy) return
    const jobs: Array<{ label: string; run: () => Promise<unknown> }> =
      tab === 'library'
        ? elsewhere.filter((s) => picked.has(s.id)).map((s) => ({ label: s.title ?? 'Source', run: () => backend.linkSource(s.id, notebookId) }))
        : tab === 'link'
        ? parsed.map((url) => ({
            label: url.replace(/^https?:\/\//, ''),
            run: () => backend.addSource(notebookId, { type: 'link', url, title: parsed.length === 1 ? title.trim() || undefined : undefined, transformations: apply }),
          }))
        : tab === 'file'
          ? files.map((file) => ({
              label: file.name,
              run: async () => {
                const base = file.name.replace(/\.[^.]+$/, '')
                const name = files.length === 1 && title.trim() ? title.trim() : undefined
                // A server reads files itself; on device, text is read here and audio is transcribed by the engine.
                if (backend.kind === 'remote') {
                  return backend.addSource(notebookId, { type: 'document', title: name ?? base, content: '', fileName: file.name, file, transformations: apply })
                }
                const extracted = await readFile(file)
                const media = isMedia(file.type, file.name)
                if (!media && !extracted.text.trim()) throw new Error('No readable text. Scanned PDFs need OCR, which this version does not do.')
                return backend.addSource(notebookId, {
                  type: 'document',
                  title: name ?? extracted.title ?? base,
                  content: extracted.text,
                  fileName: file.name,
                  file,
                  transformations: apply,
                })
              },
            }))
          : [{ label: title.trim() || 'Pasted text', run: () => backend.addSource(notebookId, { type: 'text', content: text, title: title.trim() || undefined, transformations: apply }) }]

    setBusy(true)
    const state: ItemState[] = jobs.map((j) => ({ label: j.label, state: 'waiting' }))
    setItems([...state])
    for (let i = 0; i < jobs.length; i++) {
      state[i] = { ...state[i], state: 'adding' }
      setItems([...state])
      try {
        await jobs[i].run()
        state[i] = { ...state[i], state: 'added' }
      } catch (error) {
        state[i] = { ...state[i], state: 'failed', error: error instanceof Error ? error.message : String(error) }
      }
      setItems([...state])
      refresh(notebookId)
    }
    setBusy(false)
    if (state.every((s) => s.state === 'added')) onClose()
  }

  const failed = items?.filter((i) => i.state === 'failed').length ?? 0
  const done = items?.filter((i) => i.state === 'added' || i.state === 'failed').length ?? 0
  const label = busy
    ? `Adding ${Math.min(done + 1, items?.length ?? 1)} of ${items?.length}`
    : count > 1
      ? `Add ${count} sources`
      : 'Add source'

  return (
    <Modal
      title="Add sources"
      onClose={busy ? () => undefined : onClose}
      footer={
        <>
          {items && !busy && failed > 0 && <span className="muted small">{failed} could not be added</span>}
          <span className="spacer" />
          <button className="button" onClick={onClose} disabled={busy}>
            {items && !busy ? 'Close' : 'Cancel'}
          </button>
          {!(items && !busy && failed === 0) && (
            <button className="button primary" onClick={() => void submit()} disabled={!count || busy}>
              {label}
            </button>
          )}
        </>
      }
    >
      {items ? (
        <ul className="batch">
          {items.map((item, i) => (
            <li key={i} className={`batch-item ${item.state}`}>
              <span className="batch-state">
                {item.state === 'adding' ? <Spinner /> : item.state === 'added' ? <Icon name="check" size={14} /> : item.state === 'failed' ? <Icon name="close" size={14} /> : null}
              </span>
              <span className="batch-label">
                {item.label}
                {item.error && <span className="batch-error">{item.error}</span>}
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <>
          <div className="segmented" role="tablist">
            {(['link', 'file', 'text', 'library'] as Tab[]).map((t) => (
              <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? 'active' : ''} onClick={() => setTab(t)}>
                {t === 'link' ? 'Web pages' : t === 'file' ? 'Files' : t === 'text' ? 'Text' : 'From other notebooks'}
              </button>
            ))}
          </div>
          <form
            className="form"
            onSubmit={(e) => {
              e.preventDefault()
              void submit()
            }}
          >
            {tab === 'link' && (
              <label className="field">
                <span>Addresses</span>
                <textarea
                  autoFocus
                  rows={5}
                  value={links}
                  onChange={(e) => setLinks(e.target.value)}
                  placeholder={'https://www.ftc.gov/reports/bringing-dark-patterns-light\nhttps://arxiv.org/pdf/2203.02155\nhttps://www.youtube.com/watch?v=…'}
                />
                <small>
                  {parsed.length > 1
                    ? `${parsed.length} links found. Each becomes its own source.`
                    : 'Paste one or more links, one per line. Articles, PDF links and YouTube videos (from their captions) all work.'}
                </small>
              </label>
            )}
            {tab === 'file' && (
              <div
                className="field drop-target"
                onDragOver={(e) => e.preventDefault()}
                onDrop={(e) => {
                  e.preventDefault()
                  addFiles(e.dataTransfer.files)
                }}
              >
                <span>Files</span>
                <div className="file-pick">
                  <button type="button" className="button" onClick={() => fileInput.current?.click()}>
                    Choose files
                  </button>
                  <span className="muted">
                    or drop them here. PDF, Word, PowerPoint, Excel, EPUB, Markdown, text, HTML, audio and video.
                  </span>
                </div>
                {files.length > 0 && (
                  <ul className="file-list">
                    {files.map((f, i) => (
                      <li key={`${f.name}:${f.size}`}>
                        <Icon name="file" size={14} />
                        <span className="file-name">{f.name}</span>
                        <span className="muted small">{formatBytes(f.size)}</span>
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={`Remove ${f.name}`}
                          onClick={() => setFiles((list) => list.filter((_, j) => j !== i))}
                        >
                          <Icon name="close" size={14} />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                <input
                  ref={fileInput}
                  type="file"
                  multiple
                  hidden
                  accept={ACCEPT}
                  onChange={(e) => {
                    addFiles(e.target.files)
                    e.target.value = ''
                  }}
                />
              </div>
            )}
            {tab === 'library' && (
              <div className="field">
                <span>Sources in your other notebooks</span>
                {library.isPending && <Spinner />}
                {library.data && elsewhere.length === 0 && <small>Every source is already in this notebook.</small>}
                <ul className="file-list">
                  {elsewhere.map((s) => (
                    <li key={s.id}>
                      <label className="check" style={{ flex: 1, minWidth: 0 }}>
                        <input
                          type="checkbox"
                          checked={picked.has(s.id)}
                          onChange={(e) => {
                            const next = new Set(picked)
                            if (e.target.checked) next.add(s.id)
                            else next.delete(s.id)
                            setPicked(next)
                          }}
                        />
                        <span className="file-name">{s.title || 'Untitled source'}</span>
                      </label>
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {tab === 'text' && (
              <label className="field">
                <span>Text</span>
                <textarea autoFocus rows={10} value={text} onChange={(e) => setText(e.target.value)} placeholder="Paste a transcript, an email or your own notes" />
              </label>
            )}
            {count <= 1 && tab !== 'library' && (
              <label className="field">
                <span>Title</span>
                <input
                  value={title}
                  onChange={(e) => setTitle(e.target.value)}
                  placeholder={tab === 'text' ? 'Optional, taken from the first line' : 'Optional, taken from the source'}
                />
              </label>
            )}
            {(transformations.data?.length ?? 0) > 0 && tab !== 'library' && (
              <fieldset className="field transform-pick">
                <legend>Make insights</legend>
                <div className="checks">
                  {transformations.data!.map((t) => (
                    <label key={t.id} className="check" title={t.description}>
                      <input
                        type="checkbox"
                        checked={selected.has(t.id)}
                        onChange={(e) => {
                          const next = new Set(selected)
                          if (e.target.checked) next.add(t.id)
                          else next.delete(t.id)
                          setChosen(next)
                        }}
                      />
                      {t.title}
                    </label>
                  ))}
                </div>
              </fieldset>
            )}
            <button type="submit" hidden />
          </form>
        </>
      )}
    </Modal>
  )
}

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** A source's insights, with a menu to run another transformation on it. */
function Insights({ sourceId, notebookId, hasText }: { sourceId: string; notebookId: string; hasText: boolean }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const refresh = useRefreshNotebook()
  const transformations = useTransformations()
  const [expecting, setExpecting] = useState(0)
  const insights = useInsights(sourceId, expecting)
  const [open, setOpen] = useState<string>()

  const run = useMutation({
    mutationFn: (transformationId: string) => backend.createInsight(sourceId, transformationId),
    onSuccess: () => {
      // Server insights appear when finished; keep checking for a while.
      setExpecting(Date.now() + 120_000)
      void qc.invalidateQueries({ queryKey: keys.insights(sourceId) })
    },
  })
  const remove = useMutation({
    mutationFn: (id: string) => backend.deleteInsight(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.insights(sourceId) })
      refresh(notebookId)
    },
  })
  const save = useMutation({
    mutationFn: (id: string) => backend.saveInsightAsNote(id, notebookId),
    onSuccess: () => refresh(notebookId),
  })

  const list = insights.data ?? []
  return (
    <section className="insights" aria-label="Insights">
      <div className="insights-head">
        <h3>Insights</h3>
        {hasText && transformations.data && transformations.data.length > 0 && (
          <Menu
            title="Run a transformation"
            icon="plus"
            label="Transform"
            value=""
            options={transformations.data.map((t) => ({ value: t.id, label: t.title }))}
            onChange={(id) => run.mutate(id)}
          />
        )}
      </div>
      {list.length === 0 && !run.isPending && (
        <p className="muted small">
          {hasText ? 'Run a transformation to get a summary, key points or questions from this source.' : 'Insights can be made once the text is ready.'}
        </p>
      )}
      {list.map((i) => (
        <article key={i.id} className={`insight ${i.status ?? 'completed'}`}>
          <button className="insight-head" onClick={() => setOpen(open === i.id ? undefined : i.id)} aria-expanded={open === i.id}>
            <span className="insight-type">{i.type}</span>
            {i.status === 'running' ? <Spinner /> : i.status === 'failed' ? <span className="danger small">Failed</span> : null}
          </button>
          {i.status === 'failed' && <p className="danger small">{i.error}</p>}
          {i.status !== 'running' && i.status !== 'failed' && (
            <div className={`insight-body${open === i.id ? ' open' : ''}`}>
              <Markdown>{i.content}</Markdown>
            </div>
          )}
          {open === i.id && i.status !== 'running' && (
            <div className="insight-actions">
              {i.status !== 'failed' && (
                <button className="button quiet small" onClick={() => save.mutate(i.id)} disabled={save.isPending}>
                  <Icon name="note" size={14} /> {save.isSuccess && save.variables === i.id ? 'Saved to notes' : 'Save as note'}
                </button>
              )}
              <button className="button quiet small danger" onClick={() => remove.mutate(i.id)}>
                <Icon name="trash" size={14} /> Delete
              </button>
            </div>
          )}
        </article>
      ))}
      <ErrorNote error={run.error ?? remove.error ?? save.error} />
    </section>
  )
}

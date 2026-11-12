import type { Note } from '@open-notebook/api-client'
import { isDeckNote } from '@open-notebook/engine'
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { relativeTime } from '../lib/format.ts'
import { useNotes, useRefreshNotebook } from '../lib/hooks.ts'
import { EmptyState, ErrorNote, Icon, Markdown, Spinner, confirmAction } from './ui.tsx'

export type NoteDraft = { content: string; title?: string; note_type?: 'human' | 'ai' }

/** Notes list, or the editor for one note (existing or new). */
export function Notes({
  notebookId,
  editing,
  onEdit,
  included,
  onInclude,
}: {
  notebookId: string
  editing?: Note | NoteDraft
  onEdit: (note: Note | NoteDraft | undefined) => void
  included: (noteId: string) => boolean
  onInclude: (noteId: string, on: boolean) => void
}) {
  const notes = useNotes(notebookId)

  if (editing) {
    return (
      <NoteEditor
        key={'id' in editing ? editing.id : 'new'}
        notebookId={notebookId}
        note={'id' in editing ? editing : undefined}
        draft={'id' in editing ? undefined : editing}
        onDone={() => onEdit(undefined)}
      />
    )
  }

  return (
    <div className="column">
      <div className="note-editor-bar" style={{ marginBottom: 8 }}>
        <span className="spacer" />
        <button className="button small" onClick={() => onEdit({ content: '' })}>
          <Icon name="plus" size={14} /> New note
        </button>
      </div>
      {notes.isPending && <Spinner />}
      <ErrorNote error={notes.error} />
      {notes.data?.length === 0 && (
        <EmptyState title="No notes yet">
          Write down what you're finding, or save a useful answer from Ask. Notes are included when you ask questions.
        </EmptyState>
      )}
      <div className="note-list">
        {notes.data?.filter((n) => !isDeckNote(n.content)).map((n) => (
          <div key={n.id} className="note-row">
            <button className="note-item" onClick={() => onEdit(n)}>
              <div className="note-item-title">{n.title || 'Untitled note'}</div>
              {n.content && <div className="note-item-excerpt">{plain(n.content)}</div>}
              <div className="note-item-meta">
                {n.note_type === 'ai' ? 'Saved from Ask, ' : ''}
                edited {relativeTime(n.updated)}
              </div>
            </button>
            <button
              className={`context-mode ${included(n.id) ? 'full' : 'off'}`}
              onClick={() => onInclude(n.id, !included(n.id))}
              title={included(n.id) ? 'Chat reads this note. Click to leave it out.' : 'Chat ignores this note. Click to include it.'}
            >
              {included(n.id) ? 'In chat' : 'Left out'}
            </button>
          </div>
        ))}
      </div>
    </div>
  )
}

function plain(markdown: string) {
  return markdown
    .replace(/[#>*_`~[\]]/g, '')
    .replace(/\(cite:\d+\)/g, '')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 240)
}

function NoteEditor({
  notebookId,
  note,
  draft,
  onDone,
}: {
  notebookId: string
  note?: Note
  draft?: NoteDraft
  onDone: () => void
}) {
  const backend = useBackend()
  const refresh = useRefreshNotebook()
  const [title, setTitle] = useState(note?.title ?? draft?.title ?? '')
  const [content, setContent] = useState(note?.content ?? draft?.content ?? '')
  const [preview, setPreview] = useState(!!note?.content || !!draft?.content)
  const dirty = !note || title !== (note.title ?? '') || content !== (note.content ?? '')

  const save = useMutation({
    mutationFn: () =>
      note
        ? backend.updateNote(note.id, { title, content })
        : backend.createNote(notebookId, { title: title || undefined, content, note_type: draft?.note_type ?? 'human' }),
    onSuccess: () => {
      refresh(notebookId)
      onDone()
    },
  })
  const remove = useMutation({
    mutationFn: () => backend.deleteNote(note!.id),
    onSuccess: () => {
      refresh(notebookId)
      onDone()
    },
  })

  const leave = async () => {
    if (dirty && content.trim() && !(await confirmAction('Discard your changes to this note?', 'Discard'))) return
    onDone()
  }

  return (
    <div className="column note-editor">
      <div className="note-editor-bar">
        <button className="button quiet small" onClick={leave}>
          <Icon name="back" size={14} /> All notes
        </button>
        <span className="spacer" />
        <div className="segmented">
          <button className={!preview ? 'active' : ''} onClick={() => setPreview(false)}>
            Write
          </button>
          <button className={preview ? 'active' : ''} onClick={() => setPreview(true)}>
            Read
          </button>
        </div>
      </div>
      <input className="title-input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Untitled note" aria-label="Title" />
      {preview ? (
        <div className="note-preview" onDoubleClick={() => setPreview(false)}>
          {content.trim() ? <Markdown>{content.replace(/\]\(cite:\d+\)/g, ']')}</Markdown> : <p className="muted">Empty note</p>}
        </div>
      ) : (
        <textarea
          className="note-textarea"
          autoFocus
          value={content}
          onChange={(e) => setContent(e.target.value)}
          placeholder="Write in Markdown"
          aria-label="Note"
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 's') {
              e.preventDefault()
              if (content.trim()) save.mutate()
            }
          }}
        />
      )}
      <ErrorNote error={save.error ?? remove.error} />
      <div className="note-editor-bar">
        {note && (
          <button className="button quiet small danger" onClick={() => void confirmAction('Delete this note?').then((ok) => ok && remove.mutate())}>
            <Icon name="trash" size={14} /> Delete
          </button>
        )}
        <span className="spacer" />
        <button className="button primary" onClick={() => save.mutate()} disabled={!content.trim() || !dirty || save.isPending}>
          {save.isPending ? 'Saving' : 'Save note'}
        </button>
      </div>
    </div>
  )
}

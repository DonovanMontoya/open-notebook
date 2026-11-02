import { useState } from 'react'

import type { Route } from '../App.tsx'
import { useBackend } from '../lib/backend.tsx'
import { hostOf } from '../lib/format.ts'
import { useCreateNotebook, useNotebooks } from '../lib/hooks.ts'
import { useSettings } from '../lib/settings.ts'
import { MiniPlayer } from './Player.tsx'
import { ErrorNote, Icon, Modal, Spinner } from './ui.tsx'

export function Sidebar({ route, navigate }: { route: Route; navigate: (r: Route) => void }) {
  const backend = useBackend()
  const settings = useSettings()
  const notebooks = useNotebooks()
  const [creating, setCreating] = useState(false)
  const [showArchived, setShowArchived] = useState(false)

  const active = (notebooks.data ?? []).filter((n) => !n.archived)
  const archived = (notebooks.data ?? []).filter((n) => n.archived)
  const isOpen = (id: string) => route.view === 'notebook' && route.id === id

  return (
    <nav className="sidebar" aria-label="Notebooks">
      <div className="drag" data-tauri-drag-region />
      <div className="brand" data-tauri-drag-region>
        <span className="brand-name">Open Notebook</span>
        <span className="brand-where" title={backend.kind === 'local' ? 'Stored on this device' : settings.serverUrl}>
          <Icon name={backend.kind === 'local' ? 'device' : 'server'} size={12} />
          {backend.kind === 'local' ? 'This device' : hostOf(settings.serverUrl).split('.')[0]}
        </span>
      </div>

      <div className="nav-top">
        <button className={`nav-item${route.view === 'ask' ? ' selected' : ''}`} onClick={() => navigate({ view: 'ask' })}>
          <Icon name="search" />
          <span className="nav-item-name">Ask and search</span>
        </button>
        <button className={`nav-item${route.view === 'library' ? ' selected' : ''}`} onClick={() => navigate({ view: 'library' })}>
          <Icon name="book" />
          <span className="nav-item-name">All sources</span>
        </button>
      </div>

      <div className="nav-label">
        <span>Notebooks</span>
        <button className="icon-button" onClick={() => setCreating(true)} aria-label="New notebook" title="New notebook">
          <Icon name="plus" />
        </button>
      </div>

      <div className="nav-list">
        {notebooks.isPending && (
          <div className="nav-hint">
            <Spinner />
          </div>
        )}
        <ErrorNote error={notebooks.error} />
        {active.map((n) => (
          <button
            key={n.id}
            className={`nav-item${isOpen(n.id) ? ' selected' : ''}`}
            onClick={() => navigate({ view: 'notebook', id: n.id })}
            aria-current={isOpen(n.id) ? 'page' : undefined}
          >
            <span className="nav-item-name">{n.name}</span>
            <span className="nav-item-count" title={`${n.source_count} sources`}>
              {n.source_count || ''}
            </span>
          </button>
        ))}
        {notebooks.isSuccess && active.length === 0 && <p className="nav-hint">Nothing here yet</p>}
        {archived.length > 0 && (
          <>
            <button className="nav-item" onClick={() => setShowArchived((v) => !v)}>
              <span className="nav-item-name muted">{showArchived ? 'Hide archived' : `Archived (${archived.length})`}</span>
            </button>
            {showArchived &&
              archived.map((n) => (
                <button
                  key={n.id}
                  className={`nav-item archived${isOpen(n.id) ? ' selected' : ''}`}
                  onClick={() => navigate({ view: 'notebook', id: n.id })}
                >
                  <span className="nav-item-name">{n.name}</span>
                </button>
              ))}
          </>
        )}
      </div>

      <MiniPlayer docked />

      <div className="nav-footer">
        <button
          className={`nav-item${route.view === 'transformations' ? ' selected' : ''}`}
          onClick={() => navigate({ view: 'transformations' })}
        >
          <Icon name="layers" />
          <span className="nav-item-name">Transformations</span>
        </button>
        <button
          className={`nav-item${route.view === 'usage' ? ' selected' : ''}`}
          onClick={() => navigate({ view: 'usage' })}
        >
          <Icon name="chart" />
          <span className="nav-item-name">Usage</span>
        </button>
        <button
          className={`nav-item${route.view === 'settings' ? ' selected' : ''}`}
          onClick={() => navigate({ view: 'settings' })}
        >
          <Icon name="settings" />
          <span className="nav-item-name">Settings</span>
        </button>
      </div>

      {creating && <NewNotebookDialog onClose={() => setCreating(false)} onCreated={(id) => navigate({ view: 'notebook', id })} />}
    </nav>
  )
}

function NewNotebookDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const create = useCreateNotebook()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')

  const submit = () => {
    if (!name.trim() || create.isPending) return
    create.mutate(
      { name: name.trim(), description: description.trim() || undefined },
      {
        onSuccess: (notebook) => {
          onClose()
          onCreated(notebook.id)
        },
      },
    )
  }

  return (
    <Modal
      title="New notebook"
      onClose={onClose}
      footer={
        <>
          <button className="button" onClick={onClose}>
            Cancel
          </button>
          <button className="button primary" onClick={submit} disabled={!name.trim() || create.isPending}>
            {create.isPending ? 'Creating' : 'Create notebook'}
          </button>
        </>
      }
    >
      <form
        className="form"
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <label className="field">
          <span>Name</span>
          <input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Dark patterns literature review" />
        </label>
        <label className="field">
          <span>What it's for</span>
          <textarea
            rows={3}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Optional. The assistant reads this before answering."
          />
        </label>
        <ErrorNote error={create.error} />
        <button type="submit" hidden />
      </form>
    </Modal>
  )
}

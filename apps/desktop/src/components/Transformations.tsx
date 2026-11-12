import type { Transformation } from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { keys, useTransformations } from '../lib/hooks.ts'
import { EmptyState, ErrorNote, Icon, Markdown, Spinner, confirmAction } from './ui.tsx'

type Draft = Omit<Transformation, 'id'> & { id?: string }

const EMPTY: Draft = { name: '', title: '', description: '', prompt: '', applyDefault: false }

/** Prompts that turn sources into insights, the instructions shared by all of them, and a place to try one. */
export function TransformationsView({ onBack }: { onBack?: () => void }) {
  const list = useTransformations()
  const [selected, setSelected] = useState<string | 'new' | 'instructions'>()
  const current = list.data?.find((t) => t.id === selected)

  useEffect(() => {
    if (!selected && list.data?.[0]) setSelected(list.data[0].id)
  }, [list.data, selected])

  return (
    <div className="split-page">
      <div className="split-list">
        {onBack && (
          <button className="button quiet small back-button" onClick={onBack}>
            <Icon name="back" size={14} /> Notebooks
          </button>
        )}
        <h1 className="page-title">Transformations</h1>
        <p className="page-intro">Prompts that turn a source into an insight, like a summary or a list of questions.</p>
        {list.isPending && <Spinner />}
        <ErrorNote error={list.error} />
        <div className="nav-list">
          {list.data?.map((t) => (
            <button key={t.id} className={`nav-item${selected === t.id ? ' selected' : ''}`} onClick={() => setSelected(t.id)}>
              <span className="nav-item-name">{t.title}</span>
              {t.applyDefault && <span className="nav-item-count">Default</span>}
            </button>
          ))}
          <button className={`nav-item${selected === 'new' ? ' selected' : ''}`} onClick={() => setSelected('new')}>
            <Icon name="plus" />
            <span className="nav-item-name">New transformation</span>
          </button>
          <button className={`nav-item${selected === 'instructions' ? ' selected' : ''}`} onClick={() => setSelected('instructions')}>
            <Icon name="text" />
            <span className="nav-item-name">Shared instructions</span>
          </button>
        </div>
      </div>
      <div className="split-detail">
        {selected === 'instructions' ? (
          <Instructions />
        ) : selected === 'new' ? (
          <Editor key="new" initial={EMPTY} onSaved={(t) => setSelected(t.id)} />
        ) : current ? (
          <Editor key={current.id} initial={current} onSaved={(t) => setSelected(t.id)} onDeleted={() => setSelected(undefined)} />
        ) : (
          !list.isPending && <EmptyState title="Choose a transformation" />
        )}
      </div>
    </div>
  )
}

function Editor({ initial, onSaved, onDeleted }: { initial: Draft; onSaved: (t: Transformation) => void; onDeleted?: () => void }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const [draft, setDraft] = useState<Draft>(initial)
  const set = (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch }))
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial)

  const save = useMutation({
    mutationFn: () => backend.saveTransformation({ ...draft, name: draft.name || draft.title }),
    onSuccess: (t) => {
      void qc.invalidateQueries({ queryKey: keys.transformations })
      onSaved(t)
    },
  })
  const remove = useMutation({
    mutationFn: () => backend.deleteTransformation(initial.id!),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.transformations })
      onDeleted?.()
    },
  })

  return (
    <div className="form">
      <input className="title-input" value={draft.title} onChange={(e) => set({ title: e.target.value })} placeholder="Title, e.g. Key arguments" aria-label="Title" />
      <label className="field">
        <span>What it does</span>
        <input value={draft.description} onChange={(e) => set({ description: e.target.value })} placeholder="Shown as a hint when choosing it" />
      </label>
      <label className="field">
        <span>Prompt</span>
        <textarea className="prompt-input" rows={14} value={draft.prompt} onChange={(e) => set({ prompt: e.target.value })} placeholder="Tell the model what to produce from the source text" />
      </label>
      <label className="check">
        <input type="checkbox" checked={draft.applyDefault} onChange={(e) => set({ applyDefault: e.target.checked })} />
        Run on new sources by default
      </label>
      <ErrorNote error={save.error ?? remove.error} />
      <div className="field-row">
        {initial.id && (
          <button className="button quiet danger" onClick={() => void confirmAction(`Delete “${initial.title}”? Existing insights stay.`).then((ok) => ok && remove.mutate())}>
            <Icon name="trash" size={14} /> Delete
          </button>
        )}
        <span className="spacer" />
        <button className="button primary" onClick={() => save.mutate()} disabled={!draft.title.trim() || !draft.prompt.trim() || !dirty || save.isPending}>
          {save.isPending ? 'Saving' : initial.id ? 'Save changes' : 'Create transformation'}
        </button>
      </div>
      {initial.id && <Playground transformationId={initial.id} />}
    </div>
  )
}

function Playground({ transformationId }: { transformationId: string }) {
  const backend = useBackend()
  const [text, setText] = useState('')
  const run = useMutation({ mutationFn: () => backend.runTransformation(transformationId, text) })
  return (
    <section className="playground">
      <h3>Try it</h3>
      <textarea rows={6} value={text} onChange={(e) => setText(e.target.value)} placeholder="Paste some text to run this transformation on" />
      <div className="field-row">
        <span className="spacer" />
        <button className="button" onClick={() => run.mutate()} disabled={!text.trim() || run.isPending}>
          {run.isPending ? <Spinner /> : <Icon name="play" size={14} />} Run
        </button>
      </div>
      <ErrorNote error={run.error} />
      {run.data && (
        <div className="playground-output">
          <Markdown>{run.data}</Markdown>
        </div>
      )}
    </section>
  )
}

function Instructions() {
  const backend = useBackend()
  const current = useQuery({ queryKey: ['transformation-instructions'], queryFn: () => backend.getTransformationInstructions() })
  const [text, setText] = useState<string>()
  const value = text ?? current.data ?? ''
  const save = useMutation({ mutationFn: () => backend.setTransformationInstructions(value) })
  return (
    <div className="form">
      <h2 className="detail-title">Shared instructions</h2>
      <p className="page-intro">Placed before every transformation prompt. Use it for rules that apply to all of them, like language or tone.</p>
      {current.isPending ? (
        <Spinner />
      ) : (
        <textarea className="prompt-input" rows={18} value={value} onChange={(e) => setText(e.target.value)} aria-label="Shared instructions" />
      )}
      <ErrorNote error={current.error ?? save.error} />
      <div className="field-row">
        <span className="spacer" />
        {save.isSuccess && text === undefined && <span className="status ok">Saved</span>}
        <button
          className="button primary"
          onClick={() => save.mutate(undefined, { onSuccess: () => setText(undefined) })}
          disabled={text === undefined || save.isPending}
        >
          Save instructions
        </button>
      </div>
    </div>
  )
}

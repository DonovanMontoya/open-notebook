import { linkCitations, type AskResult, type AskStep, type SearchHit } from '@open-notebook/engine'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { useNotebooks } from '../lib/hooks.ts'
import { EmptyState, ErrorNote, Icon, Markdown, Spinner } from './ui.tsx'

export interface OpenTarget {
  kind: 'source' | 'note'
  id: string
  notebookId?: string
  query?: string
}

/** Ask a question across notebooks, or search them. */
export function AskView({ onOpen, onBack }: { onOpen: (target: OpenTarget) => void; onBack?: () => void }) {
  const backend = useBackend()
  const notebooks = useNotebooks()
  const [mode, setMode] = useState<'ask' | 'search'>('ask')
  const [scope, setScope] = useState('')
  const [question, setQuestion] = useState('')
  const [submitted, setSubmitted] = useState('')
  const [steps, setSteps] = useState<AskStep[]>([])

  const ask = useMutation({
    mutationFn: (q: string) => {
      setSteps([])
      return backend.ask(q, { notebookId: scope || undefined, onStep: (step) => setSteps((s) => [...s, step]) })
    },
  })
  const search = useQuery({
    queryKey: ['global-search', submitted, scope],
    enabled: mode === 'search' && submitted.length >= 2,
    queryFn: () => backend.search(submitted, scope || undefined),
  })

  const submit = () => {
    const q = question.trim()
    if (!q) return
    setSubmitted(q)
    if (mode === 'ask') ask.mutate(q)
  }

  return (
    <div className="column ask-page">
      {onBack && (
        <button className="button quiet small back-button" onClick={onBack}>
          <Icon name="back" size={14} /> Notebooks
        </button>
      )}
      <h1 className="page-title">{mode === 'ask' ? 'Ask your notebooks' : 'Search your notebooks'}</h1>
      <p className="page-intro">
        {mode === 'ask'
          ? 'A research answer drawn from every source and note, with citations.'
          : 'Find passages and notes by meaning or by word.'}
      </p>
      <form
        className="ask-box"
        onSubmit={(e) => {
          e.preventDefault()
          submit()
        }}
      >
        <textarea
          rows={2}
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          placeholder={mode === 'ask' ? 'What do my sources say about subscription traps?' : 'subscription cancellation'}
          aria-label={mode === 'ask' ? 'Question' : 'Search'}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault()
              submit()
            }
          }}
        />
        <div className="ask-controls">
          <div className="segmented" role="tablist">
            <button type="button" role="tab" aria-selected={mode === 'ask'} className={mode === 'ask' ? 'active' : ''} onClick={() => setMode('ask')}>
              Ask
            </button>
            <button type="button" role="tab" aria-selected={mode === 'search'} className={mode === 'search' ? 'active' : ''} onClick={() => setMode('search')}>
              Search
            </button>
          </div>
          <select value={scope} onChange={(e) => setScope(e.target.value)} aria-label="Where to look">
            <option value="">All notebooks</option>
            {notebooks.data?.map((n) => (
              <option key={n.id} value={n.id}>
                {n.name}
              </option>
            ))}
          </select>
          <span className="spacer" />
          <button className="button primary" type="submit" disabled={!question.trim() || ask.isPending}>
            {mode === 'ask' ? (ask.isPending ? 'Researching' : 'Ask') : 'Search'}
          </button>
        </div>
      </form>

      {mode === 'ask' ? (
        <AskAnswer steps={steps} result={ask.data} pending={ask.isPending} error={ask.error} onOpen={onOpen} />
      ) : (
        <SearchResults hits={search.data} pending={search.isFetching} error={search.error} query={submitted} onOpen={onOpen} />
      )}
    </div>
  )
}

function AskAnswer({
  steps,
  result,
  pending,
  error,
  onOpen,
}: {
  steps: AskStep[]
  result?: AskResult
  pending: boolean
  error: unknown
  onOpen: (target: OpenTarget) => void
}) {
  const strategy = steps.find((s) => s.kind === 'strategy')
  const answered = new Set(steps.filter((s) => s.kind === 'answer').map((s) => (s as { term: string }).term))
  const final = result?.answer ?? (steps.find((s) => s.kind === 'final') as { answer: string } | undefined)?.answer
  const documents = result?.documents ?? []
  const linked = final ? linkCitations(final, documents) : undefined

  if (!pending && !final && !error) return null
  return (
    <section className="ask-result">
      {strategy && strategy.kind === 'strategy' && (
        <div className="ask-plan">
          {strategy.reasoning && <p className="ask-reasoning">{strategy.reasoning}</p>}
          <ul className="ask-searches">
            {strategy.searches.map((s) => (
              <li key={s.term} className={answered.has(s.term) || final ? 'done' : ''}>
                {answered.has(s.term) || final ? <Icon name="check" size={14} /> : <Spinner />}
                {s.term}
              </li>
            ))}
          </ul>
        </div>
      )}
      {pending && !final && <p className="writing">{strategy ? 'Reading what the searches found' : 'Planning the research'}</p>}
      <ErrorNote error={error} />
      {final && linked && (
        <div className="answer">
          <Markdown
            cite={{
              onCite: (index) => {
                const doc = linked.cited[index] as { id: string; notebookId?: string } | undefined
                if (doc) onOpen({ kind: doc.id.startsWith('note:') ? 'note' : 'source', id: doc.id, notebookId: doc.notebookId })
              },
            }}
          >
            {linked.markdown}
          </Markdown>
        </div>
      )}
      {final && documents.length > 0 && (
        <div className="ask-documents">
          <h3>Documents it drew on</h3>
          {documents.map((d) => (
            <button key={d.id} className="search-hit" onClick={() => onOpen({ kind: d.id.startsWith('note:') ? 'note' : 'source', id: d.id, notebookId: d.notebookId })}>
              <span className="search-hit-title">
                <Icon name={d.id.startsWith('note:') ? 'note' : 'file'} size={14} />
                {d.title}
              </span>
            </button>
          ))}
        </div>
      )}
    </section>
  )
}

function SearchResults({
  hits,
  pending,
  error,
  query,
  onOpen,
}: {
  hits?: SearchHit[]
  pending: boolean
  error: unknown
  query: string
  onOpen: (target: OpenTarget) => void
}) {
  if (!query) return null
  return (
    <section className="ask-result">
      {pending && <Spinner />}
      <ErrorNote error={error} />
      {hits?.length === 0 && <EmptyState title={`Nothing matches “${query}”`} />}
      {hits?.map((hit) => (
        <button key={`${hit.kind}:${hit.id}`} className="search-hit wide" onClick={() => onOpen({ kind: hit.kind, id: hit.id, notebookId: hit.notebookId, query })}>
          <span className="search-hit-title">
            <Icon name={hit.kind === 'note' ? 'note' : 'file'} size={14} />
            {hit.title}
          </span>
          {hit.snippet && <span className="search-hit-snippet">{hit.snippet}</span>}
        </button>
      ))}
    </section>
  )
}

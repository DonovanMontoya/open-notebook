import type { SourceSummary } from '@open-notebook/api-client'
import type { StoredDeck, StudyDeck, StudyKind } from '@open-notebook/engine'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { relativeTime } from '../lib/format.ts'
import { keys } from '../lib/hooks.ts'
import { EmptyState, ErrorNote, Icon, Spinner } from './ui.tsx'

const decksKey = (notebookId: string) => ['decks', notebookId] as const

/** Flashcards and quizzes made from this notebook's sources. */
export function Study({ notebookId, sources, onOpenSource }: { notebookId: string; sources: SourceSummary[]; onOpenSource: (id: string) => void }) {
  const backend = useBackend()
  const decks = useQuery({ queryKey: decksKey(notebookId), queryFn: () => backend.listStudyDecks(notebookId) })
  const [open, setOpen] = useState<StudyDeck>()

  if (open) {
    return (
      <div className="column">
        <button className="button quiet small" onClick={() => setOpen(undefined)} style={{ marginBottom: 16 }}>
          <Icon name="back" size={14} /> All decks
        </button>
        {open.kind === 'flashcards' ? (
          <Flashcards deck={open} sources={sources} onOpenSource={onOpenSource} />
        ) : (
          <Quiz deck={open} sources={sources} onOpenSource={onOpenSource} />
        )}
      </div>
    )
  }

  return (
    <div className="column">
      <Generator notebookId={notebookId} sources={sources} onMade={setOpen} />
      <h3 className="listen-heading">Decks</h3>
      {decks.isPending && <Spinner />}
      <ErrorNote error={decks.error} />
      {decks.data?.length === 0 && <EmptyState title="No decks yet">Make flashcards to remember the key points, or a quiz to check yourself.</EmptyState>}
      <ul className="episodes">
        {decks.data?.map((d: StoredDeck) => (
          <li key={d.noteId} className="episode">
            <span className="deck-icon" aria-hidden>
              {d.kind === 'quiz' ? '?' : d.items.length}
            </span>
            <div className="episode-main">
              <button className="episode-title" onClick={() => setOpen(d)}>
                {d.title}
              </button>
              <p className="episode-meta">
                {d.kind === 'quiz' ? 'Quiz' : 'Flashcards'}, {d.items.length} {d.kind === 'quiz' ? 'questions' : 'cards'}
                {d.created ? `, ${relativeTime(d.created)}` : ''}
              </p>
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}

function Generator({ notebookId, sources, onMade }: { notebookId: string; sources: SourceSummary[]; onMade: (deck: StudyDeck) => void }) {
  const backend = useBackend()
  const qc = useQueryClient()
  const ready = sources.filter((s) => s.status === 'completed' || !s.status)
  const [kind, setKind] = useState<StudyKind>('flashcards')
  const [count, setCount] = useState(8)
  const [difficulty, setDifficulty] = useState<'easy' | 'medium' | 'hard'>('medium')
  const [focus, setFocus] = useState('')
  const [chosen, setChosen] = useState<Set<string>>()
  const selected = chosen ?? new Set(ready.map((s) => s.id))

  const make = useMutation({
    mutationFn: () =>
      backend.generateStudyDeck(notebookId, {
        kind,
        sourceIds: [...selected],
        count,
        difficulty,
        instructions: focus,
        language: navigator.language.split('-')[0] || 'en',
      }),
    onSuccess: (deck) => {
      void qc.invalidateQueries({ queryKey: decksKey(notebookId) })
      void qc.invalidateQueries({ queryKey: keys.notes(notebookId) })
      onMade(deck)
    },
  })

  return (
    <div className="composer-card">
      <div className="composer-row">
        <div className="segmented" role="radiogroup" aria-label="Kind">
          {(['flashcards', 'quiz'] as StudyKind[]).map((k) => (
            <button key={k} role="radio" aria-checked={kind === k} className={kind === k ? 'active' : ''} onClick={() => setKind(k)}>
              {k === 'flashcards' ? 'Flashcards' : 'Quiz'}
            </button>
          ))}
        </div>
        <div className="segmented" role="radiogroup" aria-label="Difficulty">
          {(['easy', 'medium', 'hard'] as const).map((d) => (
            <button key={d} role="radio" aria-checked={difficulty === d} className={difficulty === d ? 'active' : ''} onClick={() => setDifficulty(d)}>
              {d[0].toUpperCase() + d.slice(1)}
            </button>
          ))}
        </div>
        <label className="inline-select study-count">
          <input type="range" min={4} max={20} value={count} onChange={(e) => setCount(Number(e.target.value))} aria-label={kind === 'flashcards' ? 'Number of cards' : 'Number of questions'} />
          <span className="tabular">
            {count} {kind === 'flashcards' ? 'cards' : 'questions'}
          </span>
        </label>
      </div>
      {ready.length > 1 && (
        <div className="checks">
          {ready.map((s) => (
            <label key={s.id} className="check">
              <input
                type="checkbox"
                checked={selected.has(s.id)}
                onChange={(e) => {
                  const next = new Set(selected)
                  if (e.target.checked) next.add(s.id)
                  else next.delete(s.id)
                  setChosen(next)
                }}
              />
              {s.title || 'Untitled source'}
            </label>
          ))}
        </div>
      )}
      <textarea className="focus-input" rows={2} value={focus} onChange={(e) => setFocus(e.target.value)} placeholder="Anything to focus on? Optional" aria-label="Focus" />
      <div className="composer-row">
        <span className="voice-line">{ready.length ? `From ${selected.size} of ${ready.length} sources` : 'Add a source first'}</span>
        <span className="spacer" />
        <button className="button primary" onClick={() => make.mutate()} disabled={!selected.size || make.isPending}>
          {make.isPending ? 'Making the deck' : kind === 'quiz' ? 'Make quiz' : 'Make flashcards'}
        </button>
      </div>
      <ErrorNote error={make.error} />
    </div>
  )
}

function SourceLinks({ ids, sources, onOpenSource }: { ids: string[]; sources: SourceSummary[]; onOpenSource: (id: string) => void }) {
  return (
    <span className="study-sources">
      {ids.map((id) => (
        <button key={id} className="cite" onClick={() => onOpenSource(id)}>
          {sources.find((s) => s.id === id)?.title ?? 'Source'}
        </button>
      ))}
    </span>
  )
}

/** One card at a time: space flips, arrows move, and cards you miss come back at the end. */
function Flashcards({ deck, sources, onOpenSource }: { deck: StudyDeck; sources: SourceSummary[]; onOpenSource: (id: string) => void }) {
  const [order, setOrder] = useState(() => deck.items.map((_, i) => i))
  const [at, setAt] = useState(0)
  const [flipped, setFlipped] = useState(false)
  const [known, setKnown] = useState(0)
  const done = at >= order.length
  const card = deck.items[order[at]]

  const next = (knewIt: boolean) => {
    if (knewIt) setKnown((k) => k + 1)
    else setOrder((o) => [...o, o[at]])
    setFlipped(false)
    setAt((a) => a + 1)
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement).closest('input, textarea')) return
      if (e.key === ' ') {
        e.preventDefault()
        setFlipped((f) => !f)
      } else if (flipped && e.key === 'ArrowRight') next(true)
      else if (flipped && e.key === 'ArrowLeft') next(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  return (
    <div className="study">
      <h2 className="detail-title">{deck.title}</h2>
      <p className="episode-meta">
        {done ? `${deck.items.length} cards` : `Card ${Math.min(at + 1, order.length)} of ${order.length}`}, {known} known
      </p>
      {done ? (
        <div className="study-done">
          <p className="empty-title">All {deck.items.length} cards known</p>
          <button
            className="button"
            onClick={() => {
              setOrder(deck.items.map((_, i) => i))
              setAt(0)
              setKnown(0)
            }}
          >
            Go again
          </button>
        </div>
      ) : (
        <>
          <button className={`flashcard${flipped ? ' flipped' : ''}`} onClick={() => setFlipped((f) => !f)} aria-label={flipped ? 'Show question' : 'Show answer'}>
            <span className="flashcard-face front">{card.question}</span>
            <span className="flashcard-face back">{card.answer}</span>
          </button>
          <div className="study-controls">
            {flipped ? (
              <>
                <button className="button" onClick={() => next(false)}>
                  Again <kbd>←</kbd>
                </button>
                <button className="button primary" onClick={() => next(true)}>
                  Got it <kbd>→</kbd>
                </button>
              </>
            ) : (
              <button className="button" onClick={() => setFlipped(true)}>
                Show answer <kbd>Space</kbd>
              </button>
            )}
          </div>
          {flipped && <SourceLinks ids={card.source_ids} sources={sources} onOpenSource={onOpenSource} />}
        </>
      )}
    </div>
  )
}

function Quiz({ deck, sources, onOpenSource }: { deck: StudyDeck; sources: SourceSummary[]; onOpenSource: (id: string) => void }) {
  const [at, setAt] = useState(0)
  const [picked, setPicked] = useState<number>()
  const [score, setScore] = useState(0)
  const done = at >= deck.items.length
  const item = deck.items[at]

  return (
    <div className="study">
      <h2 className="detail-title">{deck.title}</h2>
      <p className="episode-meta">
        {done ? 'Finished' : `Question ${at + 1} of ${deck.items.length}`}, {score} right
      </p>
      {done ? (
        <div className="study-done">
          <p className="empty-title">
            {score} of {deck.items.length} right
          </p>
          <button
            className="button"
            onClick={() => {
              setAt(0)
              setScore(0)
              setPicked(undefined)
            }}
          >
            Try again
          </button>
        </div>
      ) : (
        <>
          <p className="quiz-question">{item.question}</p>
          <ol className="quiz-options">
            {item.options.map((option, i) => {
              const state = picked === undefined ? '' : i === item.correct_index ? 'right' : i === picked ? 'wrong' : 'dim'
              return (
                <li key={i}>
                  <button
                    className={`quiz-option ${state}`}
                    disabled={picked !== undefined}
                    onClick={() => {
                      setPicked(i)
                      if (i === item.correct_index) setScore((s) => s + 1)
                    }}
                  >
                    <span className="quiz-letter">{'ABCD'[i]}</span>
                    {option}
                  </button>
                </li>
              )
            })}
          </ol>
          {picked !== undefined && (
            <div className="quiz-explain">
              <p>{item.answer}</p>
              <SourceLinks ids={item.source_ids} sources={sources} onOpenSource={onOpenSource} />
              <div className="study-controls">
                <button
                  className="button primary"
                  onClick={() => {
                    setPicked(undefined)
                    setAt((a) => a + 1)
                  }}
                >
                  {at + 1 < deck.items.length ? 'Next question' : 'See score'}
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  )
}

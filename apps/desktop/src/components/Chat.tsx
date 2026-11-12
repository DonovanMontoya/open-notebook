import type { ChatMessage, ChatSession, SourceSummary } from '@open-notebook/api-client'
import { finishedBlocks, linkCitations, sentenceAround, type ContextSelection } from '@open-notebook/engine'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { errorMessage, relativeTime } from '../lib/format.ts'
import { keys, useStoredState } from '../lib/hooks.ts'
import { useSettings } from '../lib/settings.ts'
import { EmptyState, ErrorNote, Icon, Markdown, Menu, Spinner, confirmAction, usePopover } from './ui.tsx'
import { createProvider } from '@open-notebook/engine'
import { platformFetch } from '../platform/index.ts'
import { providerConfig } from '../lib/settings.ts'

export interface CiteTarget {
  sourceId: string
  query: string
  key: string
}

/** What a chat is about: the whole notebook, or one source in it. */
export type ChatScope = { kind: 'notebook'; notebookId: string; context: ContextSelection } | { kind: 'source'; sourceId: string }

interface Props {
  scope: ChatScope
  sources: SourceSummary[]
  activeCite?: string
  onCite: (target: CiteTarget) => void
  onSaveNote: (content: string) => void
  onAddSource: () => void
}

/** undefined: follow the latest session. null: a new, unsaved chat. */
type Selected = string | null | undefined

export function Chat({ scope, sources, activeCite, onCite, onSaveNote, onAddSource }: Props) {
  const backend = useBackend()
  const settings = useSettings()
  const qc = useQueryClient()
  const sessionsKey = scope.kind === 'notebook' ? keys.sessions(scope.notebookId) : ['source-sessions', scope.sourceId]
  const sessions = useQuery({
    queryKey: sessionsKey,
    queryFn: () => (scope.kind === 'notebook' ? backend.listChatSessions(scope.notebookId) : backend.listSourceChatSessions(scope.sourceId)),
  })
  const [selected, setSelected] = useState<Selected>(undefined)
  const activeId = selected === undefined ? sessions.data?.[0]?.id : (selected ?? undefined)
  const session = useQuery({
    queryKey: keys.session(activeId ?? ''),
    enabled: !!activeId,
    queryFn: () =>
      scope.kind === 'notebook' ? backend.getChatSession(activeId!) : backend.getSourceChatSession(scope.sourceId, activeId!),
  })
  const [renaming, setRenaming] = useState<string>()
  // A model chosen for this conversation, remembered per session (or for new chats in this scope).
  const scopeId = scope.kind === 'notebook' ? scope.notebookId : scope.sourceId
  const [overrides, setOverrides] = useStoredState<Record<string, string>>('open-notebook-model-overrides', {})
  const overrideKey = activeId ?? `new:${scopeId}`
  const modelOverride = overrides[overrideKey]

  const [draft, setDraft] = useState('')
  const [pending, setPending] = useState<{ question: string; text: string }>()
  const [error, setError] = useState<string>()
  const abort = useRef<AbortController>(undefined)
  const scroller = useRef<HTMLDivElement>(null)
  const input = useRef<HTMLTextAreaElement>(null)

  const messages = activeId ? (session.data?.messages ?? []) : []
  const turns = useMemo(() => pairTurns(messages), [messages])

  // Follow new text only while the reader is at the bottom; scrolling up while an answer streams stops following.
  const following = useRef(true)
  useEffect(() => {
    const el = scroller.current
    if (!el) return
    const onScroll = () => {
      following.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48
    }
    el.addEventListener('scroll', onScroll, { passive: true })
    return () => el.removeEventListener('scroll', onScroll)
  }, [])
  useEffect(() => {
    following.current = true
  }, [turns.length, activeId, pending?.question])
  useEffect(() => {
    const el = scroller.current
    if (el && following.current) el.scrollTop = el.scrollHeight
  }, [turns.length, pending?.text, pending?.question, activeId])

  useEffect(() => {
    const el = input.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`
  }, [draft])

  const send = async () => {
    const question = draft.trim()
    if (!question || pending) return
    setError(undefined)
    setDraft('')
    setPending({ question, text: '' })
    const controller = new AbortController()
    abort.current = controller
    try {
      const common = {
        sessionId: activeId,
        message: question,
        modelOverride,
        signal: controller.signal,
        onText: (delta: string) => setPending((p) => p && { ...p, text: p.text + delta }),
      }
      const result =
        scope.kind === 'notebook'
          ? await backend.sendChatMessage({ ...common, notebookId: scope.notebookId, context: scope.context })
          : await backend.sendSourceChatMessage({ ...common, sourceId: scope.sourceId })
      qc.setQueryData(keys.session(result.session.id), result.session)
      if (!activeId && modelOverride) setOverrides({ ...overrides, [result.session.id]: modelOverride, [overrideKey]: '' })
      setSelected(result.session.id)
      void qc.invalidateQueries({ queryKey: sessionsKey })
    } catch (e) {
      if (!controller.signal.aborted) setError(errorMessage(e))
      setDraft((d) => d || question)
    } finally {
      setPending(undefined)
      abort.current = undefined
    }
  }

  const modelLabel =
    backend.kind === 'remote' ? 'Server default model' : settings.chat?.model ? settings.chat.model : 'No chat model chosen'
  const ready = scope.kind === 'source' || sources.some((s) => s.status === 'completed' || !s.status)
  const activeSession = sessions.data?.find((x) => x.id === activeId)

  const rename = async (title: string) => {
    if (!activeId) return
    await backend.renameChatSession(activeId, title)
    setRenaming(undefined)
    void qc.invalidateQueries({ queryKey: sessionsKey })
  }
  const remove = async () => {
    if (!activeId || !(await confirmAction('Delete this conversation?'))) return
    if (scope.kind === 'notebook') await backend.deleteChatSession(activeId)
    else await backend.deleteSourceChatSession(scope.sourceId, activeId)
    setSelected(undefined)
    void qc.invalidateQueries({ queryKey: sessionsKey })
  }

  return (
    <div className="chat">
      {(sessions.data?.length ?? 0) > 0 && (
        <div className="chat-bar">
          {renaming !== undefined ? (
            <form
              className="rename"
              onSubmit={(e) => {
                e.preventDefault()
                void rename(renaming)
              }}
            >
              <input autoFocus value={renaming} onChange={(e) => setRenaming(e.target.value)} aria-label="Conversation name" onKeyDown={(e) => e.key === 'Escape' && setRenaming(undefined)} />
              <button className="button small" type="submit">
                Save
              </button>
              <button className="button quiet small" type="button" onClick={() => setRenaming(undefined)}>
                Cancel
              </button>
            </form>
          ) : (
            <Conversations sessions={sessions.data ?? []} activeId={activeId} onSelect={setSelected} />
          )}
          {activeId && renaming === undefined && (
            <>
              <button className="icon-button" onClick={() => setRenaming(activeSession?.title ?? '')} title="Rename conversation" aria-label="Rename conversation">
                <Icon name="note" />
              </button>
              <button className="icon-button" onClick={() => void remove()} title="Delete conversation" aria-label="Delete conversation">
                <Icon name="trash" />
              </button>
            </>
          )}
          <button className="button small new-chat" onClick={() => setSelected(null)} disabled={!activeId}>
            <Icon name="plus" size={14} /> New chat
          </button>
        </div>
      )}
      <div className="chat-scroll" ref={scroller}>
        <div className="column">
          {activeId && session.isPending && <Spinner />}
          {!activeId && !pending && (
            <EmptyState title={scope.kind === 'source' ? 'Ask about this source' : ready ? 'Ask about your sources' : 'Add a source to ask about'}>
              {scope.kind === 'source' ? (
                'Questions here are answered from this source alone.'
              ) : ready ? (
                'Answers cite the source they come from. Click a citation to read the passage.'
              ) : (
                <>
                  Answers come from the pages, PDFs and text you add to this notebook.
                  <div>
                    <button className="button" onClick={onAddSource}>
                      <Icon name="plus" size={14} /> Add a source
                    </button>
                  </div>
                </>
              )}
            </EmptyState>
          )}

          {turns.map((t, i) => (
            <Turn
              key={t.question?.id ?? t.answer?.id ?? i}
              question={t.question?.content}
              answer={t.answer?.content}
              turnKey={t.answer?.id ?? String(i)}
              sources={sources}
              activeCite={activeCite}
              onCite={onCite}
              onSaveNote={onSaveNote}
            />
          ))}
          {pending && (
            <Turn
              question={pending.question}
              answer={pending.text}
              streaming
              turnKey="pending"
              sources={sources}
              activeCite={activeCite}
              onCite={onCite}
              onSaveNote={onSaveNote}
            />
          )}
          {error && (
            <div style={{ marginTop: 24 }}>
              <ErrorNote error={error} />
            </div>
          )}
        </div>
      </div>

      <div className="chat-composer">
        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault()
            void send()
          }}
        >
          <textarea
            ref={input}
            rows={1}
            value={draft}
            placeholder={scope.kind === 'source' ? 'Ask about this source' : 'Ask about these sources'}
            aria-label="Question"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault()
                void send()
              }
            }}
          />
          {pending && backend.streamsChat ? (
            <button type="button" className="button" onClick={() => abort.current?.abort()} aria-label="Stop" title="Stop">
              <Icon name="stop" size={14} />
            </button>
          ) : (
            <button type="submit" className="button primary" disabled={!draft.trim() || !!pending} aria-label="Ask" title="Ask">
              {pending ? <Spinner /> : <Icon name="send" size={14} />}
            </button>
          )}
        </form>
        <div className="composer-meta">
          <ModelMenu value={modelOverride} defaultLabel={modelLabel} onChange={(v) => setOverrides({ ...overrides, [overrideKey]: v })} />
          <span className="composer-hint">Shift Return for a new line</span>
        </div>
      </div>
    </div>
  )
}

function Turn(props: {
  question?: string
  answer?: string
  streaming?: boolean
  turnKey: string
  sources: SourceSummary[]
  activeCite?: string
  onCite: (target: CiteTarget) => void
  onSaveNote: (content: string) => void
}) {
  const { question, answer: full = '', streaming, turnKey, sources, activeCite, onCite, onSaveNote } = props
  // While streaming, show whole finished blocks only; the block in progress stays hidden.
  const answer = streaming ? finishedBlocks(full) : full
  const linked = useMemo(() => linkCitations(answer, sources), [answer, sources])
  const activeIndex = activeCite?.startsWith(`${turnKey}#`) ? Number(activeCite.split('#')[1]) : undefined

  return (
    <article className="turn">
      {question && <p className="question">{question}</p>}
      <div className="answer">
        {answer && (
          <Markdown
            cite={{
              activeIndex,
              onCite: (index, label) => {
                const source = linked.cited[index]
                if (!source) return
                // Server answers cite by record id; the tab shows the title instead.
                const byLabel = answer.indexOf(`[${label}`)
                const at = byLabel >= 0 ? byLabel : answer.indexOf(source.id)
                onCite({ sourceId: source.id, query: sentenceAround(answer, Math.max(at, 0)), key: `${turnKey}#${index}` })
              },
            }}
          >
            {linked.markdown}
          </Markdown>
        )}
        {streaming && <p className="writing">{full ? 'Writing' : 'Reading your sources'}</p>}
      </div>
      {answer && !streaming && (
        <div className="answer-actions">
          <button className="button quiet small" onClick={() => onSaveNote(answer)}>
            <Icon name="note" size={14} /> Save as note
          </button>
          <button className="button quiet small" onClick={() => void navigator.clipboard.writeText(answer)}>
            Copy
          </button>
        </div>
      )}
    </article>
  )
}

function pairTurns(messages: ChatMessage[]): Array<{ question?: ChatMessage; answer?: ChatMessage }> {
  const turns: Array<{ question?: ChatMessage; answer?: ChatMessage }> = []
  for (const m of messages) {
    if (m.type === 'human') turns.push({ question: m })
    else if (turns.length && !turns[turns.length - 1].answer) turns[turns.length - 1].answer = m
    else turns.push({ answer: m })
  }
  return turns
}

/** Pick a model for this conversation: the server's language models, or the chat provider's models on device. */
function ModelMenu({ value, defaultLabel, onChange }: { value?: string; defaultLabel: string; onChange: (value: string) => void }) {
  const backend = useBackend()
  const settings = useSettings()
  const kind = settings.chat?.provider
  const options = useQuery({
    queryKey: ['chat-model-options', backend.kind, kind],
    staleTime: 300_000,
    retry: false,
    queryFn: async () => {
      if (backend.kind === 'remote') {
        return (await backend.listModels()).filter((m) => m.type === 'language').map((m) => ({ value: m.id, label: `${m.name} (${m.provider})` }))
      }
      if (!kind) return []
      const models = await createProvider(await providerConfig(kind), await platformFetch()).listModels()
      return models.filter((m) => !m.capabilities || m.capabilities.includes('chat')).map((m) => ({ value: `${kind}::${m.id}`, label: m.label ?? m.id }))
    },
  })
  const list = [{ value: '', label: `Default: ${defaultLabel}` }, ...(options.data ?? [])]
  const current = list.find((o) => o.value === (value ?? ''))
  return (
    <span className="model-menu">
      <Menu title="Model for this conversation" label={current && value ? current.label : defaultLabel} value={value ?? ''} options={list} onChange={onChange} />
    </span>
  )
}

/** The notebook's conversations, newest first, in a dropdown that names the current one. */
function Conversations({ sessions, activeId, onSelect }: { sessions: ChatSession[]; activeId?: string; onSelect: (id: string | null) => void }) {
  const { open, setOpen, ref } = usePopover()
  const current = sessions.find((s) => s.id === activeId)
  const pick = (id: string | null) => {
    onSelect(id)
    setOpen(false)
  }
  return (
    <div className="conversations" ref={ref}>
      <button className="conversation-current" onClick={() => setOpen((o) => !o)} aria-haspopup="listbox" aria-expanded={open}>
        <span className="conversation-name">{current ? current.title || 'Untitled conversation' : 'New chat'}</span>
        <span className="conversation-count">{sessions.length}</span>
        <Icon name="chevron-down" size={14} />
      </button>
      {open && (
        <ul className="conversation-list" role="listbox" aria-label="Conversations">
          {sessions.map((s) => (
            <li key={s.id}>
              <button role="option" aria-selected={s.id === activeId} className={s.id === activeId ? 'selected' : ''} onClick={() => pick(s.id)}>
                <span className="conversation-name">{s.title || 'Untitled conversation'}</span>
                <span className="conversation-when">{relativeTime(s.updated || s.created)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

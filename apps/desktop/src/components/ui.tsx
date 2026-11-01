import { useEffect, useRef, useState, type ReactNode } from 'react'
import ReactMarkdown, { defaultUrlTransform } from 'react-markdown'
import remarkGfm from 'remark-gfm'

import { isTauri } from '../platform/index.ts'

export async function openExternal(url: string) {
  if (isTauri) {
    const { openUrl } = await import('@tauri-apps/plugin-opener')
    await openUrl(url)
  } else {
    window.open(url, '_blank', 'noopener')
  }
}

export interface CiteHandler {
  /** Called with the citation's index in the answer and its label. */
  onCite: (index: number, label: string) => void
  activeIndex?: number
}

export function Markdown({ children, cite }: { children: string; cite?: CiteHandler }) {
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        urlTransform={(url) => (url.startsWith('cite:') ? url : defaultUrlTransform(url))}
        components={{
          a: ({ href, children: label }) => {
            if (href?.startsWith('cite:') && cite) {
              const index = Number(href.slice(5))
              const text = String(Array.isArray(label) ? label.join('') : (label ?? ''))
              return (
                <button
                  type="button"
                  className={`cite${cite.activeIndex === index ? ' active' : ''}`}
                  title={`Open ${text}`}
                  onClick={() => cite.onCite(index, text)}
                >
                  {text}
                </button>
              )
            }
            return (
              <a
                href={href}
                onClick={(e) => {
                  e.preventDefault()
                  if (href && /^https?:/i.test(href)) void openExternal(href)
                }}
              >
                {label}
              </a>
            )
          },
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
}

export function Modal({
  title,
  onClose,
  children,
  footer,
  wide,
}: {
  title: ReactNode
  onClose: () => void
  children: ReactNode
  footer?: ReactNode
  wide?: boolean
}) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const dialog = ref.current
    if (dialog && !dialog.open) dialog.showModal()
  }, [])
  return (
    <dialog
      ref={ref}
      className={`modal${wide ? ' modal-wide' : ''}`}
      onCancel={(e) => {
        e.preventDefault()
        onClose()
      }}
      onClick={(e) => {
        if (e.target === ref.current) onClose()
      }}
    >
      <header className="modal-header">
        <h2>{title}</h2>
        <button className="icon-button" onClick={onClose} aria-label="Close">
          <Icon name="close" />
        </button>
      </header>
      <div className="modal-body">{children}</div>
      {footer && <footer className="modal-footer">{footer}</footer>}
    </dialog>
  )
}

export function Spinner({ label }: { label?: string }) {
  return <span className="spinner" role="status" aria-label={label ?? 'Loading'} />
}

export function EmptyState({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <p className="empty-title">{title}</p>
      {children && <div className="empty-body">{children}</div>}
    </div>
  )
}

export function ErrorNote({ error }: { error: unknown }) {
  if (!error) return null
  return <p className="error-note">{error instanceof Error ? error.message : String(error)}</p>
}

const ICONS: Record<string, string> = {
  plus: 'M12 5v14M5 12h14',
  check: 'M20 6L9 17l-5-5',
  play: 'M7 4.5v15a.5.5 0 0 0 .76.43l12.5-7.5a.5.5 0 0 0 0-.86L7.76 4.07A.5.5 0 0 0 7 4.5z',
  pause: 'M8 5v14M16 5v14',
  'chapter-back': 'M6 5v14M19 5L9 12l10 7z',
  'chapter-next': 'M18 5v14M5 5l10 7-10 7z',
  'chevron-down': 'M6 9l6 6 6-6',
  moon: 'M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z',
  volume: 'M11 5L6 9H3v6h3l5 4zM15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13',
  headphones: 'M3 18v-6a9 9 0 0 1 18 0v6M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z',
  layers: 'M12 2L2 7l10 5 10-5zM2 17l10 5 10-5M2 12l10 5 10-5',
  chart: 'M3 3v18h18M18 17V9M13 17V5M8 17v-3',
  list: 'M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01',
  close: 'M6 6l12 12M18 6L6 18',
  settings:
    'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z',
  book: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5v14zM4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5',
  link: 'M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71',
  file: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6',
  text: 'M4 6h16M4 12h16M4 18h10',
  note: 'M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z',
  send: 'M22 2L11 13M22 2l-7 20-4-9-9-4z',
  stop: 'M6 6h12v12H6z',
  search: 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.35-4.35',
  trash: 'M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6',
  retry: 'M1 4v6h6M3.51 15a9 9 0 1 0 2.13-9.36L1 10',
  back: 'M15 18l-6-6 6-6',
  server: 'M2 4h20v6H2zM2 14h20v6H2zM6 7h.01M6 17h.01',
  device: 'M4 4h16v12H4zM2 20h20',
  chat: 'M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z',
  external: 'M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14L21 3',
}

/** Circular skip arrow with the number of seconds inside, like a podcast app. */
export function SkipIcon({ seconds, forward, size = 28 }: { seconds: number; forward?: boolean; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 28 28" fill="none" aria-hidden>
      <g transform={forward ? undefined : 'translate(28 0) scale(-1 1)'} stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
        <path d="M22.5 14A8.5 8.5 0 1 1 14 5.5h3" />
        <path d="M15 2.5l3 3-3 3" />
      </g>
      <text x="14" y="17.6" textAnchor="middle" fontSize="8.5" fontWeight="600" fill="currentColor" fontFamily="var(--ui)">
        {seconds}
      </text>
    </svg>
  )
}

/** A button that opens a small list of choices. */
export function Menu<T extends string | number>({
  label,
  value,
  options,
  onChange,
  icon,
  title,
}: {
  label: ReactNode
  value: T
  options: Array<{ value: T; label: string }>
  onChange: (value: T) => void
  icon?: string
  title: string
}) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false)
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [open])
  return (
    <div className="menu" ref={ref}>
      <button className="button quiet small" onClick={() => setOpen((o) => !o)} aria-haspopup="listbox" aria-expanded={open} title={title}>
        {icon && <Icon name={icon} size={14} />}
        {label}
      </button>
      {open && (
        <ul className="menu-list" role="listbox" aria-label={title}>
          {options.map((o) => (
            <li key={String(o.value)}>
              <button
                role="option"
                aria-selected={o.value === value}
                className={o.value === value ? 'selected' : ''}
                onClick={() => {
                  onChange(o.value)
                  setOpen(false)
                }}
              >
                {o.label}
                {o.value === value && <Icon name="check" size={14} />}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

export function Icon({ name, size = 16 }: { name: keyof typeof ICONS | string; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d={ICONS[name] ?? ''} />
    </svg>
  )
}

type ConfirmVerb = 'Delete' | 'Remove' | 'Discard' | 'Rebuild'
type Ask = { message: string; action: ConfirmVerb; resolve: (ok: boolean) => void }
let showAsk: ((ask: Ask) => void) | undefined

/**
 * Asks before an action, with `action` as the confirm button's label. The Mac
 * web view ignores window.confirm, so this shows the app's own dialog instead.
 */
export function confirmAction(message: string, action: ConfirmVerb = 'Delete'): Promise<boolean> {
  if (!showAsk) return Promise.resolve(window.confirm(message))
  return new Promise((resolve) => showAsk!({ message, action, resolve }))
}

/** Mount once near the root; renders the dialog confirmAction asks for. */
export function ConfirmHost() {
  const [ask, setAsk] = useState<Ask>()
  useEffect(() => {
    showAsk = (next) => setAsk(next)
    return () => {
      showAsk = undefined
    }
  }, [])
  if (!ask) return null
  const close = (ok: boolean) => {
    ask.resolve(ok)
    setAsk(undefined)
  }
  const [title, ...rest] = ask.message.split(/(?<=\?)\s/)
  return (
    <Modal
      title={title}
      onClose={() => close(false)}
      footer={
        <>
          <button className="button" onClick={() => close(false)}>
            Cancel
          </button>
          <button className={`button primary${ask.action === 'Rebuild' ? '' : ' danger-fill'}`} autoFocus onClick={() => close(true)}>
            {ask.action}
          </button>
        </>
      }
    >
      {rest.length > 0 && <p className="confirm-detail">{rest.join(' ')}</p>}
    </Modal>
  )
}

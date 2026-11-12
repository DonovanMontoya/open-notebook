// Moving the window. The title bar is hidden (titleBarStyle "Overlay"), so the app decides
// where a press starts a window drag: the top strip of every screen, and any element marked
// data-drag (headers, the sidebar brand), including the text inside them. Controls never drag.
import { isTauri } from './index.ts'

/** The height of the hidden title bar, where the traffic lights sit. */
const TITLE_BAR = 32

const CONTROLS =
  'button, a, input, textarea, select, label, summary, audio, video, [role="button"], [role="tab"], [role="option"], [role="slider"], [role="listbox"], [contenteditable="true"], [contenteditable=""], [data-no-drag], .modal, .menu-list, .conversation-list, .search-results'

export function enableWindowDrag(): void {
  if (!isTauri) return
  const win = import('@tauri-apps/api/window').then((m) => m.getCurrentWindow())
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey) return
    const target = e.target
    if (!(target instanceof Element) || target.closest(CONTROLS)) return
    if (!(e.clientY <= TITLE_BAR || target.closest('[data-drag]'))) return
    e.preventDefault()
    // A double click zooms the window, like a normal title bar, unless the element has its own double-click action.
    if (e.detail === 2) {
      if (!target.closest('[data-own-dblclick]')) void win.then((w) => w.toggleMaximize())
      return
    }
    void win.then((w) => w.startDragging())
  })
}

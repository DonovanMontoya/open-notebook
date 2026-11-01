import { useState } from 'react'

import { AskView, type OpenTarget } from './components/Ask.tsx'
import { LibraryView } from './components/Library.tsx'
import { NotebookView } from './components/Notebook.tsx'
import { MiniPlayer, NowPlaying } from './components/Player.tsx'
import { SettingsView } from './components/Settings.tsx'
import { Sidebar } from './components/Sidebar.tsx'
import { TransformationsView } from './components/Transformations.tsx'
import { UsageView } from './components/Usage.tsx'
import { ConfirmHost, EmptyState, ErrorNote, Spinner } from './components/ui.tsx'
import { useBackend, useBackendState } from './lib/backend.tsx'
import { useNotebooks } from './lib/hooks.ts'
import { PlayerProvider, usePlayer } from './lib/player.tsx'
import { useSettings } from './lib/settings.ts'

export type Route =
  | { view: 'home' }
  | { view: 'notebook'; id: string; open?: OpenTarget }
  | { view: 'settings' }
  | { view: 'transformations' }
  | { view: 'ask' }
  | { view: 'library' }
  | { view: 'usage' }

export default function App() {
  const state = useBackendState()
  const [route, setRoute] = useState<Route>({ view: 'home' })

  if (state.status === 'loading') {
    return (
      <div className="center" data-tauri-drag-region>
        <Spinner label="Opening" />
      </div>
    )
  }

  if (state.status === 'error') {
    return (
      <div className="shell in-settings">
        <div />
        <main className="desk">
          <div className="drag" data-tauri-drag-region />
          <div className="column">
            <ErrorNote error={state.error} />
          </div>
          <SettingsView />
        </main>
      </div>
    )
  }

  return (
    <PlayerProvider>
      <Shell route={route} setRoute={setRoute} />
      <ConfirmHost />
    </PlayerProvider>
  )
}

function Shell({ route, setRoute }: { route: Route; setRoute: (r: Route) => void }) {
  const player = usePlayer()
  const backend = useBackend()

  /** Opens a source or note from Ask or search, finding its notebook when needed. */
  const open = async (target: OpenTarget) => {
    let notebookId = target.notebookId
    if (!notebookId && target.kind === 'source') notebookId = (await backend.getSource(target.id)).notebooks?.[0]
    if (notebookId) setRoute({ view: 'notebook', id: notebookId, open: target })
  }
  const where = route.view === 'notebook' ? 'in-notebook' : route.view === 'home' ? 'at-root' : 'in-settings'

  return (
    <div className={`shell ${where}${player.episode ? ' has-player' : ''}`}>
      <Sidebar route={route} navigate={setRoute} />
      <main className="desk" style={{ minHeight: 0 }}>
        {route.view === 'notebook' ? (
          <NotebookView
            key={route.id}
            id={route.id}
            open={route.open}
            onBack={() => setRoute({ view: 'home' })}
            onDeleted={() => setRoute({ view: 'home' })}
            onSettings={() => setRoute({ view: 'settings' })}
          />
        ) : route.view === 'ask' ? (
          <div className="desk-body">
            <div className="drag" data-tauri-drag-region />
            <AskView onOpen={(t) => void open(t)} onBack={() => setRoute({ view: 'home' })} />
          </div>
        ) : route.view === 'usage' ? (
          <div className="desk-body">
            <div className="drag" data-tauri-drag-region />
            <UsageView onBack={() => setRoute({ view: 'home' })} />
          </div>
        ) : route.view === 'library' ? (
          <div className="desk-body">
            <div className="drag" data-tauri-drag-region />
            <LibraryView onOpen={(sourceId, notebookId) => setRoute({ view: 'notebook', id: notebookId, open: { kind: 'source', id: sourceId } })} onBack={() => setRoute({ view: 'home' })} />
          </div>
        ) : route.view === 'transformations' ? (
          <div className="desk-body">
            <div className="drag" data-tauri-drag-region />
            <TransformationsView onBack={() => setRoute({ view: 'home' })} />
          </div>
        ) : route.view === 'settings' ? (
          <div className="desk-body">
            <div className="drag" data-tauri-drag-region />
            <SettingsView onBack={() => setRoute({ view: 'home' })} />
          </div>
        ) : (
          <Home onOpen={(id) => setRoute({ view: 'notebook', id })} onSettings={() => setRoute({ view: 'settings' })} />
        )}
        <NowPlaying />
      </main>
      <MiniPlayer />
    </div>
  )
}

function Home({ onOpen, onSettings }: { onOpen: (id: string) => void; onSettings: () => void }) {
  const notebooks = useNotebooks()
  const settings = useSettings()
  const latest = notebooks.data?.find((n) => !n.archived)
  const needsModel = settings.mode === 'local' && !settings.chat?.model

  return (
    <div className="center" data-tauri-drag-region>
      {needsModel ? (
        <EmptyState title="Add a model to start asking">
          Notebooks live on this device. Questions go to the AI provider you choose, with your own API key.
          <div>
            <button className="button primary" onClick={onSettings}>
              Open settings
            </button>
          </div>
        </EmptyState>
      ) : latest ? (
        <EmptyState title="Pick a notebook">
          Or continue where you left off.
          <div>
            <button className="button" onClick={() => onOpen(latest.id)}>
              Open {latest.name}
            </button>
          </div>
        </EmptyState>
      ) : (
        <EmptyState title="Start a notebook">
          A notebook holds the sources for one question or project. Use the plus button next to Notebooks.
        </EmptyState>
      )}
    </div>
  )
}

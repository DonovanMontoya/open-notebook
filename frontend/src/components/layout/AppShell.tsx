'use client'

import { AppSidebar } from './AppSidebar'
import { SetupBanner } from './SetupBanner'

interface AppShellProps {
  children: React.ReactNode
}

export function AppShell({ children }: AppShellProps) {
  return (
    <div className="flex overflow-hidden" style={{ height: 'calc(100dvh - var(--podcast-player-height, 0px))' }}>
      <AppSidebar />
      <main className="flex-1 flex flex-col min-h-0 overflow-hidden">
        <SetupBanner />
        {children}
      </main>
    </div>
  )
}

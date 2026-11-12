'use client'

import { useState } from 'react'
import { Menu } from 'lucide-react'
import { AppSidebar } from './AppSidebar'
import { SetupBanner } from './SetupBanner'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { useTranslation } from '@/lib/hooks/use-translation'

interface AppShellProps { children: React.ReactNode }

export function AppShell({ children }: AppShellProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  const { t } = useTranslation()
  return (
    <div className="app-shell flex min-w-0 overflow-hidden" style={{ height: 'calc(100dvh - var(--podcast-player-height, 0px))' }}>
      <div className="hidden shrink-0 md:block"><AppSidebar /></div>
      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex min-h-14 shrink-0 items-center gap-3 border-b bg-sidebar px-4 md:hidden" style={{ paddingTop: 'env(safe-area-inset-top)' }}>
          <Button variant="ghost" size="icon" aria-label={t('mobile.openMenu')} onClick={() => setMenuOpen(true)}><Menu className="size-5" /></Button>
          <span className="font-display text-base font-semibold">{t('common.appName')}</span>
        </header>
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <SetupBanner />
          {children}
        </main>
      </div>
      <Dialog open={menuOpen} onOpenChange={setMenuOpen}>
        <DialogContent className="inset-y-0 left-0 top-0 h-dvh max-h-dvh w-[min(320px,90vw)] max-w-none translate-x-0 translate-y-0 gap-0 rounded-none border-y-0 border-l-0 p-0 sm:max-w-none" aria-describedby={undefined}>
          <DialogTitle className="sr-only">{t('mobile.navigation')}</DialogTitle>
          <AppSidebar mobile onNavigate={() => setMenuOpen(false)} />
        </DialogContent>
      </Dialog>
    </div>
  )
}

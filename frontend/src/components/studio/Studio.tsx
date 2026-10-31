'use client'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from '@/components/ui/dialog'
import { useTranslation } from '@/lib/hooks/use-translation'
import { useStudio, type StudyDeck, type StudyRequest } from '@/lib/hooks/use-studio'
import { StudyPlayer } from './StudyPlayer'
import { LiveTalk } from './LiveTalk'

export function Studio({ notebookId, sources }: { notebookId: string; sources: { id: string; title?: string | null }[] }) {
  const { t, language } = useTranslation()
  const { decks, generate } = useStudio(notebookId)
  const difficultyLabels = { easy: t('studio.easy'), medium: t('studio.medium'), hard: t('studio.hard') }
  const [open, setOpen] = useState(false)
  const [selected, setSelected] = useState<string[]>([])
  const [kind, setKind] = useState<StudyDeck['kind']>('flashcards')
  const [count, setCount] = useState(8)
  const [difficulty, setDifficulty] = useState<StudyRequest['difficulty']>('medium')
  const [instructions, setInstructions] = useState('')
  const [active, setActive] = useState<StudyDeck | null>(null)
  const [session, setSession] = useState(0)
  function play(deck: StudyDeck) { setActive(deck); setSession(value => value + 1) }
  return <>
    <Button className="mt-3" variant="outline" onClick={() => setOpen(true)}>{t('studio.title')}</Button>
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="sm:max-w-3xl max-h-[90vh] overflow-y-auto">
        <DialogHeader><DialogTitle>{t('studio.title')}</DialogTitle><DialogDescription>{t('studio.description')}</DialogDescription></DialogHeader>
        {active ? <><Button variant="outline" onClick={() => setActive(null)}>{t('studio.back')}</Button><h2 className="font-semibold">{active.title}</h2><StudyPlayer key={session} deck={active} /></> : <div className="space-y-4">
          <form className="space-y-4" onSubmit={async event => {
            event.preventDefault()
            try { play(await generate.mutateAsync({ kind, count, difficulty, instructions, language, source_ids: selected })) } catch { /* Mutation displays the error. */ }
          }}>
            <fieldset disabled={generate.isPending} className="space-y-4">
              <legend className="font-semibold">{t('studio.create')}</legend>
              <div className="flex flex-wrap gap-4">
                <label>{t('studio.format')}<select className="block border rounded p-2 bg-background" value={kind} onChange={e => setKind(e.target.value as StudyDeck['kind'])}><option value="flashcards">{t('studio.flashcards')}</option><option value="quiz">{t('studio.quiz')}</option></select></label>
                <label>{t('studio.count')}<input className="block border rounded p-2 w-24" type="number" min={1} max={20} required value={count} onChange={e => setCount(e.target.valueAsNumber)} /></label>
                <label>{t('studio.difficulty')}<select className="block border rounded p-2 bg-background" value={difficulty} onChange={e => setDifficulty(e.target.value as StudyRequest['difficulty'])}>{(['easy', 'medium', 'hard'] as const).map(level => <option key={level} value={level}>{difficultyLabels[level]}</option>)}</select></label>
              </div>
              <fieldset className="border rounded p-3 max-h-48 overflow-y-auto"><legend>{t('studio.sources')}</legend>
                {sources.length === 0 && <p>{t('studio.noSources')}</p>}
                {sources.map(source => <label key={source.id} className="flex gap-2 py-1"><input type="checkbox" checked={selected.includes(source.id)} onChange={e => setSelected(ids => e.target.checked ? [...ids, source.id] : ids.filter(id => id !== source.id))} />{source.title || source.id}</label>)}
              </fieldset>
              <label className="block">{t('studio.focus')}<textarea maxLength={2000} className="block w-full border rounded p-2" value={instructions} onChange={e => setInstructions(e.target.value)} /></label>
              <Button type="submit" disabled={!selected.length || !Number.isInteger(count) || count < 1 || count > 20}>{t(generate.isPending ? 'studio.generating' : 'studio.generate')}</Button>
            </fieldset>
          </form>
          <LiveTalk />
          <h3 className="font-semibold">{t('studio.savedDecks')}</h3>
          {decks.isLoading && <p>{t('common.loading')}</p>}
          {decks.isError && <Button variant="outline" onClick={() => decks.refetch()}>{t('studio.retry')}</Button>}
          {decks.data?.length === 0 && <p className="text-muted-foreground">{t('studio.empty')}</p>}
          {decks.data?.map((deck, index) => <Button key={index} className="block w-full h-auto whitespace-normal text-left" variant="outline" onClick={() => play(deck)}>{deck.title} · {t(`studio.${deck.kind}`)}</Button>)}
        </div>}
      </DialogContent>
    </Dialog>
  </>
}

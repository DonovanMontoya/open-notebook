'use client'

import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/lib/hooks/use-translation'
import type { StudyDeck } from '@/lib/hooks/use-studio'

export function StudyPlayer({ deck }: { deck: StudyDeck }) {
  const { t } = useTranslation()
  const [queue, setQueue] = useState(deck.items.map((_, index) => index))
  const [position, setPosition] = useState(0)
  const [revealed, setRevealed] = useState(false)
  const [choice, setChoice] = useState<number | null>(null)
  const [missed, setMissed] = useState<number[]>([])
  const finished = position >= queue.length
  function advance(correct: boolean) {
    if (!correct) setMissed(items => [...items, queue[position]])
    setPosition(value => value + 1)
    setRevealed(false)
    setChoice(null)
  }
  function restart(onlyMissed: boolean) {
    setQueue(onlyMissed ? missed : deck.items.map((_, index) => index))
    setPosition(0)
    setMissed([])
    setRevealed(false)
    setChoice(null)
  }
  if (finished) return <section className="space-y-4" aria-live="polite">
    <h3 className="text-xl font-semibold">{t('studio.results')}</h3>
    <p>{t('studio.score', { correct: queue.length - missed.length, total: queue.length })}</p>
    <Button onClick={() => restart(false)}>{t('studio.restart')}</Button>{' '}
    {missed.length > 0 && <Button variant="outline" onClick={() => restart(true)}>{t('studio.reviewMissed')}</Button>}
  </section>
  const item = deck.items[queue[position]]
  return <section className="space-y-4">
    <p aria-live="polite">{t('studio.progress', { current: position + 1, total: queue.length })}</p>
    <progress className="w-full" value={position} max={queue.length} aria-label={t('studio.progressLabel')} />
    <h3 className="text-xl font-semibold">{item.question}</h3>
    {deck.kind === 'quiz' ? <div className="grid gap-2">
      {item.options.map((option, index) => <Button key={index} variant={choice === index ? 'default' : 'outline'} className="h-auto whitespace-normal justify-start text-left" disabled={revealed} onClick={() => { setChoice(index); setRevealed(true) }}>{option}</Button>)}
    </div> : !revealed && <Button onClick={() => setRevealed(true)}>{t('studio.reveal')}</Button>}
    {revealed && <div className="space-y-3 rounded-lg border p-4" aria-live="polite">
      {deck.kind === 'quiz' && <p className="font-semibold">{t(choice === item.correct_index ? 'studio.correct' : 'studio.incorrect')}</p>}
      {deck.kind === 'quiz' && <p>{item.options[item.correct_index!]}</p>}
      <p className="whitespace-pre-wrap">{item.answer}</p>
      <p className="text-sm text-muted-foreground">{t('studio.sources')}: {item.source_ids.join(', ')}</p>
      {deck.kind === 'quiz' ? <Button onClick={() => advance(choice === item.correct_index)}>{t('studio.next')}</Button> : <div className="flex gap-2">
        <Button onClick={() => advance(true)}>{t('studio.gotIt')}</Button>
        <Button variant="outline" onClick={() => advance(false)}>{t('studio.missed')}</Button>
      </div>}
    </div>}
  </section>
}

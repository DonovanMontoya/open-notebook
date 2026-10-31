import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { StudyPlayer } from './StudyPlayer'
import type { StudyDeck } from '@/lib/hooks/use-studio'
vi.mock('@/lib/hooks/use-translation', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
const deck: StudyDeck = { version: 1, kind: 'flashcards', title: 'Test', items: [
  { question: 'Question one', answer: 'Answer one', options: [], correct_index: null, source_ids: ['source:1'] },
  { question: 'Question two', answer: 'Answer two', options: [], correct_index: null, source_ids: ['source:1'] },
] }
describe('StudyPlayer', () => {
  it('hides answers and reviews only missed cards', () => {
    render(<StudyPlayer deck={deck} />)
    expect(screen.queryByText('Answer one')).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('studio.reveal'))
    fireEvent.click(screen.getByText('studio.missed'))
    fireEvent.click(screen.getByText('studio.reveal'))
    fireEvent.click(screen.getByText('studio.gotIt'))
    expect(screen.getByText('studio.results')).toBeInTheDocument()
    fireEvent.click(screen.getByText('studio.reviewMissed'))
    expect(screen.getByText('Question one')).toBeInTheDocument()
    fireEvent.click(screen.getByText('studio.reveal'))
    fireEvent.click(screen.getByText('studio.gotIt'))
    expect(screen.getByText('studio.results')).toBeInTheDocument()
    expect(screen.queryByText('studio.reviewMissed')).not.toBeInTheDocument()
  })
  it('locks a quiz answer until advancing', () => {
    render(<StudyPlayer deck={{ ...deck, kind: 'quiz', items: [{ ...deck.items[0], options: ['A', 'B', 'C', 'D'], correct_index: 0 }] }} />)
    fireEvent.click(screen.getByText('B'))
    expect(screen.getByText('studio.incorrect')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'A' })).toBeDisabled()
    fireEvent.click(screen.getByText('studio.next'))
    expect(screen.getByText('studio.reviewMissed')).toBeInTheDocument()
  })
})

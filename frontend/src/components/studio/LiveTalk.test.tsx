import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { describe, it, expect, vi } from 'vitest'
import { LiveTalk } from './LiveTalk'
vi.mock('@/lib/hooks/use-translation', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
const create = vi.fn()
vi.mock('@/lib/api/client', () => ({ default: { post: (...args: unknown[]) => create(...args) } }))
function show() { return render(<QueryClientProvider client={new QueryClient()}><LiveTalk /></QueryClientProvider>) }
describe('LiveTalk', () => {
  it('does not request a microphone or start a billed session on render', () => {
    const getUserMedia = vi.fn()
    Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia } })
    show()
    expect(screen.getByText('live.start')).toBeInTheDocument()
    expect(getUserMedia).not.toHaveBeenCalled()
    expect(create).not.toHaveBeenCalled()
  })
  it('handles microphone denial without creating a provider session', async () => {
    Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia: vi.fn().mockRejectedValue(new Error('Denied')) } })
    show()
    fireEvent.click(screen.getByText('live.start'))
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('live.failed'))
    expect(create).not.toHaveBeenCalled()
  })
  it('stops a microphone granted after the user cancels', async () => {
    let grant!: (value: MediaStream) => void
    Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia: () => new Promise<MediaStream>(resolve => { grant = resolve }) } })
    show()
    fireEvent.click(screen.getByText('live.start'))
    fireEvent.click(screen.getByText('live.stop'))
    const stop = vi.fn()
    grant({ getTracks: () => [{ stop }] } as unknown as MediaStream)
    await waitFor(() => expect(stop).toHaveBeenCalled())
    expect(create).not.toHaveBeenCalled()
  })
})

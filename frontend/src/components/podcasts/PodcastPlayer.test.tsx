import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { EpisodePlayback, PodcastPlayerProvider, formatTime } from './PodcastPlayer'
import type { PodcastEpisode } from '@/lib/types/podcasts'
vi.mock('@/lib/hooks/use-translation', () => ({ useTranslation: () => ({ t: (key: string) => key }) }))
const post = vi.fn()
vi.mock('@/lib/api/client', () => ({ default: { post: (...args: unknown[]) => post(...args) } }))
vi.mock('@/lib/api/podcasts', () => ({ resolvePodcastAssetUrl: async (path: string) => path }))
const episode = { id: 'podcast_episode:a', name: 'Test episode', audio_url: '/audio' } as PodcastEpisode
const play = vi.fn().mockResolvedValue(undefined)
const pause = vi.fn()
beforeEach(() => {
  vi.clearAllMocks()
  Object.defineProperty(HTMLMediaElement.prototype, 'play', { configurable: true, value: play })
  Object.defineProperty(HTMLMediaElement.prototype, 'pause', { configurable: true, value: pause })
  Object.defineProperty(HTMLMediaElement.prototype, 'load', { configurable: true, value: vi.fn() })
  post.mockResolvedValue({ data: { url: '/api/podcasts/episodes/a/audio?media_ticket=test' } })
})
describe('persistent podcast player', () => {
  it('loads audio on demand and retains the same element when navigating away', async () => {
    const client = new QueryClient()
    const view = (show: boolean) => <QueryClientProvider client={client}><PodcastPlayerProvider>{show ? <EpisodePlayback episode={episode} /> : <p>Another page</p>}</PodcastPlayerProvider></QueryClientProvider>
    const result = render(view(true))
    expect(post).not.toHaveBeenCalled()
    const audio = result.container.querySelector('audio')!
    fireEvent.click(screen.getByText('player.play'))
    await waitFor(() => expect(play).toHaveBeenCalled())
    expect(audio.src).toContain('media_ticket=test')
    const calls = pause.mock.calls.length
    result.rerender(view(false))
    expect(result.container.querySelector('audio')).toBe(audio)
    expect(pause).toHaveBeenCalledTimes(calls)
    expect(screen.getByRole('region', { name: 'player.title' })).toBeInTheDocument()
    Object.defineProperty(audio, 'duration', { configurable: true, value: 120 })
    fireEvent.durationChange(audio)
    const slider = screen.getByRole('slider')
    fireEvent.change(slider, { target: { value: '65' } })
    fireEvent.keyUp(slider, { key: 'ArrowRight' })
    expect(audio.currentTime).toBe(65)
    fireEvent.change(screen.getByRole('combobox'), { target: { value: '1.5' } })
    expect(audio.playbackRate).toBe(1.5)
    fireEvent.click(screen.getByLabelText('player.close'))
    expect(audio.getAttribute('src')).toBeNull()
  })
  it('downloads through a native link without fetching an audio blob', async () => {
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    render(<QueryClientProvider client={new QueryClient()}><PodcastPlayerProvider><EpisodePlayback episode={episode} /></PodcastPlayerProvider></QueryClientProvider>)
    fireEvent.click(screen.getByText('player.download'))
    await waitFor(() => expect(click).toHaveBeenCalled())
    const link = click.mock.instances[0] as HTMLAnchorElement
    expect(link.href).toContain('download=true')
    expect(link.download).toBe('Test episode.mp3')
    expect(play).not.toHaveBeenCalled()
    click.mockRestore()
  })
  it('formats long episodes', () => { expect(formatTime(3661)).toBe('1:01:01'); expect(formatTime(NaN)).toBe('0:00') })
})

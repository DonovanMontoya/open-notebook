'use client'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { useTranslation } from '@/lib/hooks/use-translation'
import apiClient from '@/lib/api/client'

type LiveStatus = 'idle' | 'connecting' | 'connected' | 'closing' | 'ended' | 'failed'
export function LiveTalk() {
  const { t } = useTranslation()
  const [status, setStatus] = useState<LiveStatus>('idle')
  const [voice, setVoice] = useState('marin')
  const [muted, setMuted] = useState(false)
  const [error, setError] = useState('')
  const peer = useRef<RTCPeerConnection | null>(null)
  const channel = useRef<RTCDataChannel | null>(null)
  const stream = useRef<MediaStream | null>(null)
  const audio = useRef<HTMLAudioElement | null>(null)
  const generation = useRef(0)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const create = useMutation({
    mutationFn: async (sdp: string) => (await apiClient.post<{ transport: { sdp: string } }>('/live/session', { sdp, voice }, { timeout: 50000 })).data,
    retry: false,
  })
  const cleanup = useCallback(() => {
    generation.current++
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    stream.current?.getTracks().forEach(track => track.stop())
    stream.current = null
    const old = channel.current
    channel.current = null
    old?.close()
    peer.current?.close()
    peer.current = null
    if (audio.current) audio.current.srcObject = null
  }, [])
  useEffect(() => () => {
    if (channel.current?.readyState === 'open') channel.current.send(JSON.stringify({ type: 'session.close' }))
    cleanup()
  }, [cleanup])
  async function start() {
    cleanup()
    const current = generation.current
    setError(''); setMuted(false); setStatus('connecting')
    try {
      const mic = await navigator.mediaDevices.getUserMedia({ audio: true })
      if (current !== generation.current) { mic.getTracks().forEach(track => track.stop()); return }
      stream.current = mic
      const connection = new RTCPeerConnection()
      peer.current = connection
      connection.ontrack = event => {
        if (audio.current) {
          audio.current.srcObject = new MediaStream([event.track])
          void audio.current.play().catch(() => setError(t('live.playHint')))
        }
      }
      mic.getAudioTracks().forEach(track => connection.addTrack(track, mic))
      const events = connection.createDataChannel('oai-events')
      channel.current = events
      events.onmessage = ({ data }) => {
        if (current !== generation.current) return
        try {
          const event = JSON.parse(data)
          if (event.type === 'session.started') {
            if (timer.current) clearTimeout(timer.current)
            setStatus('connected')
            // Bound idle/forgotten sessions to 30 minutes.
            timer.current = setTimeout(() => {
              if (events.readyState === 'open') events.send(JSON.stringify({ type: 'session.close' }))
              cleanup(); setStatus('ended')
            }, 30 * 60 * 1000)
          } else if (event.type === 'session.closed') { cleanup(); setStatus('ended') }
          else if (event.type === 'error') { setError(t('live.providerError')); cleanup(); setStatus('failed') }
        } catch { /* Ignore non-JSON messages. */ }
      }
      events.onclose = () => {
        if (channel.current === events) { cleanup(); setStatus('ended') }
      }
      connection.onconnectionstatechange = () => {
        if (connection.connectionState === 'failed' && current === generation.current) {
          cleanup(); setStatus('failed'); setError(t('live.failed'))
        }
      }
      await connection.setLocalDescription(await connection.createOffer())
      if (connection.iceGatheringState !== 'complete') await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => { connection.removeEventListener('icegatheringstatechange', changed); reject(new Error('ICE timeout')) }, 10000)
        function changed() {
          if (connection.iceGatheringState === 'complete') {
            clearTimeout(timeout); connection.removeEventListener('icegatheringstatechange', changed); resolve()
          }
        }
        connection.addEventListener('icegatheringstatechange', changed); changed()
      })
      if (current !== generation.current) return
      const sdp = connection.localDescription?.sdp
      if (!sdp) throw new Error('Missing SDP')
      const result = await create.mutateAsync(sdp)
      if (current !== generation.current) return
      timer.current = setTimeout(() => { cleanup(); setStatus('failed'); setError(t('live.failed')) }, 20000)
      await connection.setRemoteDescription({ type: 'answer', sdp: result.transport.sdp })
    } catch {
      if (current === generation.current) { cleanup(); setStatus('failed'); setError(t('live.failed')) }
    }
  }
  function stop() {
    stream.current?.getAudioTracks().forEach(track => { track.enabled = false })
    if (channel.current?.readyState === 'open') {
      setStatus('closing')
      channel.current.send(JSON.stringify({ type: 'session.close' }))
      if (timer.current) clearTimeout(timer.current)
      timer.current = setTimeout(() => { cleanup(); setStatus('ended') }, 15000)
    } else { cleanup(); setStatus('ended') }
  }
  const busy = status === 'connecting' || status === 'connected' || status === 'closing'
  const statusText = { idle: t('live.idle'), connecting: t('live.connecting'), connected: t('live.connected'), closing: t('live.closing'), ended: t('live.ended'), failed: t('live.failed') }
  return <section className="mt-4 rounded border p-4 space-y-3">
    <h3 className="font-semibold">{t('live.title')}</h3>
    <p className="text-sm text-muted-foreground">{t('live.description')}</p>
    <div className="flex flex-wrap items-center gap-3">
      <label>{t('live.voice')} <select className="border rounded bg-background p-2" disabled={busy} value={voice} onChange={e => setVoice(e.target.value)}>{['marin', 'cedar', 'alloy', 'coral'].map(v => <option key={v} value={v}>{v}</option>)}</select></label>
      {!busy ? <Button onClick={start}>{t('live.start')}</Button> : <Button variant="outline" disabled={status === 'closing'} onClick={stop}>{t('live.stop')}</Button>}
      {status === 'connected' && <Button variant="outline" onClick={() => { stream.current?.getAudioTracks().forEach(track => { track.enabled = muted }); setMuted(!muted) }}>{t(muted ? 'live.unmute' : 'live.mute')}</Button>}
      <span role="status">{statusText[status]}</span>
    </div>
    {error && <p role="alert" className="text-destructive">{error}</p>}
    <audio ref={audio} autoPlay controls aria-label={t('live.audio')} />
  </section>
}

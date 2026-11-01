import type { LiveVoice } from '@open-notebook/engine'
import { useEffect, useRef, useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { errorMessage } from '../lib/format.ts'
import { Icon, Spinner } from './ui.tsx'

type Status = 'idle' | 'connecting' | 'connected' | 'closing' | 'ended' | 'failed'
const VOICES: LiveVoice[] = ['marin', 'cedar', 'alloy', 'coral']
const MAX_MINUTES = 30

/** A spoken conversation with GPT Live, like the web app's Live talk. */
export function LiveTalk() {
  const backend = useBackend()
  const [voice, setVoice] = useState<LiveVoice>('marin')
  const [status, setStatus] = useState<Status>('idle')
  const [error, setError] = useState<string>()
  const [muted, setMuted] = useState(false)
  const peer = useRef<RTCPeerConnection>(undefined)
  const channel = useRef<RTCDataChannel>(undefined)
  const mic = useRef<MediaStream>(undefined)
  const audio = useRef<HTMLAudioElement>(null)
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined)

  const cleanup = () => {
    clearTimeout(timer.current)
    channel.current?.close()
    peer.current?.close()
    mic.current?.getTracks().forEach((t) => t.stop())
    peer.current = channel.current = mic.current = undefined
    setMuted(false)
  }
  useEffect(() => cleanup, [])

  const start = async () => {
    cleanup()
    setError(undefined)
    setStatus('connecting')
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      mic.current = stream
      const connection = new RTCPeerConnection()
      peer.current = connection
      connection.ontrack = (event) => {
        if (audio.current) {
          audio.current.srcObject = new MediaStream([event.track])
          void audio.current.play()
        }
      }
      stream.getAudioTracks().forEach((t) => connection.addTrack(t, stream))
      const events = connection.createDataChannel('oai-events')
      channel.current = events
      events.onmessage = ({ data }) => {
        try {
          const event = JSON.parse(data) as { type: string }
          if (event.type === 'session.started') {
            clearTimeout(timer.current)
            setStatus('connected')
            timer.current = setTimeout(() => stop(), MAX_MINUTES * 60_000)
          } else if (event.type === 'session.closed') {
            cleanup()
            setStatus('ended')
          } else if (event.type === 'error') {
            cleanup()
            setStatus('failed')
            setError('OpenAI Live reported an error and ended the conversation.')
          }
        } catch {
          // Not JSON; ignore.
        }
      }
      connection.onconnectionstatechange = () => {
        if (connection.connectionState === 'failed') {
          cleanup()
          setStatus('failed')
          setError('The connection to OpenAI Live dropped.')
        }
      }
      await connection.setLocalDescription(await connection.createOffer())
      if (connection.iceGatheringState !== 'complete') {
        await new Promise<void>((resolve, reject) => {
          const giveUp = setTimeout(() => reject(new Error('Could not reach the network for Live')), 10_000)
          connection.addEventListener('icegatheringstatechange', () => {
            if (connection.iceGatheringState === 'complete') {
              clearTimeout(giveUp)
              resolve()
            }
          })
        })
      }
      const answer = await backend.createLiveSession(connection.localDescription!.sdp, voice)
      timer.current = setTimeout(() => {
        cleanup()
        setStatus('failed')
        setError('OpenAI Live did not answer in time.')
      }, 20_000)
      await connection.setRemoteDescription({ type: 'answer', sdp: answer })
    } catch (e) {
      cleanup()
      setStatus('failed')
      setError(e instanceof DOMException && e.name === 'NotAllowedError' ? 'Allow microphone access for Open Notebook in System Settings, Privacy & Security.' : errorMessage(e))
    }
  }

  const stop = () => {
    mic.current?.getAudioTracks().forEach((t) => (t.enabled = false))
    if (channel.current?.readyState === 'open') {
      setStatus('closing')
      channel.current.send(JSON.stringify({ type: 'session.close' }))
      timer.current = setTimeout(() => {
        cleanup()
        setStatus('ended')
      }, 5000)
    } else {
      cleanup()
      setStatus('ended')
    }
  }

  const live = status === 'connecting' || status === 'connected' || status === 'closing'
  return (
    <div className={`live-talk${status === 'connected' ? ' on' : ''}`}>
      <div className="live-text">
        <span className="live-title">
          <span className="live-dot" aria-hidden />
          Talk it through
        </span>
        <span className="voice-line">
          {status === 'connected'
            ? 'Listening. Speak naturally; you can interrupt.'
            : status === 'connecting'
              ? 'Connecting'
              : status === 'closing'
                ? 'Ending'
                : 'A spoken study companion. It does not read your sources.'}
        </span>
      </div>
      <div className="live-actions">
        {!live && (
          <select value={voice} onChange={(e) => setVoice(e.target.value as LiveVoice)} aria-label="Voice" className="small-select">
            {VOICES.map((v) => (
              <option key={v} value={v}>
                {v[0].toUpperCase() + v.slice(1)}
              </option>
            ))}
          </select>
        )}
        {status === 'connected' && (
          <button
            className="button"
            onClick={() => {
              mic.current?.getAudioTracks().forEach((t) => (t.enabled = muted))
              setMuted(!muted)
            }}
          >
            {muted ? 'Unmute' : 'Mute'}
          </button>
        )}
        {live ? (
          <button className="button" onClick={stop} disabled={status === 'closing'}>
            {status === 'connecting' ? <Spinner /> : <Icon name="stop" size={12} />} End
          </button>
        ) : (
          <button className="button" onClick={() => void start()}>
            Start talking
          </button>
        )}
      </div>
      {error && <p className="danger small">{error}</p>}
      <audio ref={audio} autoPlay hidden />
    </div>
  )
}

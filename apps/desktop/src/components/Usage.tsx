import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { EmptyState, ErrorNote, Icon, Spinner } from './ui.tsx'

const FEATURE_LABEL: Record<string, string> = {
  chat: 'Chat',
  ask: 'Ask',
  search: 'Search',
  embedding: 'Indexing sources',
  transformation: 'Insights',
  study: 'Study decks',
  podcast_writing: 'Podcast scripts',
  podcast_audio: 'Podcast voices',
  podcast_live_audio: 'Podcast voices (Live)',
  transcription: 'Transcription',
}

const n = new Intl.NumberFormat()
const usd = new Intl.NumberFormat(undefined, { style: 'currency', currency: 'USD', maximumFractionDigits: 2 })

export function UsageView({ onBack }: { onBack?: () => void }) {
  const backend = useBackend()
  const [days, setDays] = useState(30)
  const usage = useQuery({ queryKey: ['usage', days, backend.kind], queryFn: () => backend.usage(days) })
  const u = usage.data
  const tokens = (u?.groups ?? []).reduce((t, g) => t + g.inputTokens + g.outputTokens, 0)
  const audio = (u?.groups ?? []).reduce((t, g) => t + g.audioSeconds, 0)

  return (
    <div className="column usage">
      {onBack && (
        <button className="button quiet small back-button" onClick={onBack}>
          <Icon name="back" size={14} /> Notebooks
        </button>
      )}
      <h1 className="page-title">Usage</h1>
      <p className="page-intro">
        {backend.kind === 'remote'
          ? 'What the server spent on models, with its price estimates.'
          : 'Model calls made from this device. Token counts are estimated from text length; your provider’s dashboard has exact figures.'}
      </p>
      <div className="segmented" role="radiogroup" aria-label="Period" style={{ margin: '20px 0' }}>
        {[7, 30, 90].map((d) => (
          <button key={d} role="radio" aria-checked={days === d} className={days === d ? 'active' : ''} onClick={() => setDays(d)}>
            {d} days
          </button>
        ))}
      </div>
      {usage.isPending && <Spinner />}
      <ErrorNote error={usage.error} />
      {u && (
        <>
          <div className="stats">
            <div className="stat">
              <span className="stat-label">Calls</span>
              <span className="stat-value">{n.format(u.calls)}</span>
            </div>
            <div className="stat">
              <span className="stat-label">{u.estimated ? 'Tokens, estimated' : 'Tokens'}</span>
              <span className="stat-value">{n.format(tokens)}</span>
            </div>
            <div className="stat">
              <span className="stat-label">Audio</span>
              <span className="stat-value">{audio >= 60 ? `${n.format(Math.round(audio / 60))} min` : `${Math.round(audio)} s`}</span>
            </div>
            {u.estimatedUsd != null && (
              <div className="stat">
                <span className="stat-label">Estimated cost</span>
                <span className="stat-value">{usd.format(u.estimatedUsd)}</span>
              </div>
            )}
          </div>
          {u.groups.length === 0 ? (
            <EmptyState title="Nothing in this period" />
          ) : (
            <table className="usage-table">
              <thead>
                <tr>
                  <th>Used for</th>
                  <th className="model-col">Model</th>
                  <th className="num">Calls</th>
                  <th className="num">Tokens in</th>
                  <th className="num">Tokens out</th>
                  {u.estimatedUsd != null && <th className="num">Cost</th>}
                </tr>
              </thead>
              <tbody>
                {u.groups.map((g) => (
                  <tr key={`${g.feature}:${g.model}`}>
                    <td>
                      {FEATURE_LABEL[g.feature] ?? g.feature}
                      <span className="usage-model">{g.model}</span>
                    </td>
                    <td className="muted model-col">{g.model}</td>
                    <td className="num">{n.format(g.calls)}</td>
                    <td className="num">{g.audioSeconds ? '' : n.format(g.inputTokens)}</td>
                    <td className="num">{g.audioSeconds ? `${Math.round(g.audioSeconds)} s audio` : n.format(g.outputTokens)}</td>
                    {u.estimatedUsd != null && <td className="num">{g.estimatedUsd != null ? usd.format(g.estimatedUsd) : ''}</td>}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </>
      )}
    </div>
  )
}

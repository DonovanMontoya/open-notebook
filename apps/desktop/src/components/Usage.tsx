import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { costOf, MODEL_PRICES, priceFor, type Price } from '@open-notebook/engine'

import { useBackend } from '../lib/backend.tsx'
import { useStoredState } from '../lib/hooks.ts'
import { EmptyState, ErrorNote, Icon, Modal, Spinner } from './ui.tsx'

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
  live_talk: 'Live talk',
}

const n = new Intl.NumberFormat()
const usd = new Intl.NumberFormat(undefined, { style: 'currency', currency: 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2 })
/** Small amounts keep enough digits to be worth showing. */
const money = (v: number) => (v > 0 && v < 0.01 ? `<${usd.format(0.01)}` : usd.format(v))
const duration = (s: number) => (s >= 60 ? `${n.format(Math.round(s / 60))} min` : `${Math.round(s)} s`)

export function UsageView({ onBack }: { onBack?: () => void }) {
  const backend = useBackend()
  const [days, setDays] = useState(30)
  const [prices, setPrices] = useStoredState<Record<string, Price>>('open-notebook-model-prices', {})
  const [pricing, setPricing] = useState<{ model: string; audio: boolean }>()
  const usage = useQuery({ queryKey: ['usage', days, backend.kind], queryFn: () => backend.usage(days) })
  const u = usage.data
  const local = backend.kind !== 'remote'

  // On device, cost comes from the price list and the user's own prices; the server reports its own.
  const groups = (u?.groups ?? [])
    .map((g) => ({ ...g, cost: local ? costOf(g, prices) : (g.estimatedUsd ?? undefined) }))
    .sort((a, b) => (b.cost ?? -1) - (a.cost ?? -1) || b.calls - a.calls)
  const total = local ? groups.reduce((t, g) => t + (g.cost ?? 0), 0) : u?.estimatedUsd
  const unpriced = [...new Set(groups.filter((g) => g.cost === undefined).map((g) => g.model))]
  const tokens = groups.reduce((t, g) => t + g.inputTokens + g.outputTokens, 0)
  const audio = groups.reduce((t, g) => t + g.audioSeconds, 0)

  return (
    <div className="column usage">
      {onBack && (
        <button className="button quiet small back-button" onClick={onBack}>
          <Icon name="back" size={14} /> Notebooks
        </button>
      )}
      <h1 className="page-title">Usage</h1>
      <p className="page-intro">
        {local
          ? 'What model calls from this device cost, at list prices. Tokens are estimated from text length, so treat totals as close, not exact; your provider’s billing page has the final figures.'
          : 'What the server spent on models, with its price estimates.'}
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
          <div className="usage-total">
            <span className="usage-amount">{total != null ? usd.format(total) : 'No estimate'}</span>
            <span className="usage-caption">
              Estimated cost, last {days} days
              {unpriced.length > 0 && total != null && `, not counting ${unpriced.length === 1 ? unpriced[0] : `${unpriced.length} models`} without a price`}
            </span>
            <span className="usage-facts">
              {n.format(u.calls)} calls, {n.format(tokens)} {u.estimated ? 'tokens (estimated)' : 'tokens'}
              {audio > 0 && ` and ${duration(audio)} of audio`}
            </span>
          </div>
          {groups.length === 0 ? (
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
                  <th className="num">Cost</th>
                </tr>
              </thead>
              <tbody>
                {groups.map((g) => (
                  <tr key={`${g.feature}:${g.model}`}>
                    <td>
                      {FEATURE_LABEL[g.feature] ?? g.feature}
                      <span className="usage-model">{g.model}</span>
                    </td>
                    <td className="muted model-col">{g.model}</td>
                    <td className="num">{n.format(g.calls)}</td>
                    <td className="num">{g.audioSeconds ? '' : n.format(g.inputTokens)}</td>
                    <td className="num">{g.audioSeconds ? `${duration(g.audioSeconds)} audio` : n.format(g.outputTokens)}</td>
                    <td className="num">
                      {!local ? (
                        g.cost != null ? money(g.cost) : ''
                      ) : (
                        <button
                          className="link-button usage-cost"
                          onClick={() => setPricing({ model: g.model, audio: g.audioSeconds > 0 })}
                          title={g.cost === undefined ? `Set a price for ${g.model}` : `Change the price for ${g.model}`}
                        >
                          {g.cost === undefined ? 'Set price' : money(g.cost)}
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </>
      )}
      {pricing && (
        <PriceDialog
          model={pricing.model}
          audio={pricing.audio}
          price={priceFor(pricing.model, prices)}
          custom={!!prices[pricing.model]}
          onClose={() => setPricing(undefined)}
          onSave={(price) => {
            const next = { ...prices }
            if (price) next[pricing.model] = price
            else delete next[pricing.model]
            setPrices(next)
            setPricing(undefined)
          }}
        />
      )}
    </div>
  )
}

/** Edit what a model costs, per million tokens or per minute of audio. */
function PriceDialog({
  model,
  audio,
  price,
  custom,
  onClose,
  onSave,
}: {
  model: string
  audio: boolean
  price?: Price
  custom: boolean
  onClose: () => void
  onSave: (price: Price | undefined) => void
}) {
  const [input, setInput] = useState(price?.input?.toString() ?? '')
  const [output, setOutput] = useState(price?.output?.toString() ?? '')
  const [perMinute, setPerMinute] = useState(price?.perMinute?.toString() ?? '')
  const num = (v: string) => (v.trim() === '' || Number.isNaN(Number(v)) ? undefined : Number(v))
  const next: Price = audio ? { perMinute: num(perMinute) } : { input: num(input), output: num(output) }
  const valid = audio ? next.perMinute != null : next.input != null || next.output != null
  return (
    <Modal
      title={`Price for ${model}`}
      onClose={onClose}
      footer={
        <>
          {custom && (
            <button className="button quiet" onClick={() => onSave(undefined)} style={{ marginRight: 'auto' }}>
              {MODEL_PRICES[model] ? 'Use list price' : 'Clear price'}
            </button>
          )}
          <button className="button" onClick={onClose}>
            Cancel
          </button>
          <button className="button primary" disabled={!valid} onClick={() => onSave(next)}>
            Save price
          </button>
        </>
      }
    >
      <form
        className="form"
        onSubmit={(e) => {
          e.preventDefault()
          if (valid) onSave(next)
        }}
      >
        <p className="muted">Copy the rate from your provider’s pricing page. It applies to every use of this model on this device.</p>
        {audio ? (
          <label className="field">
            <span>Dollars per minute of audio</span>
            <input autoFocus inputMode="decimal" value={perMinute} onChange={(e) => setPerMinute(e.target.value)} placeholder="0.015" />
          </label>
        ) : (
          <div className="price-fields">
            <label className="field">
              <span>Input, $ per million tokens</span>
              <input autoFocus inputMode="decimal" value={input} onChange={(e) => setInput(e.target.value)} placeholder="2.00" />
            </label>
            <label className="field">
              <span>Output, $ per million tokens</span>
              <input inputMode="decimal" value={output} onChange={(e) => setOutput(e.target.value)} placeholder="10.00" />
            </label>
          </div>
        )}
        <button type="submit" hidden />
      </form>
    </Modal>
  )
}

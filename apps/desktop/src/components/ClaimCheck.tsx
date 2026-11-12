// How closely a podcast episode's claims follow the notebook's sources.
import type { CheckedClaim, Episode, EpisodeCheck } from '@open-notebook/engine'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { useBackend } from '../lib/backend.tsx'
import { Modal, Spinner } from './ui.tsx'

const VERDICT_LABEL: Record<CheckedClaim['verdict'], string> = {
  supported: 'Matches your sources',
  overstated: 'Overstated',
  unsupported: 'Not in your sources',
}

function totals(check: EpisodeCheck) {
  const supported = check.supported ?? 0
  const overstated = check.overstated ?? 0
  const unsupported = check.unsupported ?? 0
  return { supported, overstated, unsupported, total: supported + overstated + unsupported }
}

/** A plain verdict from the share of claims that match a passage in the sources. */
export function checkVerdict(check: EpisodeCheck): string {
  const { supported, total } = totals(check)
  if (!total) return 'No factual claims to check'
  const share = supported / total
  if (share >= 0.9) return 'Closely follows your sources'
  if (share >= 0.75) return 'Mostly follows your sources'
  return 'Check before relying on it'
}

function Meter({ check, large }: { check: EpisodeCheck; large?: boolean }) {
  const { supported, overstated, unsupported, total } = totals(check)
  const width = (n: number) => `${total ? (n / total) * 100 : 0}%`
  return (
    <span className={`claim-meter${large ? ' large' : ''}`} aria-hidden>
      <span className="supported" style={{ width: width(supported) }} />
      <span className="overstated" style={{ width: width(overstated) }} />
      <span className="unsupported" style={{ width: width(unsupported) }} />
    </span>
  )
}

/** The line under an episode: a meter and a verdict, or the state of the check. */
export function ClaimCheckLine({ episode, onCheck, checking }: { episode: Episode; onCheck: () => void; checking: boolean }) {
  const [open, setOpen] = useState(false)
  const check = episode.check
  if (checking || check?.status === 'checking') {
    return (
      <p className="claim-line muted">
        <Spinner /> Checking claims against your sources
      </p>
    )
  }
  if (!check) {
    return (
      <button className="link-button claim-line" onClick={onCheck}>
        Check claims against your sources
      </button>
    )
  }
  if (check.status === 'failed') {
    return (
      <p className="claim-line">
        <span className="danger">The claim check failed: {check.error}</span>
        <button className="link-button" onClick={onCheck}>
          Check again
        </button>
      </p>
    )
  }
  const { supported, total } = totals(check)
  return (
    <>
      <button className="claim-line claim-summary" onClick={() => setOpen(true)}>
        <Meter check={check} />
        <span>
          <strong>{checkVerdict(check)}</strong>
          {total > 0 && `, ${supported} of ${total} claims`}
        </span>
      </button>
      {open && <ClaimCheckDialog episode={episode} check={check} onClose={() => setOpen(false)} onCheck={onCheck} />}
    </>
  )
}

function ClaimCheckDialog({ episode, check, onClose, onCheck }: { episode: Episode; check: EpisodeCheck; onClose: () => void; onCheck: () => void }) {
  const { supported, overstated, unsupported, total } = totals(check)
  const claims = check.claims ?? []
  const flagged = claims.filter((c) => c.verdict !== 'supported')
  const matched = claims.filter((c) => c.verdict === 'supported')
  const models = check.writer && check.writer !== check.model ? `Written by ${check.writer}, checked by ${check.model}.` : `Written and checked by ${check.model}.`
  return (
    <Modal
      title={checkVerdict(check)}
      onClose={onClose}
      wide
      footer={
        <>
          <button
            className="button"
            onClick={() => {
              onCheck()
              onClose()
            }}
          >
            Check again
          </button>
          <button className="button primary" onClick={onClose}>
            Done
          </button>
        </>
      }
    >
      <div className="claim-check">
        <Meter check={check} large />
        <ul className="claim-legend">
          <li className="supported">
            <strong>{supported}</strong> match a passage in your sources
          </li>
          <li className="overstated">
            <strong>{overstated}</strong> overstated
          </li>
          <li className="unsupported">
            <strong>{unsupported}</strong> not in your sources
          </li>
        </ul>
        <p className="muted">
          {total} factual claims in “{episode.name}”. {models}
          {check.sourcesTotal ? ` Draws on ${check.sourcesUsed} of ${check.sourcesTotal} sources.` : ''} Each passage given as evidence was looked up in
          your sources by the app; a claim only counts as a match if its passage is there.
        </p>
        {check.history?.length ? (
          <p className="muted">
            Before recording, {check.history.length === 1 ? 'one round' : `${check.history.length} rounds`} of fixes took the flagged claims from{' '}
            {check.history[0].overstated + check.history[0].unsupported} to {overstated + unsupported}.
          </p>
        ) : null}

        {flagged.length > 0 && (
          <>
            <h3>Worth a second look</h3>
            <ul className="claims">
              {flagged.map((c, i) => (
                <ClaimItem key={i} claim={c} />
              ))}
            </ul>
          </>
        )}
        {matched.length > 0 && (
          <details className="claims-matched">
            <summary>Claims that match your sources ({matched.length})</summary>
            <ul className="claims">
              {matched.map((c, i) => (
                <ClaimItem key={i} claim={c} />
              ))}
            </ul>
          </details>
        )}
      </div>
    </Modal>
  )
}

function ClaimItem({ claim }: { claim: CheckedClaim }) {
  return (
    <li className={`claim ${claim.verdict}`}>
      <p className="claim-said">“{claim.said || claim.claim}”</p>
      <p className="claim-verdict">
        <strong>{VERDICT_LABEL[claim.verdict]}.</strong> {claim.note ?? (claim.said ? claim.claim : '')}
      </p>
      {claim.quote && claim.quoteFound && (
        <blockquote>
          {claim.quote}
          {claim.source && <cite>{claim.source}</cite>}
        </blockquote>
      )}
    </li>
  )
}

/** Starts a claim check and refreshes the episode list while it runs. */
export function useClaimCheck(episodeId: string, listKey: readonly unknown[]) {
  const backend = useBackend()
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => backend.checkEpisode(episodeId),
    onSettled: () => void qc.invalidateQueries({ queryKey: listKey }),
  })
}

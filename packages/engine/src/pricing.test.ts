import assert from 'node:assert/strict'
import { test } from 'node:test'

import { costOf, priceFor } from './pricing.ts'

test('costOf prices tokens and audio from the list', () => {
  assert.equal(costOf({ model: 'claude-opus-5-5', inputTokens: 1_000_000, outputTokens: 100_000, audioSeconds: 0 }), 4 + 2)
  assert.equal(costOf({ model: 'gpt-4o-mini-tts', inputTokens: 0, outputTokens: 0, audioSeconds: 600 }), 0.15)
  assert.ok(Math.abs(costOf({ model: 'gpt-live-1', inputTokens: 0, outputTokens: 0, audioSeconds: 90 })! - 0.075) < 1e-9)
})

test('unknown models have no price until the user sets one', () => {
  const usage = { model: 'gpt-6.1-sol', inputTokens: 2_000_000, outputTokens: 0, audioSeconds: 0 }
  assert.equal(costOf(usage), undefined)
  assert.equal(costOf(usage, { 'gpt-6.1-sol': { input: 1.5, output: 6 } }), 3)
})

test('dated model ids match their base price, and overrides win', () => {
  assert.deepEqual(priceFor('claude-sonnet-5-5-20261001'), { input: 2, output: 10 })
  assert.deepEqual(priceFor('claude-sonnet-5-5', { 'claude-sonnet-5-5': { input: 1, output: 5 } }), { input: 1, output: 5 })
})

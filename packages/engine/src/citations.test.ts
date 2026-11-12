import assert from 'node:assert/strict'
import { test } from 'node:test'

import { bestPassage, linkCitations, matchSource, sentenceAround } from './citations.ts'

const SOURCES = [
  { id: 's1', title: 'FTC-2022-Bringing-Dark-Patterns-to-Light.pdf' },
  { id: 's2', title: 'Training language models to follow instructions' },
  { id: 's3', title: 'Plan' },
]

test('matchSource handles file names, prefixes and case', () => {
  assert.equal(matchSource('FTC 2022 Bringing Dark Patterns to Light', SOURCES)?.id, 's1')
  assert.equal(matchSource('training language models', SOURCES)?.id, 's2')
  assert.equal(matchSource('plan', SOURCES)?.id, 's3')
  assert.equal(matchSource('Unrelated', SOURCES), undefined)
})

test('linkCitations turns known titles into cite links and leaves everything else', () => {
  const { markdown, cited } = linkCitations(
    'Dark patterns are common [FTC 2022 Bringing Dark Patterns to Light]. See [the docs](https://x.y) and [1]. ' +
      'Also [Plan; Training language models] and [Nope].',
    SOURCES,
  )
  assert.equal(
    markdown,
    'Dark patterns are common [FTC 2022 Bringing Dark Patterns to Light](cite:0). See [the docs](https://x.y) and [1]. ' +
      'Also [Plan](cite:1) [Training language models](cite:2) and [Nope].',
  )
  assert.deepEqual(cited.map((s) => s.id), ['s1', 's3', 's2'])
})

test('bestPassage picks the paragraph that shares the most terms', () => {
  const text = 'Intro about nothing much.\n\nThe launch happens on March third in Lisbon.\n\nBudget notes follow here.'
  const p = bestPassage(text, 'When is the launch in Lisbon?')!
  assert.equal(text.slice(p.start, p.end), 'The launch happens on March third in Lisbon.')
  assert.equal(bestPassage(text, 'zz'), undefined)
})

test('sentenceAround returns the sentence containing an index', () => {
  const text = 'First point here. Second point [Plan] matters. Third.'
  assert.equal(sentenceAround(text, text.indexOf('[Plan]')), 'Second point [Plan] matters.')
})

test('server citations by record id show the source title', () => {
  const { markdown, cited } = linkCitations('Shown in [source:s9] and [source_insight:x1].', [
    { id: 'source:s9', title: 'Primary sources' },
  ])
  assert.equal(markdown, 'Shown in [Primary sources](cite:0) and [source_insight:x1].')
  assert.equal(cited[0].id, 'source:s9')
})

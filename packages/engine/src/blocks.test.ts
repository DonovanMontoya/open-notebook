import assert from 'node:assert/strict'
import { test } from 'node:test'

import { finishedBlocks } from './blocks.ts'

test('only finished paragraphs are shown while streaming', () => {
  assert.equal(finishedBlocks('First para'), '')
  assert.equal(finishedBlocks('First para\n\nSecond is still bei'), 'First para')
  assert.equal(finishedBlocks('First para\n\n'), 'First para')
  assert.equal(finishedBlocks('One\n\nTwo\n\n- a\n- b'), 'One\n\nTwo')
})

test('blank lines inside an open code fence do not end a block', () => {
  const open = 'Intro\n\n```py\nx = 1\n\ny = 2'
  assert.equal(finishedBlocks(open), 'Intro')
  const closed = 'Intro\n\n```py\nx = 1\n\ny = 2\n```\nAfter the fence'
  assert.equal(finishedBlocks(closed), 'Intro\n\n```py\nx = 1\n\ny = 2\n```')
})

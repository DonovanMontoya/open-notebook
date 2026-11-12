import assert from 'node:assert/strict'
import { test } from 'node:test'

import { isMedia } from './transcribe.ts'
import { captionsToText, parseWatchPage, pickTrack, youtubeId } from './youtube.ts'

test('youtubeId understands the common URL shapes', () => {
  assert.equal(youtubeId('https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10'), 'dQw4w9WgXcQ')
  assert.equal(youtubeId('https://youtu.be/dQw4w9WgXcQ'), 'dQw4w9WgXcQ')
  assert.equal(youtubeId('https://www.youtube.com/shorts/abcdef123'), 'abcdef123')
  assert.equal(youtubeId('https://example.com/watch?v=x'), undefined)
})

test('parseWatchPage finds the title and caption tracks, even with braces inside strings', () => {
  const player = {
    videoDetails: { title: 'A talk {with braces}' },
    captions: {
      playerCaptionsTracklistRenderer: {
        captionTracks: [
          { baseUrl: 'https://yt/auto', languageCode: 'en', kind: 'asr' },
          { baseUrl: 'https://yt/es', languageCode: 'es' },
          { baseUrl: 'https://yt/en', languageCode: 'en-GB', name: { simpleText: 'English' } },
        ],
      },
    },
  }
  const html = `<script>var ytInitialPlayerResponse = ${JSON.stringify(player)};var other = {};</script>`
  const page = parseWatchPage(html)
  assert.equal(page.title, 'A talk {with braces}')
  assert.equal(page.tracks.length, 3)
  assert.equal(pickTrack(page.tracks, ['en'])?.baseUrl, 'https://yt/en', 'manual captions beat automatic ones')
  assert.equal(pickTrack(page.tracks, ['es'])?.baseUrl, 'https://yt/es')
  assert.equal(pickTrack(page.tracks, ['fr'])?.baseUrl, 'https://yt/es', 'falls back to any manual track')
})

test('captionsToText joins segments and starts paragraphs at long pauses', () => {
  const text = captionsToText({
    events: [
      { tStartMs: 0, dDurationMs: 1000, segs: [{ utf8: 'Hello' }, { utf8: ' there.' }] },
      { tStartMs: 1100, dDurationMs: 900, segs: [{ utf8: 'Second line.' }] },
      { tStartMs: 9000, dDurationMs: 1000, segs: [{ utf8: 'After a pause.' }] },
      { tStartMs: 10000, segs: [{ utf8: '\n' }] },
    ],
  })
  assert.equal(text, 'Hello there. Second line.\n\nAfter a pause.')
})

test('isMedia recognizes audio and video by type or name', () => {
  assert.ok(isMedia('audio/mpeg'))
  assert.ok(isMedia('', 'interview.m4a'))
  assert.ok(isMedia('video/mp4'))
  assert.ok(!isMedia('application/pdf', 'paper.pdf'))
})

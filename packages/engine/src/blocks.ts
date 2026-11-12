/**
 * While a reply streams in, show only the Markdown blocks that are finished:
 * everything up to the last blank line that is not inside a code fence. The
 * block still being written stays hidden, so paragraphs, lists and tables
 * appear whole instead of word by word.
 */
export function finishedBlocks(text: string): string {
  let inFence = false
  let lastBoundary = 0
  let lineStart = 0
  for (let i = 0; i <= text.length; i++) {
    if (i < text.length && text[i] !== '\n') continue
    const line = text.slice(lineStart, i)
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence
      // A closing fence ends a block on its own.
      if (!inFence && i < text.length) lastBoundary = i + 1
    } else if (!inFence && line.trim() === '' && i < text.length && lineStart > 0) {
      lastBoundary = lineStart
    }
    lineStart = i + 1
  }
  return text.slice(0, lastBoundary).trimEnd()
}

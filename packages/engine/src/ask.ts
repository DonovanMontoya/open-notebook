// Ask: research across the whole knowledge base, following the server's
// three-step method (prompts/ask): plan searches, answer each from what it
// finds, then write one final answer that cites documents by id.

export interface AskSearch {
  term: string
  instructions: string
}

export type AskStep =
  | { kind: 'strategy'; reasoning: string; searches: AskSearch[] }
  | { kind: 'answer'; term: string; answer: string }
  | { kind: 'final'; answer: string }

export interface AskResult {
  answer: string
  /** Documents the answer may cite, so citations can show titles. */
  documents: Array<{ id: string; title: string; notebookId?: string }>
}

export interface AskOptions {
  notebookId?: string
  onStep?: (step: AskStep) => void
  signal?: AbortSignal
}

const ROLE = 'You are a research assistant who helps users learn from documents in their workspace.'

export function strategyPrompt(question: string, maxSearches = 4): string {
  return [
    ROLE,
    'First step: plan how to find the information that answers the question.',
    `Explain your reasoning briefly, then give up to ${maxSearches} search queries. Each has a search term and instructions for what to extract from the results.`,
    'Answer with JSON only, in this shape:',
    '{"reasoning": "...", "searches": [{"term": "...", "instructions": "..."}]}',
    `# Question\n${question}`,
  ].join('\n\n')
}

export function parseStrategy(raw: string, maxSearches = 4): { reasoning: string; searches: AskSearch[] } {
  const text = raw.replace(/```(?:json)?/gi, '')
  const start = text.indexOf('{')
  const end = text.lastIndexOf('}')
  try {
    const data = JSON.parse(text.slice(start, end + 1)) as { reasoning?: unknown; searches?: unknown }
    const searches = (Array.isArray(data.searches) ? data.searches : [])
      .map((s: { term?: unknown; instructions?: unknown }) => ({
        term: typeof s?.term === 'string' ? s.term.trim() : '',
        instructions: typeof s?.instructions === 'string' ? s.instructions.trim() : '',
      }))
      .filter((s: AskSearch) => s.term)
      .slice(0, maxSearches)
    if (searches.length) return { reasoning: typeof data.reasoning === 'string' ? data.reasoning : '', searches }
  } catch {
    // Fall through to a single search on the question itself.
  }
  return { reasoning: '', searches: [] }
}

export function searchAnswerPrompt(question: string, search: AskSearch, results: Array<{ id: string; title: string; text: string }>): string {
  return [
    ROLE,
    `# Question\n${question}`,
    `# Search\nTerm: ${search.term}\nInstructions: ${search.instructions || 'Summarize what the results say about the term.'}`,
    '# Results',
    ...results.map((r) => `<document id="${r.id}" title="${r.title.replace(/"/g, "'")}">\n${r.text}\n</document>`),
    [
      '# Your job',
      'Answer from these results only. Cite each claim with the document id in square brackets, like [source:abc].',
      'Use ids exactly as given; never invent ids. If the results do not cover it, say so.',
      `Ids you may cite: ${[...new Set(results.map((r) => r.id))].join(', ') || 'none'}`,
    ].join('\n'),
  ].join('\n\n')
}

export function finalAnswerPrompt(question: string, reasoning: string, answers: Array<{ term: string; answer: string }>): string {
  return [
    ROLE,
    'Last step: write the final answer to the question from the partial answers below.',
    `# Question\n${question}`,
    reasoning ? `# Your search strategy\n${reasoning}` : '',
    '# Partial answers',
    ...answers.map((a) => `## ${a.term}\n${a.answer}`),
    [
      '# Your job',
      'Write a clear, complete answer in Markdown. Keep the citations from the partial answers, using the same [id] form.',
      'Only cite ids that appear in the partial answers. If the documents do not answer the question, say what is missing.',
    ].join('\n'),
  ]
    .filter(Boolean)
    .join('\n\n')
}

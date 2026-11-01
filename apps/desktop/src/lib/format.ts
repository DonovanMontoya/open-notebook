const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })

/** "3 minutes ago", "yesterday", or a date for anything older than a week. */
export function relativeTime(iso: string | undefined | null): string {
  if (!iso) return ''
  const date = new Date(iso.includes('T') || iso.endsWith('Z') ? iso : iso.replace(' ', 'T'))
  if (Number.isNaN(date.getTime())) return ''
  const seconds = (date.getTime() - Date.now()) / 1000
  const abs = Math.abs(seconds)
  if (abs < 45) return 'just now'
  if (abs < 3600) return rtf.format(Math.round(seconds / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(seconds / 3600), 'hour')
  if (abs < 7 * 86400) return rtf.format(Math.round(seconds / 86400), 'day')
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host
  } catch {
    return url
  }
}

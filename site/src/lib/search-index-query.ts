/**
 * The search box's matching, kept apart from the index builder because this
 * half runs in the browser and that half reads the filesystem.
 */

/** One thing a search can land on: a page, or a heading or call within one. */
export interface SearchEntry {
  /** What the result says. */
  readonly title: string
  /** Where it is, shown under the title: the page a heading is on, say. */
  readonly context: string
  readonly href: string
  /** Extra words that match but are not shown. */
  readonly keywords: string
  /** Pages before headings before calls, when two match as well as each other. */
  readonly kind: 'page' | 'heading' | 'call'
}

const KIND_RANK: Record<SearchEntry['kind'], number> = { page: 0, heading: 1, call: 2 }

/**
 * The entries matching `query`, best first.
 *
 * Every word in the query has to appear somewhere. A title that starts with
 * the query beats one that contains it, which beats a match only in the
 * keywords.
 */
export function searchEntries(entries: readonly SearchEntry[], query: string, limit = 30): SearchEntry[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  if (words.length === 0) return []
  const phrase = words.join(' ')
  const scored: Array<{ entry: SearchEntry; score: number }> = []
  for (const entry of entries) {
    const title = entry.title.toLowerCase()
    const haystack = `${title} ${entry.context.toLowerCase()} ${entry.keywords.toLowerCase()}`
    if (!words.every((word) => haystack.includes(word))) continue
    let score = KIND_RANK[entry.kind]
    if (title === phrase) score -= 30
    else if (title.startsWith(phrase)) score -= 20
    else if (title.includes(phrase)) score -= 10
    else if (words.every((word) => title.includes(word))) score -= 5
    scored.push({ entry, score })
  }
  scored.sort((left, right) => left.score - right.score || left.entry.title.length - right.entry.title.length)
  return scored.slice(0, limit).map(({ entry }) => entry)
}

import { api, groupSlug } from './api'
import { allDocs } from './content'
import type { SearchEntry } from './search-index-query'

/**
 * Everything the search box knows, built at build time.
 *
 * Served as one static JSON file and fetched the first time the box opens,
 * rather than carried in every page's HTML. It is small — titles, headings
 * and call names, not prose — so a substring match in the browser is all the
 * search there needs to be.
 */
export function searchIndex(): SearchEntry[] {
  const entries: SearchEntry[] = []
  for (const page of allDocs()) {
    entries.push({
      title: page.title,
      context: page.section,
      href: page.href,
      keywords: page.summary ?? '',
      kind: 'page',
    })
    for (const heading of page.headings) {
      if (heading.level < 2 || heading.level > 3) continue
      entries.push({
        title: heading.text,
        context: page.title,
        href: `${page.href}#${heading.id}`,
        keywords: page.section,
        kind: 'heading',
      })
    }
  }
  for (const group of api().groups) {
    const href = `/reference/${groupSlug(group.name)}`
    entries.push({ title: group.name, context: 'C ABI reference', href, keywords: group.docs.summary, kind: 'page' })
    for (const fn of group.functions) {
      entries.push({
        title: fn.name,
        context: `${group.name} · C ABI`,
        href: `${href}#${fn.symbol}`,
        keywords: `${fn.symbol} ${fn.docs.summary}`,
        kind: 'call',
      })
    }
  }
  return entries
}

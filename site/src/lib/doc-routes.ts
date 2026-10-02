import type { Metadata } from 'next'
import { allDocs, docBySlug } from './content'
import { pageMetadata } from './page-meta'

/**
 * The docs are routed one level and two levels deep — `/docs/getting-started`
 * and `/docs/concepts/data-model` — rather than through one catch-all, so that
 * each page can have its Markdown beside it at `index.html.md`. A catch-all
 * segment has to be the last one in a route, which leaves no room for that.
 */
export const MAX_DOC_DEPTH = 2

/** Every page at exactly `depth` segments, as route params. */
export function docParams(depth: 1 | 2): Array<{ a: string; b?: string }> {
  const pages = allDocs()
  const tooDeep = pages.filter((page) => page.slug.length > MAX_DOC_DEPTH)
  if (tooDeep.length > 0) {
    throw new Error(
      `docs are routed at most ${MAX_DOC_DEPTH} levels deep: ${tooDeep.map((page) => page.href).join(', ')}`,
    )
  }
  return pages
    .filter((page) => page.slug.length === depth)
    .map((page) => (depth === 1 ? { a: page.slug[0]! } : { a: page.slug[0]!, b: page.slug[1]! }))
}

export type DocParams = { a?: string; b?: string }

export function slugOf(params: DocParams): string[] {
  return [params.a, params.b].filter((segment): segment is string => Boolean(segment))
}

export function docMetadata(slug: readonly string[]): Metadata {
  const page = docBySlug(slug)
  if (!page) return {}
  return pageMetadata({
    path: page.href,
    title: page.title,
    description: page.summary ?? page.title,
    eyebrow: page.section,
  })
}

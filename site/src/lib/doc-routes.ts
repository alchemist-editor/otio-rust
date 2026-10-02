import type { Metadata } from 'next'
import { allDocs, docBySlug } from './content'
import { pageMetadata } from './page-meta'
import { absoluteUrl } from './site'

/**
 * The docs are routed one level and two levels deep — `/docs/getting-started`
 * and `/docs/concepts/data-model` — rather than through one catch-all, so that
 * each page can have its Markdown beside it at `index.html.md`. A catch-all
 * segment has to be the last one in a route, which leaves no room for that.
 */
export const MAX_DOC_DEPTH = 2

/**
 * Pages that moved, from where they were to where they are.
 *
 * A link somebody already shared should still land. `vercel.json` answers
 * these with a permanent redirect; every other host gets a small page at the
 * old path that sends the reader on and names the new one as canonical.
 */
export const MOVED_DOCS: Readonly<Record<string, string>> = {
  'data-model': '/docs/concepts/data-model',
  'timeline-structure': '/docs/concepts/timeline-structure',
  'time-ranges': '/docs/concepts/time-ranges',
  'how-the-sdks-are-made': '/docs/internals/how-the-sdks-are-made',
  'guides/otio-file-format': '/docs/concepts/otio-file-format',
}

/** Where a moved page went, or undefined when `slug` was never moved. */
export function movedTo(slug: readonly string[]): string | undefined {
  return MOVED_DOCS[slug.join('/')]
}

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

/** The old paths at `depth` segments, as route params, for the pages that send readers on. */
export function movedParams(depth: 1 | 2): Array<{ a: string; b?: string }> {
  return Object.keys(MOVED_DOCS)
    .map((path) => path.split('/'))
    .filter((segments) => segments.length === depth)
    .map((segments) => (depth === 1 ? { a: segments[0]! } : { a: segments[0]!, b: segments[1]! }))
}

export type DocParams = { a?: string; b?: string }

export function slugOf(params: DocParams): string[] {
  return [params.a, params.b].filter((segment): segment is string => Boolean(segment))
}

export function docMetadata(slug: readonly string[]): Metadata {
  const moved = movedTo(slug)
  if (moved) {
    return {
      title: 'Moved',
      alternates: { canonical: absoluteUrl(moved) },
      robots: { index: false, follow: true },
    }
  }
  const page = docBySlug(slug)
  if (!page) return {}
  return pageMetadata({
    path: page.href,
    title: page.title,
    description: page.summary ?? page.title,
    eyebrow: page.section,
  })
}

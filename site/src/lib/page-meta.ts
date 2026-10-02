import type { Metadata } from 'next'
import { SITE_NAME, absoluteUrl } from './site'

/**
 * Everything a page needs in its `<head>` beyond a title, built one way.
 *
 * Each page is described by its path, its title and one sentence, and
 * everything else follows from those: the canonical URL, the OpenGraph and
 * Twitter cards, the generated card image, and the Markdown version of the
 * page that `rel="alternate"` points a crawler or an agent at.
 */
export interface PageDescription {
  /** The page's path, as the site links to it: `/docs/concepts/data-model`. */
  readonly path: string
  /** The title on its own, without the site's name. */
  readonly title: string
  readonly description: string
  /** `article` for documentation, `website` for the landing pages. */
  readonly type?: 'article' | 'website'
  /** What the card image says above the title, such as the page's section. */
  readonly eyebrow?: string
}

/** Where the Markdown version of a page lives, following llmstxt.org. */
export function markdownPath(path: string): string {
  const trimmed = path.replace(/\/+$/, '')
  return `${trimmed}/index.html.md`
}

/** Where the generated card image of a page lives. */
export function ogImagePath(path: string): string {
  const trimmed = path.replace(/^\/+|\/+$/g, '')
  return `/og/${trimmed || 'index'}.png`
}

export const OG_IMAGE_SIZE = { width: 1200, height: 630 } as const

export function pageMetadata(page: PageDescription): Metadata {
  const url = absoluteUrl(page.path)
  const image = {
    url: absoluteUrl(ogImagePath(page.path)),
    width: OG_IMAGE_SIZE.width,
    height: OG_IMAGE_SIZE.height,
    alt: `${page.title} · ${SITE_NAME}`,
    type: 'image/png',
  }
  return {
    title: page.title,
    description: page.description,
    alternates: {
      canonical: url,
      types: { 'text/markdown': absoluteUrl(markdownPath(page.path)) },
    },
    openGraph: {
      type: page.type ?? 'article',
      url,
      siteName: SITE_NAME,
      title: page.title,
      description: page.description,
      locale: 'en_US',
      images: [image],
    },
    twitter: {
      card: 'summary_large_image',
      title: page.title,
      description: page.description,
      images: [image.url],
    },
  }
}

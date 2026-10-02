import { api, groupSlug } from './api'
import { allDocs } from './content'
import type { PageDescription } from './page-meta'
import { SITE_DESCRIPTION, SITE_NAME } from './site'

/**
 * Every HTML page the site builds, described once.
 *
 * The sitemap and the card images both walk this list, so a page added to
 * the site turns up in both or in neither.
 */
export function sitePages(): PageDescription[] {
  const docs = allDocs().map(
    (page): PageDescription => ({
      path: page.href,
      title: page.title,
      description: page.summary ?? page.title,
      eyebrow: page.section,
      type: 'article',
    }),
  )
  const reference = api().groups.map(
    (group): PageDescription => ({
      path: `/reference/${groupSlug(group.name)}`,
      title: group.name,
      description: group.docs.summary,
      eyebrow: 'C ABI reference',
      type: 'article',
    }),
  )
  return [
    { path: '/', title: SITE_NAME, description: SITE_DESCRIPTION, type: 'website' },
    { path: '/languages', title: 'Languages', description: LANGUAGES_DESCRIPTION, type: 'website' },
    { path: '/reference', title: 'The C ABI', description: REFERENCE_DESCRIPTION, eyebrow: 'Reference', type: 'website' },
    ...docs,
    ...reference,
  ]
}

export const LANGUAGES_DESCRIPTION =
  'Every language OpenTimelineIO can be used from here: Rust, Python, TypeScript, Go, Swift, C++, Zig, C, C# and Objective-C.'

export const REFERENCE_DESCRIPTION =
  'Every group of calls in libotio, generated from the same description the language SDKs are generated from.'

/** Facts about the site itself, in one place. */
export const SITE_NAME = 'OpenTimelineIO for Rust'

export const SITE_DESCRIPTION =
  'A pure-Rust OpenTimelineIO: the data model, the time math, the file-format adapters, and an SDK for every language they are generated into.'

export const REPOSITORY_URL = 'https://github.com/alchemist-editor/otio-rust'

/**
 * Where the site is served from, which every absolute URL it writes is built
 * on: canonical links, the sitemap, OpenGraph images and llms.txt.
 *
 * A static export cannot ask the request what host it arrived on, so this is
 * decided when the site is built. `SITE_URL` wins; on Vercel the project's
 * production domain is next, which its build environment always sets; and a
 * local build falls back to the dev server.
 */
export const SITE_URL = siteUrl()

function siteUrl(): string {
  const explicit = process.env.SITE_URL ?? process.env.NEXT_PUBLIC_SITE_URL
  if (explicit) return explicit.replace(/\/+$/, '')
  const vercel = process.env.VERCEL_PROJECT_PRODUCTION_URL
  if (vercel) return `https://${vercel.replace(/\/+$/, '')}`
  return 'http://localhost:3000'
}

/**
 * A path on this site, absolute.
 *
 * Page paths get the trailing slash the export serves them under, so a
 * canonical URL is the one that answers without a redirect. A path naming a
 * file, such as `/llms.txt`, is left alone.
 */
export function absoluteUrl(path: string): string {
  const isFile = /\.[a-z0-9]+$/i.test(path.split(/[?#]/)[0] ?? '')
  const withSlash = isFile || path.endsWith('/') ? path : `${path}/`
  return `${SITE_URL}${withSlash.startsWith('/') ? '' : '/'}${withSlash}`
}

/** A link to a path inside the repository, on the default branch. */
export function repositoryFile(path: string): string {
  return `${REPOSITORY_URL}/blob/main/${path}`
}

/** A link that opens a file in the repository for editing. */
export function repositoryEdit(path: string): string {
  return `${REPOSITORY_URL}/edit/main/${path}`
}

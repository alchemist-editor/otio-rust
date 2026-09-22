/** Facts about the site itself, in one place. */
export const SITE_NAME = 'OpenTimelineIO for Rust'

export const SITE_DESCRIPTION =
  'A pure-Rust OpenTimelineIO: the data model, the time math, the file-format adapters, and an SDK for every language they are generated into.'

export const REPOSITORY_URL = 'https://github.com/alchemist-editor/otio-rust'

/** A link to a path inside the repository, on the default branch. */
export function repositoryFile(path: string): string {
  return `${REPOSITORY_URL}/blob/main/${path}`
}

import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

/** This file's directory, whichever way the site is being built. */
const here = dirname(fileURLToPath(import.meta.url))

/** The `site/` directory. */
export const SITE_ROOT = resolve(here, '../..')

/** The repository root, which is where `sdk/api.json` and the crates live. */
export const REPO_ROOT = resolve(SITE_ROOT, '..')

/** Where the Markdown pages live. */
export const DOCS_ROOT = resolve(SITE_ROOT, 'content/docs')

/** Where the per-language code samples live, one directory per sample. */
export const SAMPLES_ROOT = resolve(SITE_ROOT, 'content/samples')

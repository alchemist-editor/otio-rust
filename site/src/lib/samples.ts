import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { SAMPLES_ROOT } from './paths'
import { SDK_LANGUAGES, type SdkLanguage } from './sdk-languages'
import { highlighter } from './highlighter'

/**
 * A code sample, in as many languages as it has been written in.
 *
 * Samples live as real files — `content/samples/<id>/<language>.<ext>` — for
 * one reason: a snippet pasted into prose is a snippet nobody ever compiles.
 * As files they can be opened in an editor, checked against the SDK they
 * claim to use, and eventually fed to a compiler.
 * `scripts/check-samples.mjs` already fails the build when one is missing a
 * shipping language, so the switcher can never show a reader a gap.
 */
export interface Sample {
  /** The directory name, and what a page refers to it by. */
  readonly id: string
  /** One variant per language the sample was written in, in tab order. */
  readonly variants: readonly SampleVariant[]
}

export interface SampleVariant {
  readonly languageId: string
  readonly label: string
  /** The sample's source, verbatim. Empty when the language has none yet. */
  readonly code: string
  /** That source, highlighted on the server so no grammar reaches the client. */
  readonly html: string
  /** The file it came from, relative to the repository root. */
  readonly source: string
  /**
   * Why this language has no sample, when it has none.
   *
   * A tab that says what is missing is a better answer than a tab that is
   * not there: a reader who came for Python should learn that the EDL
   * adapter is not bound yet, not silently get Go.
   */
  readonly unavailable?: string
}

function readVariant(id: string, language: SdkLanguage): SampleVariant | undefined {
  const file = join(SAMPLES_ROOT, id, `${language.id}.${language.extension}`)
  let code: string
  try {
    code = readFileSync(file, 'utf8')
  } catch {
    return readNote(id, language)
  }
  const trimmed = code.replace(/\s+$/, '')
  return {
    languageId: language.id,
    label: language.label,
    code: trimmed,
    // Numbered, because these are the longest blocks on the site and prose
    // that wants to point at one line needs a way to say which.
    html: highlighter.highlightToHtml(trimmed, { lang: language.grammar, lineNumbers: true }),
    source: `site/content/samples/${id}/${language.id}.${language.extension}`,
  }
}

/** The tab for a language that cannot do this yet, and what it says. */
function readNote(id: string, language: SdkLanguage): SampleVariant | undefined {
  const file = join(SAMPLES_ROOT, id, `${language.id}.unavailable`)
  let note: string
  try {
    note = readFileSync(file, 'utf8')
  } catch {
    return undefined
  }
  return {
    languageId: language.id,
    label: language.label,
    code: '',
    html: '',
    source: `site/content/samples/${id}/${language.id}.unavailable`,
    unavailable: note.trim(),
  }
}

/** Every sample on disk, keyed by id. Read once per build. */
let cache: Map<string, Sample> | undefined

export function allSamples(): Map<string, Sample> {
  if (cache) return cache
  const samples = new Map<string, Sample>()
  let entries: string[]
  try {
    entries = readdirSync(SAMPLES_ROOT)
  } catch {
    entries = []
  }
  for (const id of entries.sort()) {
    if (!statSync(join(SAMPLES_ROOT, id)).isDirectory()) continue
    const variants = SDK_LANGUAGES.map((language) => readVariant(id, language)).filter(
      (variant): variant is SampleVariant => variant !== undefined,
    )
    if (variants.length > 0) samples.set(id, { id, variants })
  }
  cache = samples
  return samples
}

export function sampleById(id: string): Sample | undefined {
  return allSamples().get(id)
}

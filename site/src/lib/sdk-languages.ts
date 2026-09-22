/**
 * The languages OpenTimelineIO can be used from, and what the site calls them.
 *
 * This list is the site's one definition of the switcher: the order of the
 * tabs, the file extension a sample is written in, and which grammar the
 * highlighter reaches for. `scripts/check-samples.mjs` reads it too, and
 * fails the build when a sample is missing a language that is `shipping`.
 *
 * Adding a target is an entry here plus a file per sample. Nothing else.
 */
export interface SdkLanguage {
  /** The id used in URLs, in sample filenames and in the stored preference. */
  readonly id: string
  /** What the tab says. */
  readonly label: string
  /** The grammar registered with the highlighter. */
  readonly grammar: string
  /** The extension a sample for this language is written with. */
  readonly extension: string
  /** One line on what using OTIO from here is like. */
  readonly blurb: string
  /**
   * `shipping` languages must have a file for every sample. `planned` ones
   * are advertised on the languages page but are not yet required to, so a
   * target being built in another branch does not fail this build.
   */
  readonly status: 'shipping' | 'planned'
  /** Where the binding or SDK lives in the repository. */
  readonly path: string
}

export const SDK_LANGUAGES: readonly SdkLanguage[] = [
  {
    id: 'rust',
    label: 'Rust',
    grammar: 'rust',
    extension: 'rs',
    blurb: 'The core itself. A document owns its objects; everything else is built on this.',
    status: 'shipping',
    path: 'crates/otio-core',
  },
  {
    id: 'python',
    label: 'Python',
    grammar: 'python',
    extension: 'py',
    blurb: 'A drop-in replacement for upstream OpenTimelineIO, measured against its own test suite.',
    status: 'shipping',
    path: 'crates/otio-python',
  },
  {
    id: 'typescript',
    label: 'TypeScript',
    grammar: 'ts',
    extension: 'ts',
    blurb: 'The core as WebAssembly, in a browser or in Node. No document to hold.',
    status: 'shipping',
    path: 'crates/otio-wasm/ts',
  },
  {
    id: 'go',
    label: 'Go',
    grammar: 'go',
    extension: 'go',
    blurb: 'cgo over libotio. Methods, embedded structs for the schema ladder, and an error.',
    status: 'shipping',
    path: 'sdk/go',
  },
  {
    id: 'swift',
    label: 'Swift',
    grammar: 'swift',
    extension: 'swift',
    blurb: 'A SwiftPM package: a class per schema, real enums, and throwing calls.',
    status: 'shipping',
    path: 'sdk/swift',
  },
  {
    id: 'cpp',
    label: 'C++',
    grammar: 'cpp',
    extension: 'cpp',
    blurb: 'Header-only C++17. Objects are small value types over an arena handle.',
    status: 'shipping',
    path: 'sdk/cpp',
  },
  {
    id: 'zig',
    label: 'Zig',
    grammar: 'zig',
    extension: 'zig',
    blurb: 'The one target that keeps the document in the open, because an arena is how Zig works.',
    status: 'shipping',
    path: 'sdk/zig',
  },
  {
    id: 'c',
    label: 'C',
    grammar: 'c',
    extension: 'c',
    blurb: 'The ABI every other SDK is generated from. Out-parameters and status codes.',
    status: 'shipping',
    path: 'crates/otio-capi',
  },
  {
    id: 'csharp',
    label: 'C#',
    grammar: 'csharp',
    extension: 'cs',
    blurb: 'Being generated now, alongside Objective-C.',
    status: 'planned',
    path: 'sdk/csharp',
  },
  {
    id: 'objectivec',
    label: 'Objective-C',
    grammar: 'objectivec',
    extension: 'm',
    blurb: 'Being generated now, alongside C#.',
    status: 'planned',
    path: 'sdk/objc',
  },
]

/** The languages every sample has to cover. */
export const SHIPPING_LANGUAGES = SDK_LANGUAGES.filter((language) => language.status === 'shipping')

/**
 * The languages a sample shows as tabs. Everything else is in the menu.
 *
 * Python is first because it is the binding most readers arrive with.
 * TypeScript is the one selected before a reader has chosen: the front page
 * sample has no Python yet, and the highlighted tab should be one that has
 * code to read.
 */
export const FEATURED_LANGUAGES = ['python', 'typescript', 'cpp'] as const

/** The language a reader sees first, before they have chosen one. */
export const DEFAULT_LANGUAGE: (typeof FEATURED_LANGUAGES)[number] = 'typescript'

/** Splits a sample's languages into the tabs and the menu, in switcher order. */
export function splitFeaturedLanguages<T extends { languageId: string }>(
  variants: readonly T[],
): { featured: T[]; more: T[] } {
  const featured: T[] = []
  for (const id of FEATURED_LANGUAGES) {
    const found = variants.find((variant) => variant.languageId === id)
    if (found) featured.push(found)
  }
  const pinned = new Set<string>(FEATURED_LANGUAGES)
  return { featured, more: variants.filter((variant) => !pinned.has(variant.languageId)) }
}

export function languageById(id: string): SdkLanguage | undefined {
  return SDK_LANGUAGES.find((language) => language.id === id)
}

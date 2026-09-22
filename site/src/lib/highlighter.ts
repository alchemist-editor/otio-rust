import { createHighlighter } from '@tanstack/highlight/core'
import { createTanStackMarkdownHighlighter } from '@tanstack/highlight/markdown'
import { cpp } from '@tanstack/highlight/languages/cpp'
import { diff } from '@tanstack/highlight/languages/diff'
import { go } from '@tanstack/highlight/languages/go'
import { json } from '@tanstack/highlight/languages/json'
import { markdown } from '@tanstack/highlight/languages/markdown'
import { plaintext } from '@tanstack/highlight/languages/plaintext'
import { python } from '@tanstack/highlight/languages/python'
import { shell } from '@tanstack/highlight/languages/shell'
import { toml } from '@tanstack/highlight/languages/toml'
import { ts } from '@tanstack/highlight/languages/ts'
import { yaml } from '@tanstack/highlight/languages/yaml'
import { additionalLanguages } from './languages'

/**
 * The one highlighter the whole site shares.
 *
 * Registration is explicit rather than `allLanguages`, because the library
 * asks for it to be: an unused grammar is bytes in the bundle. What is here
 * is what the docs actually contain — the SDK languages, plus the formats
 * that show up in a build instruction or a fixture.
 */
export const highlighter = createHighlighter({
  fallbackLanguage: 'plaintext',
  languages: [
    ...additionalLanguages,
    cpp,
    diff,
    go,
    json,
    markdown,
    plaintext,
    python,
    shell,
    toml,
    ts,
    yaml,
  ],
})

/** The same highlighter, in the shape `@tanstack/markdown` wants for fences. */
export const markdownHighlighter = createTanStackMarkdownHighlighter(highlighter)

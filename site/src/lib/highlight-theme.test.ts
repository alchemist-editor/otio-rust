import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { syntaxThemeCss } from './highlight-theme'
import { highlighter, markdownHighlighter } from './highlighter'
import { SAMPLES_ROOT } from './paths'
import { SDK_LANGUAGES } from './sdk-languages'

/**
 * Languages with no sample file yet. The switcher does not require one, but
 * the grammar is already registered and has to colour real code.
 */
const PLANNED_SOURCES: Record<string, string> = {
  csharp: `public static class Reader {
    public static void Main() {
        var document = Document.Open("cut.edl");
    }
}
`,
  objectivec: `#import "OTIODocument.h"

int main(void) {
    NSString *path = @"cut.edl";
    return 0;
}
`,
}

function sampleSource(id: string, extension: string): string {
  for (const directory of readdirSync(SAMPLES_ROOT)) {
    try {
      return readFileSync(join(SAMPLES_ROOT, directory, `${id}.${extension}`), 'utf8')
    } catch {
      // The next sample directory may have it.
    }
  }
  const planned = PLANNED_SOURCES[id]
  if (planned) return planned
  throw new Error(`no source for ${id}`)
}

describe('syntax theme', () => {
  it('paints every token class the grammars emit', () => {
    for (const token of ['keyword', 'string', 'comment', 'function', 'type', 'number', 'operator']) {
      expect(syntaxThemeCss).toContain(`.th-${token} { color: var(--th-${token}); }`)
    }
  })

  it('follows the Markdown guide for both wrappers the site renders', () => {
    expect(syntaxThemeCss).toContain('.markdown-renderer')
    expect(syntaxThemeCss).toContain('.dark .markdown-renderer')
    expect(syntaxThemeCss).toContain('pre.th-code')
    expect(syntaxThemeCss).toContain('.markdown-renderer pre.tm-code')
    expect(syntaxThemeCss).toContain('.markdown-renderer .tm-code--line-numbers')
    expect(syntaxThemeCss).toContain('.markdown-renderer .th-line--highlighted')
  })
})

describe('each language', () => {
  it.each(SDK_LANGUAGES.map((language) => [language.label, language] as const))(
    'highlights %s',
    (_label, language) => {
      const html = highlighter.highlightToHtml(sampleSource(language.id, language.extension).trimEnd(), {
        lang: language.grammar,
      })
      expect(html).toContain('th-keyword')
      expect(html).toMatch(/th-(string|comment)/)
      expect(html).not.toContain('<script')
    },
  )

  it('highlights the shell and toml fences the docs are written in', () => {
    expect(markdownHighlighter('cargo test --workspace\n', 'sh', {})).toContain('th-command')
    expect(markdownHighlighter('otio-core = "git"\n', 'toml', {})).toContain('th-string')
    expect(markdownHighlighter('<not html>\n', 'text', {})).toContain('&lt;not html&gt;')
  })
})

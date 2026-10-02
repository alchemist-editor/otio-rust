import { describe, expect, it } from 'vitest'
import { allDocs, docBySlug } from './content'
import { docMarkdown, llmsIndex, llmsLanguage, sampleMarkdown } from './markdown-export'
import { markdownPath, ogImagePath } from './page-meta'
import { searchIndex } from './search-index'
import { searchEntries } from './search-index-query'
import { SDK_LANGUAGES } from './sdk-languages'
import { sitePages } from './site-pages'

describe('docMarkdown', () => {
  it('replaces every sample comment with code', () => {
    const pages = allDocs().filter((page) => page.body.includes('::sample'))
    expect(pages.length).toBeGreaterThan(0)
    for (const page of pages) {
      const markdown = docMarkdown(page)
      expect(markdown, page.href).not.toContain('::sample')
      expect(markdown, page.href).not.toContain('No sample named')
    }
  })

  it('makes site links absolute and leaves none relative', () => {
    const page = docBySlug(['concepts', 'data-model'])!
    const markdown = docMarkdown(page)
    expect(markdown).toMatch(/\]\(https?:\/\/[^)]+\/docs\/concepts\/time-ranges\/\)/)
    expect(markdown).not.toMatch(/\]\(\/docs/)
  })

  it('shows one language when asked, and says when a sample has none', () => {
    const python = sampleMarkdown('read-an-edl', 'python')
    expect(python).toContain('```python')
    expect(python).not.toContain('```go')
    expect(sampleMarkdown('write-a-bundle', 'typescript')).toMatch(/^> Not available in TypeScript/)
  })

  it('every language page pins its samples to a language the sample has', () => {
    for (const page of allDocs().filter((doc) => doc.section === 'Languages')) {
      expect(docMarkdown(page), page.href).not.toMatch(/has no .* version/)
    }
  })
})

describe('links between pages', () => {
  it('every site-relative link in the docs names a page that exists', () => {
    const known = new Set([...sitePages().map((page) => page.path), '/llms.txt'])
    const broken: string[] = []
    let checked = 0
    for (const page of allDocs()) {
      for (const match of page.body.matchAll(/\]\((\/[^)#\s]*)(#[^)\s]*)?\)/g)) {
        checked += 1
        const target = match[1]!.replace(/\/$/, '') || '/'
        if (!known.has(target)) broken.push(`${page.href} -> ${match[1]}`)
      }
    }
    expect(checked).toBeGreaterThan(20)
    expect(broken).toEqual([])
  })
})

describe('llms.txt', () => {
  it('links every docs page and every language file', () => {
    const index = llmsIndex()
    for (const page of allDocs()) expect(index).toContain(markdownPath(page.href))
    for (const language of SDK_LANGUAGES) expect(index).toContain(`/llms/${language.id}.txt`)
  })

  it('a language file carries no other language’s samples', () => {
    const go = llmsLanguage('go')!
    expect(go).toContain('```go')
    expect(go).not.toContain('```python')
    expect(go).not.toContain('```swift')
    expect(llmsLanguage('klingon')).toBeUndefined()
  })
})

describe('sitePages', () => {
  it('covers every docs page once, with a card image path each', () => {
    const paths = sitePages().map((page) => page.path)
    expect(new Set(paths).size).toBe(paths.length)
    for (const page of allDocs()) expect(paths).toContain(page.href)
    expect(ogImagePath('/')).toBe('/og/index.png')
    expect(ogImagePath('/docs/concepts/data-model')).toBe('/og/docs/concepts/data-model.png')
  })
})

describe('search', () => {
  it('finds pages, headings and calls', () => {
    const entries = searchIndex()
    expect(entries.length).toBeGreaterThan(100)
    expect(searchEntries(entries, 'time ranges')[0]?.title).toBe('Time ranges')
    expect(searchEntries(entries, 'slide').some((entry) => entry.kind === 'heading')).toBe(true)
    expect(searchEntries(entries, '   ')).toEqual([])
  })
})

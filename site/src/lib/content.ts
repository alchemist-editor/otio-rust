import { readFileSync, readdirSync } from 'node:fs'
import { join, relative } from 'node:path'
import { parseMarkdown } from '@tanstack/markdown/parser'
import type { MarkdownDocument, MarkdownHeading } from '@tanstack/markdown'
import { DOCS_ROOT } from './paths'
import { docsExtensions } from './sample-extension'

/** One Markdown page, parsed. */
export interface DocPage {
  /** The URL path under `/docs`, as segments. `[]` is the docs index. */
  readonly slug: readonly string[]
  /** `docs/...` joined, for links and keys. */
  readonly href: string
  readonly title: string
  /** One line under the title. */
  readonly summary?: string
  /** Which group in the sidebar this belongs to. */
  readonly section: string
  /** Where it sits inside that group. */
  readonly order: number
  /** The parsed document, ready to render. */
  readonly document: MarkdownDocument
  /** Its headings, for the table of contents. */
  readonly headings: readonly MarkdownHeading[]
}

/**
 * The sidebar groups, in the order they appear.
 *
 * A page names its group in frontmatter; a group not listed here sorts to
 * the end, so adding a page never silently hides it.
 */
export const SECTIONS = ['Start here', 'The data model', 'Guides', 'Reference'] as const

const PARSE_OPTIONS = {
  frontmatter: true,
  headingIds: true,
  allowHtml: false,
  extensions: docsExtensions(),
}

/** A frontmatter block, which here is only ever flat `key: value` lines. */
function parseFrontmatter(raw: string | undefined): Record<string, string> {
  const fields: Record<string, string> = {}
  if (!raw) return fields
  for (const line of raw.split('\n')) {
    const colon = line.indexOf(':')
    if (colon === -1) continue
    const key = line.slice(0, colon).trim()
    const value = line
      .slice(colon + 1)
      .trim()
      .replace(/^["'](.*)["']$/, '$1')
    if (key) fields[key] = value
  }
  return fields
}

function markdownFiles(directory: string): string[] {
  const found: string[] = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) found.push(...markdownFiles(path))
    else if (entry.name.endsWith('.md')) found.push(path)
  }
  return found
}

let cache: DocPage[] | undefined

/** Every page under `content/docs`, in sidebar order. */
export function allDocs(): DocPage[] {
  if (cache) return cache

  const pages = markdownFiles(DOCS_ROOT).map((file): DocPage => {
    const source = readFileSync(file, 'utf8')
    const document = parseMarkdown(source, PARSE_OPTIONS)
    const fields = parseFrontmatter(document.frontmatter)
    const slug = relative(DOCS_ROOT, file)
      .replace(/\.md$/, '')
      .split(/[\\/]/)
      .filter((segment) => segment !== 'index')

    return {
      slug,
      href: ['/docs', ...slug].join('/'),
      title: fields.title ?? slug.at(-1) ?? 'Untitled',
      summary: fields.summary,
      section: fields.section ?? 'Guides',
      order: Number(fields.order ?? '100'),
      document,
      headings: document.headings ?? [],
    }
  })

  const sectionIndex = (section: string) => {
    const found = (SECTIONS as readonly string[]).indexOf(section)
    return found === -1 ? SECTIONS.length : found
  }

  pages.sort(
    (left, right) =>
      sectionIndex(left.section) - sectionIndex(right.section) ||
      left.order - right.order ||
      left.title.localeCompare(right.title),
  )

  cache = pages
  return pages
}

export function docBySlug(slug: readonly string[]): DocPage | undefined {
  const wanted = slug.join('/')
  return allDocs().find((page) => page.slug.join('/') === wanted)
}

/** The pages grouped for the sidebar, empty groups dropped. */
export function docSections(): Array<{ section: string; pages: DocPage[] }> {
  const groups = new Map<string, DocPage[]>()
  for (const page of allDocs()) {
    const list = groups.get(page.section) ?? []
    list.push(page)
    groups.set(page.section, list)
  }
  return [...groups].map(([section, pages]) => ({ section, pages }))
}

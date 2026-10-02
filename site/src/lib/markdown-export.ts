import { api, declarationOf, groupSlug, type ApiFunction, type ApiGroup } from './api'
import { allDocs, docSections, type DocPage } from './content'
import { markdownPath } from './page-meta'
import { sampleById, type SampleVariant } from './samples'
import { SDK_LANGUAGES, languageById, type SdkLanguage } from './sdk-languages'
import { REPOSITORY_URL, SITE_DESCRIPTION, SITE_NAME, absoluteUrl, repositoryFile } from './site'

/**
 * Every page of the site, as Markdown.
 *
 * The HTML is for people; this is for whatever reads a page without a
 * browser — an agent, a crawler, `curl` piped into a model. Each page has a
 * Markdown twin at `<page>/index.html.md`, which is where llmstxt.org says to
 * look for one, and the `llms*.txt` files are built out of the same pieces.
 *
 * The prose is already Markdown, so a docs page is its own source with two
 * things done to it: each `::sample` comment is replaced by the code it
 * names, and each site-relative link is made absolute, because a file read
 * outside the site has nothing to resolve `/docs/...` against.
 */

/** The fence label each language's code is tagged with. */
const FENCE: Record<string, string> = { ts: 'typescript' }

function fenceFor(language: SdkLanguage): string {
  return FENCE[language.grammar] ?? language.grammar
}

/** One variant of a sample, as a fenced block or as the note that stands in for it. */
function variantMarkdown(variant: SampleVariant, labelled: boolean): string {
  const language = languageById(variant.languageId)
  const heading = labelled ? `${variant.label}:\n\n` : ''
  if (variant.unavailable || !language) {
    return `${heading}> Not available in ${variant.label} yet. ${variant.unavailable ?? ''}`.trimEnd()
  }
  const fence = variant.code.includes('```') ? '````' : '```'
  return `${heading}${fence}${fenceFor(language)}\n${variant.code}\n${fence}\n\n<sub>Source: ${repositoryFile(variant.source)}</sub>`
}

/**
 * A sample, in one language or in all of them.
 *
 * Asked for one language the sample does not cover, this says so rather than
 * quietly showing another: an agent told it is reading Swift should not be
 * handed Go.
 */
export function sampleMarkdown(id: string, languageId?: string): string {
  const sample = sampleById(id)
  if (!sample) return `> No sample named \`${id}\`.`
  if (languageId) {
    const variant = sample.variants.find((candidate) => candidate.languageId === languageId)
    if (!variant) {
      const label = languageById(languageId)?.label ?? languageId
      return `> This sample (\`${id}\`) has no ${label} version.`
    }
    return variantMarkdown(variant, false)
  }
  return sample.variants.map((variant) => variantMarkdown(variant, true)).join('\n\n')
}

const SAMPLE_COMMENT = /<!--\s*::sample\s+([^>]*?)\s*-->/g

function attribute(attributes: string, name: string): string | undefined {
  return new RegExp(`${name}="([^"]*)"`).exec(attributes)?.[1]
}

/** Makes `](/docs/...)` links absolute, leaving code alone. */
function absoluteLinks(markdown: string): string {
  let inFence = false
  return markdown
    .split('\n')
    .map((line) => {
      if (/^\s*(```|~~~)/.test(line)) inFence = !inFence
      if (inFence) return line
      return line.replace(/\]\((\/[^)\s]*)\)/g, (_, path: string) => {
        const [bare, hash] = path.split('#')
        return `](${absoluteUrl(bare ?? '/')}${hash ? `#${hash}` : ''})`
      })
    })
    .join('\n')
}

/** Moves every ATX heading down by `levels`, so a page can nest under another. */
function demoteHeadings(markdown: string, levels: number): string {
  if (levels === 0) return markdown
  let inFence = false
  return markdown
    .split('\n')
    .map((line) => {
      if (/^\s*(```|~~~)/.test(line)) inFence = !inFence
      if (inFence || !/^#{1,6} /.test(line)) return line
      return `${'#'.repeat(levels)}${line}`
    })
    .join('\n')
}

export interface DocMarkdownOptions {
  /** Show every sample in this language only. */
  readonly language?: string
  /** How far to push the page's headings down; 0 makes the title an `#`. */
  readonly headingLevel?: number
  /** Leave off the source line at the foot. */
  readonly bare?: boolean
}

/** A docs page, as Markdown. */
export function docMarkdown(page: DocPage, options: DocMarkdownOptions = {}): string {
  const level = options.headingLevel ?? 0
  const lines = [`${'#'.repeat(level + 1)} ${page.title}`, '']
  if (page.summary) lines.push(`> ${page.summary}`, '')
  const body = page.body.replace(SAMPLE_COMMENT, (_, attributes: string) => {
    const id = attribute(attributes, 'id') ?? ''
    const pinned = attribute(attributes, 'lang')
    return sampleMarkdown(id, options.language ?? pinned)
  })
  lines.push(demoteHeadings(absoluteLinks(body.trim()), level))
  if (!options.bare) {
    lines.push('', '---', '', `Page: ${absoluteUrl(page.href)}`)
  }
  return `${lines.join('\n').trim()}\n`
}

// --- The reference -------------------------------------------------------

function functionMarkdown(fn: ApiFunction, level: number): string {
  const hashes = '#'.repeat(level)
  const parts = [`${hashes} \`${fn.name}\``, '', `\`${fn.symbol}\` · ${fn.role}${fn.optional ? ' · may answer nothing' : ''}`, '']
  if (fn.docs.summary) parts.push(fn.docs.summary, '')
  for (const paragraph of fn.docs.body) parts.push(paragraph, '')
  const declaration = declarationOf(fn.symbol)
  if (declaration) parts.push('```c', declaration, '```', '')
  if (fn.params.length > 0) {
    parts.push('| Parameter | Type | Role |', '| --- | --- | --- |')
    for (const param of fn.params) {
      parts.push(
        `| \`${param.name}\`${param.optional ? ' (optional)' : ''} | \`${param.type}\` | ${param.role.replace(/_/g, ' ')} |`,
      )
    }
    parts.push('')
  }
  return parts.join('\n')
}

/** One group of the C ABI, as Markdown. */
export function referenceGroupMarkdown(group: ApiGroup, headingLevel = 0): string {
  const top = '#'.repeat(headingLevel + 1)
  const lines = [`${top} ${group.name}`, '', `> ${group.docs.summary}`, '']
  for (const paragraph of group.docs.body) lines.push(paragraph, '')
  lines.push(
    `Symbols in this group are spelled ${group.prefixes.map((prefix) => `\`otio_${prefix}_…\``).join(' or ')}. An SDK drops that prefix.`,
    '',
  )
  for (const fn of group.functions) lines.push(functionMarkdown(fn, headingLevel + 2))
  if (headingLevel === 0) lines.push('---', '', `Page: ${absoluteUrl(`/reference/${groupSlug(group.name)}`)}`)
  return `${lines.join('\n').trim()}\n`
}

/** The reference's front page: every group, one line each. */
export function referenceIndexMarkdown(): string {
  const description = api()
  const count = description.groups.reduce((total, group) => total + group.functions.length, 0)
  const lines = [
    '# The C ABI',
    '',
    `> ${count} calls in ${description.groups.length} groups. Every SDK is generated from this interface, and so is this reference.`,
    '',
    `Version ${description.version}, from ${repositoryFile('sdk/api.json')}.`,
    '',
  ]
  for (const group of description.groups) {
    lines.push(
      `- [${group.name}](${absoluteUrl(markdownPath(`/reference/${groupSlug(group.name)}`))}): ${group.docs.summary} (${group.functions.length} calls)`,
    )
  }
  return `${lines.join('\n')}\n`
}

// --- The landing pages ---------------------------------------------------

export function languagesMarkdown(): string {
  const lines = [
    '# Languages',
    '',
    '> One core, many bindings. Everything below the Python bindings is generated from the C ABI rather than written by hand.',
    '',
  ]
  for (const language of SDK_LANGUAGES) {
    const page = allDocs().find((doc) => doc.href === `/docs/languages/${language.id}`)
    const link = page ? `[${language.label}](${absoluteUrl(markdownPath(page.href))})` : language.label
    lines.push(`- ${link}: ${language.blurb} Lives in \`${language.path}\`.`)
  }
  return `${lines.join('\n')}\n`
}

export function homeMarkdown(): string {
  return [
    `# ${SITE_NAME}`,
    '',
    `> ${SITE_DESCRIPTION}`,
    '',
    `Docs: ${absoluteUrl(markdownPath('/docs'))}`,
    `Languages: ${absoluteUrl(markdownPath('/languages'))}`,
    `C ABI reference: ${absoluteUrl(markdownPath('/reference'))}`,
    `For language models: ${absoluteUrl('/llms.txt')}`,
    `Source: ${REPOSITORY_URL}`,
    '',
  ].join('\n')
}

// --- llms.txt ------------------------------------------------------------

/** The per-language file's path. */
export function llmsLanguagePath(id: string): string {
  return `/llms/${id}.txt`
}

/**
 * `/llms.txt`, as llmstxt.org lays it out: a title, a one-line summary, some
 * context, then sections of links, each to the Markdown version of a page.
 *
 * The language files come first, because they are the most useful thing on
 * the list: the whole of the docs with every sample in one language, which
 * is what an agent writing code in that language wants in its context.
 */
export function llmsIndex(): string {
  const lines = [
    `# ${SITE_NAME}`,
    '',
    `> ${SITE_DESCRIPTION}`,
    '',
    'This is a port of OpenTimelineIO to Rust, not a reimplementation: the arithmetic, rounding and timecode behaviour follow upstream exactly, and a file written here opens unchanged in every tool that reads OTIO. Each language SDK is generated from one C ABI, so the same calls exist everywhere, spelled the way each language spells things.',
    '',
    'Every link below is Markdown. Every page on the site also has a Markdown version at `<page>/index.html.md`.',
    '',
    '## Docs in one language',
    '',
    'The whole documentation with every code sample shown in that language only, plus that language\'s own page. Load the one you are writing code in.',
    '',
  ]
  for (const language of SDK_LANGUAGES) {
    lines.push(`- [${language.label}](${absoluteUrl(llmsLanguagePath(language.id))}): ${language.blurb}`)
  }
  for (const { section, pages } of docSections()) {
    lines.push('', `## ${section}`, '')
    for (const page of pages) {
      lines.push(
        `- [${page.title}](${absoluteUrl(markdownPath(page.href))})${page.summary ? `: ${page.summary}` : ''}`,
      )
    }
  }
  lines.push(
    '',
    '## Reference',
    '',
    `- [The C ABI](${absoluteUrl(markdownPath('/reference'))}): every group of calls in libotio, generated from \`sdk/api.json\``,
  )
  for (const group of api().groups) {
    lines.push(
      `- [${group.name}](${absoluteUrl(markdownPath(`/reference/${groupSlug(group.name)}`))}): ${group.docs.summary}`,
    )
  }
  lines.push(
    '',
    '## Optional',
    '',
    `- [Everything](${absoluteUrl('/llms-full.txt')}): every docs page with samples in every language, and the whole C ABI reference, in one file`,
    `- [Source](${REPOSITORY_URL}): the repository`,
    '',
  )
  return lines.join('\n')
}

function preamble(title: string, note: string): string[] {
  return [`# ${title}`, '', `> ${SITE_DESCRIPTION}`, '', note, '', `Index of everything: ${absoluteUrl('/llms.txt')}`, '']
}

/** Every docs page in order, each under its section, samples as asked. */
function allDocsMarkdown(language: string | undefined, skip: (page: DocPage) => boolean): string[] {
  const lines: string[] = []
  for (const { section, pages } of docSections()) {
    const kept = pages.filter((page) => !skip(page))
    if (kept.length === 0) continue
    lines.push(`## ${section}`, '')
    for (const page of kept) lines.push(docMarkdown(page, { language, headingLevel: 2 }), '')
  }
  return lines
}

/** `/llms-full.txt`: the whole site, every language, and the reference. */
export function llmsFull(): string {
  const lines = preamble(
    `${SITE_NAME}: the complete documentation`,
    'Every documentation page, with each code sample in every language, followed by the whole C ABI reference.',
  )
  lines.push(...allDocsMarkdown(undefined, () => false))
  lines.push('## C ABI reference', '')
  for (const group of api().groups) lines.push(referenceGroupMarkdown(group, 2), '')
  return `${lines.join('\n').trim()}\n`
}

/**
 * `/llms/<language>.txt`: the docs as a reader of one language needs them.
 *
 * The language's own page leads, the other languages' pages are left out,
 * and every sample is shown in this language alone, or with the note that
 * says it cannot do that yet. C is the one language whose API *is* the
 * reference, so its file carries the reference too.
 */
export function llmsLanguage(id: string): string | undefined {
  const language = languageById(id)
  if (!language) return undefined
  const own = allDocs().find((page) => page.href === `/docs/languages/${id}`)
  const lines = preamble(
    `${SITE_NAME}: the ${language.label} documentation`,
    `The documentation with every code sample in ${language.label}. ${language.blurb} The binding lives in \`${language.path}\` (${repositoryFile(language.path)}).`,
  )
  if (own) lines.push(docMarkdown(own, { language: id, headingLevel: 1 }), '')
  lines.push(...allDocsMarkdown(id, (page) => page.section === 'Languages'))
  if (id === 'c') {
    lines.push('## C ABI reference', '')
    for (const group of api().groups) lines.push(referenceGroupMarkdown(group, 2), '')
  }
  return `${lines.join('\n').trim()}\n`
}

/** A Markdown file as a static response. */
export function markdownResponse(text: string): Response {
  return new Response(text, { headers: { 'content-type': 'text/markdown; charset=utf-8' } })
}

/** A plain-text file as a static response; `llms.txt` is served as text. */
export function textResponse(text: string): Response {
  return new Response(text, { headers: { 'content-type': 'text/plain; charset=utf-8' } })
}


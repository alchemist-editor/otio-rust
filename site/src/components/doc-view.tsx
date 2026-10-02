import Link from 'next/link'
import { ArrowLeft, ArrowRight, ChevronRight } from 'lucide-react'
import { JsonLd } from '@/components/json-ld'
import { Markdown } from '@/components/markdown'
import { PageActions } from '@/components/page-actions'
import { TableOfContents } from '@/components/table-of-contents'
import { neighbours, type DocPage } from '@/lib/content'
import { markdownPath } from '@/lib/page-meta'
import { SITE_NAME, absoluteUrl, repositoryEdit } from '@/lib/site'

/** One documentation page: where it sits, what it says, and where to go next. */
export function DocView({ page }: { page: DocPage }) {
  const { previous, next } = neighbours(page)

  const crumbs = [
    { name: 'Docs', path: '/docs' },
    ...(page.slug.length > 0 && page.section !== 'Start here' ? [{ name: page.section }] : []),
    ...(page.slug.length > 0 ? [{ name: page.title, path: page.href }] : []),
  ]

  return (
    <div className="flex gap-10 py-10">
      <JsonLd
        data={{
          '@context': 'https://schema.org',
          '@graph': [
            {
              '@type': 'TechArticle',
              headline: page.title,
              description: page.summary,
              url: absoluteUrl(page.href),
              articleSection: page.section,
              inLanguage: 'en',
              isPartOf: { '@type': 'WebSite', name: SITE_NAME, url: absoluteUrl('/') },
            },
            {
              '@type': 'BreadcrumbList',
              itemListElement: crumbs.map((crumb, index) => ({
                '@type': 'ListItem',
                position: index + 1,
                name: crumb.name,
                ...('path' in crumb && crumb.path ? { item: absoluteUrl(crumb.path) } : {}),
              })),
            },
          ],
        }}
      />
      <article className="min-w-0 max-w-3xl flex-1">
        <header className="mb-8">
          <nav aria-label="Breadcrumb" className="flex flex-wrap items-center gap-1 text-[0.78rem] text-muted">
            {crumbs.map((crumb, index) => (
              <span key={crumb.name} className="inline-flex items-center gap-1">
                {index > 0 ? <ChevronRight className="size-3 opacity-60" aria-hidden /> : null}
                {'path' in crumb && crumb.path && index < crumbs.length - 1 ? (
                  <Link href={crumb.path} className="hover:text-ink">
                    {crumb.name}
                  </Link>
                ) : (
                  <span className={index === crumbs.length - 1 ? 'text-ink' : undefined}>{crumb.name}</span>
                )}
              </span>
            ))}
          </nav>
          <h1 className="mt-3 text-3xl font-semibold tracking-tight">{page.title}</h1>
          {page.summary ? <p className="mt-3 text-lg text-muted">{page.summary}</p> : null}
          <div className="mt-5">
            <PageActions markdownHref={markdownPath(page.href)} editHref={repositoryEdit(page.sourcePath)} />
          </div>
        </header>

        <Markdown document={page.document} />

        {previous || next ? (
          <nav aria-label="Pages" className="mt-14 grid gap-3 border-t border-edge pt-6 sm:grid-cols-2">
            {previous ? (
              <Link
                href={previous.href}
                className="group flex items-center gap-4 rounded-[var(--radius)] border border-edge p-4 transition-colors hover:bg-surface"
              >
                <ArrowLeft className="size-4 shrink-0 text-muted transition-transform group-hover:-translate-x-0.5" />
                <span>
                  <span className="block text-xs uppercase tracking-[0.08em] text-muted">Previous</span>
                  <span className="mt-0.5 block font-medium">{previous.title}</span>
                </span>
              </Link>
            ) : (
              <span className="hidden sm:block" />
            )}
            {next ? (
              <Link
                href={next.href}
                className="group flex items-center justify-between gap-4 rounded-[var(--radius)] border border-edge p-4 text-right transition-colors hover:bg-surface"
              >
                <span className="flex-1">
                  <span className="block text-xs uppercase tracking-[0.08em] text-muted">Next</span>
                  <span className="mt-0.5 block font-medium">{next.title}</span>
                </span>
                <ArrowRight className="size-4 shrink-0 text-muted transition-transform group-hover:translate-x-0.5" />
              </Link>
            ) : null}
          </nav>
        ) : null}
      </article>

      <aside className="sticky top-14 hidden h-fit max-h-[calc(100dvh-4rem)] w-52 shrink-0 overflow-y-auto py-2 xl:block">
        <TableOfContents
          entries={page.headings
            .filter((heading) => heading.level >= 2 && heading.level <= 3)
            .map((heading) => ({ id: heading.id, text: heading.text, level: heading.level }))}
        />
      </aside>
    </div>
  )
}

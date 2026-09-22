import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import { Markdown } from '@/components/markdown'
import { TableOfContents } from '@/components/table-of-contents'
import { allDocs, docBySlug } from '@/lib/content'

export const dynamicParams = false

export function generateStaticParams() {
  return allDocs().map((page) => ({ slug: page.slug.length ? [...page.slug] : undefined }))
}

type Params = { slug?: string[] }

export async function generateMetadata({
  params,
}: {
  params: Promise<Params>
}): Promise<Metadata> {
  const page = docBySlug((await params).slug ?? [])
  if (!page) return {}
  return { title: page.title, description: page.summary }
}

export default async function DocPage({ params }: { params: Promise<Params> }) {
  const slug = (await params).slug ?? []
  const page = docBySlug(slug)
  if (!page) notFound()

  const pages = allDocs()
  const index = pages.findIndex((candidate) => candidate.href === page.href)
  const next = pages[index + 1]

  return (
    <div className="flex gap-10 py-10">
      <article className="min-w-0 max-w-3xl flex-1">
        <header className="mb-8">
          <p className="text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-accent">
            {page.section}
          </p>
          <h1 className="mt-2 text-3xl font-semibold tracking-tight">{page.title}</h1>
          {page.summary ? <p className="mt-3 text-lg text-muted">{page.summary}</p> : null}
        </header>

        <Markdown document={page.document} />

        {next ? (
          <nav className="mt-14 border-t border-edge pt-6">
            <Link
              href={next.href}
              className="group flex items-center justify-between gap-4 rounded-[var(--radius)] border border-edge p-4 transition-colors hover:bg-surface"
            >
              <span>
                <span className="block text-xs uppercase tracking-[0.08em] text-muted">Next</span>
                <span className="mt-0.5 block font-medium">{next.title}</span>
              </span>
              <ArrowRight className="size-4 shrink-0 text-muted transition-transform group-hover:translate-x-0.5" />
            </Link>
          </nav>
        ) : null}
      </article>

      <aside className="sticky top-14 hidden h-fit w-52 shrink-0 py-2 xl:block">
        <TableOfContents
          entries={page.headings
            .filter((heading) => heading.level >= 2 && heading.level <= 3)
            .map((heading) => ({ id: heading.id, text: heading.text, level: heading.level }))}
        />
      </aside>
    </div>
  )
}

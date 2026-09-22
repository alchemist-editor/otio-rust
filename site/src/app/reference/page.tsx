import type { Metadata } from 'next'
import Link from 'next/link'
import { api, groupSlug } from '@/lib/api'
import { Badge } from '@/components/ui/badge'
import { repositoryFile } from '@/lib/site'

export const metadata: Metadata = {
  title: 'C ABI reference',
  description:
    'Every group of calls in libotio, generated from the same description the language SDKs are generated from.',
}

export default function ReferenceIndex() {
  const description = api()
  const callCount = description.groups.reduce((total, group) => total + group.functions.length, 0)

  return (
    <div className="py-12">
      <header className="max-w-3xl">
        <p className="text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-accent">
          Reference
        </p>
        <h1 className="mt-2 text-3xl font-semibold tracking-tight">The C ABI</h1>
        <p className="mt-3 text-lg text-muted">
          {callCount} calls in {description.groups.length} groups. Every SDK on this site is
          generated from this interface, and so is this page — both read{' '}
          <Link href="/docs/how-the-sdks-are-made" className="text-accent underline underline-offset-4">
            the same description
          </Link>{' '}
          of it.
        </p>
        <p className="mt-3 text-sm text-muted">
          Version {description.version}, from{' '}
          <a
            href={repositoryFile('sdk/api.json')}
            className="text-accent underline underline-offset-4"
            target="_blank"
            rel="noreferrer"
          >
            sdk/api.json
          </a>
          .
        </p>
      </header>

      <div className="mt-10 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {description.groups.map((group) => (
          <Link
            key={group.name}
            href={`/reference/${groupSlug(group.name)}`}
            className="group rounded-[var(--radius)] border border-edge p-4 transition-colors hover:bg-surface"
          >
            <div className="flex items-baseline justify-between gap-3">
              <h2 className="font-medium group-hover:text-accent">{group.name}</h2>
              <Badge>{group.functions.length}</Badge>
            </div>
            <p className="mt-1.5 text-sm leading-relaxed text-muted">{group.docs.summary}</p>
          </Link>
        ))}
      </div>
    </div>
  )
}

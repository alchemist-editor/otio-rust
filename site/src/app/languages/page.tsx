import type { Metadata } from 'next'
import { LanguageIcon } from '@/components/language-icon'
import { Badge } from '@/components/ui/badge'
import { SDK_LANGUAGES } from '@/lib/sdk-languages'
import { repositoryFile } from '@/lib/site'

export const metadata: Metadata = {
  title: 'Languages',
  description: 'Every language OpenTimelineIO can be used from here, and where each one lives.',
}

export default function LanguagesPage() {
  return (
    <div className="mx-auto max-w-4xl px-4 py-12 sm:px-6">
      <header className="max-w-2xl">
        <p className="text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-accent">
          Languages
        </p>
        <h1 className="mt-2 text-3xl font-semibold tracking-tight">One core, many bindings</h1>
        <p className="mt-3 text-lg text-muted">
          Everything below the Python bindings is generated from the C ABI rather than written by
          hand, so every language gets the whole data model the day the core does.
        </p>
      </header>

      <ul className="mt-10 space-y-3">
        {SDK_LANGUAGES.map((language) => (
          <li
            key={language.id}
            className="rounded-[var(--radius)] border border-edge p-4 transition-colors hover:bg-surface"
          >
            <div className="flex flex-wrap items-center gap-2.5">
              <LanguageIcon id={language.id} className="size-4" />
              <h2 className="font-medium">{language.label}</h2>
              {language.status === 'planned' ? <Badge>being generated</Badge> : null}
              <a
                href={repositoryFile(language.path)}
                target="_blank"
                rel="noreferrer"
                className="ml-auto font-mono text-[0.78rem] text-muted underline decoration-dotted underline-offset-4 hover:text-accent"
              >
                {language.path}
              </a>
            </div>
            <p className="mt-1.5 text-sm leading-relaxed text-muted">{language.blurb}</p>
          </li>
        ))}
      </ul>
    </div>
  )
}

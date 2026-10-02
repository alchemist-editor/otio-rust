import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { CodeTabs } from '@/components/code-tabs'
import { Badge } from '@/components/ui/badge'
import { sampleById } from '@/lib/samples'
import { api } from '@/lib/api'
import { SDK_LANGUAGES } from '@/lib/sdk-languages'
import type { Metadata } from 'next'
import { JsonLd } from '@/components/json-ld'
import { LanguageIcon } from '@/components/language-icon'
import { docsInSection } from '@/lib/content'
import { pageMetadata } from '@/lib/page-meta'
import { REPOSITORY_URL, SITE_DESCRIPTION, SITE_NAME, absoluteUrl } from '@/lib/site'

const FACTS = [
  {
    title: 'A port, not a rewrite',
    body: 'The arithmetic, the rounding and the timecode behaviour follow upstream exactly, measured against upstream’s own test suites.',
  },
  {
    title: 'No C++, no Python underneath',
    body: 'The core is Rust the whole way down. A binding loads one Rust library and nothing else.',
  },
  {
    title: 'Every SDK is generated',
    body: 'One description of the C ABI, read out of its own source, becomes the SDK for each language. Drift fails the build.',
  },
]

export const metadata: Metadata = pageMetadata({
  path: '/',
  title: SITE_NAME,
  description: SITE_DESCRIPTION,
  type: 'website',
})

/** Where a reader goes next, by what they came to do. */
const PATHS = [
  {
    href: '/docs/getting-started',
    title: 'Get started',
    body: 'Build the core and reach it from your language, or install the npm package.',
  },
  {
    href: '/docs/guides/reading-and-writing',
    title: 'Read and write files',
    body: 'EDL, ALE, Final Cut Pro XML, AAF and bundles: what each format carries and loses.',
  },
  {
    href: '/docs/concepts/data-model',
    title: 'Understand the model',
    body: 'Timelines, tracks, clips and the five ranges every item has.',
  },
  {
    href: '/reference',
    title: 'Look up a call',
    body: 'Every call in the C ABI the SDKs are generated from, with its declaration.',
  },
]

export default function Home() {
  const hero = sampleById('read-an-edl')
  const callCount = api().groups.reduce((total, group) => total + group.functions.length, 0)

  const guides = new Set(docsInSection('Languages').map((page) => page.slug.at(-1)))

  return (
    <main>
      <JsonLd
        data={{
          '@context': 'https://schema.org',
          '@graph': [
            {
              '@type': 'WebSite',
              name: SITE_NAME,
              url: absoluteUrl('/'),
              description: SITE_DESCRIPTION,
            },
            {
              '@type': 'SoftwareSourceCode',
              name: SITE_NAME,
              description: SITE_DESCRIPTION,
              codeRepository: REPOSITORY_URL,
              programmingLanguage: SDK_LANGUAGES.map((language) => language.label),
              license: 'https://www.apache.org/licenses/LICENSE-2.0',
            },
          ],
        }}
      />
      <section className="mx-auto max-w-7xl px-4 pb-16 pt-16 sm:px-6 sm:pt-24">
        <div className="grid items-start gap-12 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          <div className="max-w-xl">
            <Badge variant="accent">OpenTimelineIO 0.19 schemas</Badge>
            <h1 className="mt-4 text-4xl font-semibold tracking-tight sm:text-5xl">
              Editorial timelines, in every language you ship in.
            </h1>
            <p className="mt-5 text-lg leading-relaxed text-muted">
              A pure-Rust OpenTimelineIO — the data model, the time math, and the adapters for EDL,
              ALE, Final Cut and AAF — with {SDK_LANGUAGES.length} bindings generated from one C
              interface of {callCount} calls.
            </p>
            <div className="mt-8 flex flex-wrap gap-3">
              <Button variant="accent" render={<Link href="/docs" />}>
                Read the docs <ArrowRight aria-hidden />
              </Button>
              <Button render={<Link href="/reference" />}>Browse the ABI</Button>
            </div>
          </div>

          <div className="min-w-0">
            {hero ? (
              <>
                <p className="mb-2 text-sm text-muted">
                  Reading a cut and asking what is in it — pick your language, and the whole site
                  follows.
                </p>
                <CodeTabs variants={hero.variants} />
              </>
            ) : null}
          </div>
        </div>
      </section>

      <section className="border-t border-edge">
        <div className="mx-auto max-w-7xl px-4 py-14 sm:px-6">
          <h2 className="text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-muted">Start here</h2>
          <div className="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {PATHS.map((path) => (
              <Link
                key={path.href}
                href={path.href}
                className="group rounded-[var(--radius)] border border-edge p-4 transition-colors hover:bg-surface"
              >
                <h3 className="flex items-center justify-between font-medium group-hover:text-accent">
                  {path.title}
                  <ArrowRight className="size-4 text-muted transition-transform group-hover:translate-x-0.5" />
                </h3>
                <p className="mt-1.5 text-sm leading-relaxed text-muted">{path.body}</p>
              </Link>
            ))}
          </div>

          <h2 className="mt-12 text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-muted">
            Pick your language
          </h2>
          <div className="mt-4 flex flex-wrap gap-2">
            {SDK_LANGUAGES.map((language) => (
              <Link
                key={language.id}
                href={guides.has(language.id) ? `/docs/languages/${language.id}` : '/languages'}
                className="inline-flex items-center gap-2 rounded-[var(--radius)] border border-edge px-3 py-1.5 text-sm transition-colors hover:bg-surface"
              >
                <LanguageIcon id={language.id} className="size-4" />
                {language.label}
              </Link>
            ))}
          </div>
        </div>
      </section>

      <section className="border-t border-edge bg-surface/40">
        <div className="mx-auto grid max-w-7xl gap-8 px-4 py-14 sm:grid-cols-3 sm:px-6">
          {FACTS.map((fact) => (
            <div key={fact.title}>
              <h2 className="font-medium">{fact.title}</h2>
              <p className="mt-2 text-sm leading-relaxed text-muted">{fact.body}</p>
            </div>
          ))}
        </div>
      </section>
    </main>
  )
}

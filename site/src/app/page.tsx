import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { CodeTabs } from '@/components/code-tabs'
import { Badge } from '@/components/ui/badge'
import { sampleById } from '@/lib/samples'
import { api } from '@/lib/api'
import { SDK_LANGUAGES } from '@/lib/sdk-languages'

const FACTS = [
  {
    title: 'A port, not a rewrite',
    body: 'The arithmetic, the rounding and the timecode behaviour follow upstream exactly, measured against upstream’s own test suites.',
  },
  {
    title: 'No C++, no Python underneath',
    body: 'The core is Rust the whole way down. A binding links a static library and nothing else.',
  },
  {
    title: 'Every SDK is generated',
    body: 'One description of the C ABI, read out of its own source, becomes the SDK for each language. Drift fails the build.',
  },
]

export default function Home() {
  const hero = sampleById('read-an-edl')
  const callCount = api().groups.reduce((total, group) => total + group.functions.length, 0)

  return (
    <main>
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

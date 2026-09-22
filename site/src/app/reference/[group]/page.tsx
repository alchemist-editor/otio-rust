import type { Metadata } from 'next'
import Link from 'next/link'
import { notFound } from 'next/navigation'
import { ArrowLeft } from 'lucide-react'
import { api, declarationHtml, groupBySlug, groupSlug, type ApiFunction } from '@/lib/api'
import { Badge } from '@/components/ui/badge'

export const dynamicParams = false

export function generateStaticParams() {
  return api().groups.map((group) => ({ group: groupSlug(group.name) }))
}

type Params = { group: string }

export async function generateMetadata({
  params,
}: {
  params: Promise<Params>
}): Promise<Metadata> {
  const group = groupBySlug((await params).group)
  if (!group) return {}
  return { title: `${group.name} · C ABI`, description: group.docs.summary }
}

/** What a parameter's role means, said once rather than in every row. */
const ROLE_NOTES: Record<string, string> = {
  output: 'written through, not read',
  output_list: 'a list written through, sized by the count',
  output_count: 'how many the call had to give',
  list_capacity: 'how many the buffer can hold',
  receiver: 'what the call is about',
  document_in: 'the document, borrowed',
  document_mut: 'the document, changed',
  document_taken: 'the document, consumed',
  length: 'the length of the buffer before it',
  bytes: 'a buffer of bytes',
}

function Signature({ fn }: { fn: ApiFunction }) {
  const html = declarationHtml(fn.symbol)
  if (!html) return null
  return (
    <div
      className="code-surface mt-4 text-[0.82rem]"
      // The declaration is the one in the committed header, highlighted at
      // build time. Nothing here reassembles a signature from parts.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}

export default async function GroupPage({ params }: { params: Promise<Params> }) {
  const group = groupBySlug((await params).group)
  if (!group) notFound()

  return (
    <div className="py-12">
      <Link
        href="/reference"
        className="inline-flex items-center gap-1.5 text-sm text-muted transition-colors hover:text-ink"
      >
        <ArrowLeft className="size-3.5" /> Every group
      </Link>

      <header className="mt-6 max-w-3xl">
        <h1 className="text-3xl font-semibold tracking-tight">{group.name}</h1>
        <p className="mt-3 text-lg text-muted">{group.docs.summary}</p>
        {group.docs.body.map((paragraph) => (
          <p key={paragraph} className="mt-3 text-muted">
            {paragraph}
          </p>
        ))}
        <p className="mt-4 text-sm text-muted">
          Symbols in this group are spelled{' '}
          {group.prefixes.map((prefix, index) => (
            <span key={prefix}>
              {index > 0 ? ' or ' : ''}
              <code className="rounded bg-surface px-1.5 py-0.5 font-mono text-[0.8rem]">
                otio_{prefix}_…
              </code>
            </span>
          ))}
          . An SDK drops that prefix, which is why the name beside each call is the short one.
        </p>
      </header>

      <div className="mt-10 max-w-4xl space-y-10">
        {group.functions.map((fn) => (
          <section key={fn.symbol} id={fn.symbol} className="scroll-mt-20">
            <div className="flex flex-wrap items-baseline gap-2">
              <h2 className="font-mono text-lg font-medium">{fn.name}</h2>
              <Badge variant="accent">{fn.role}</Badge>
              {fn.optional ? <Badge>may answer nothing</Badge> : null}
            </div>
            <p className="mt-2 text-muted">{fn.docs.summary}</p>
            {fn.docs.body.map((paragraph) => (
              <p key={paragraph} className="mt-2 text-sm text-muted">
                {paragraph}
              </p>
            ))}

            <Signature fn={fn} />

            {fn.params.length > 0 ? (
              <table className="mt-4 w-full text-sm">
                <thead>
                  <tr className="text-left text-[0.7rem] uppercase tracking-[0.08em] text-muted">
                    <th className="border-b border-edge py-2 pr-4 font-semibold">Parameter</th>
                    <th className="border-b border-edge py-2 pr-4 font-semibold">Type</th>
                    <th className="border-b border-edge py-2 font-semibold">Role</th>
                  </tr>
                </thead>
                <tbody>
                  {fn.params.map((param) => (
                    <tr key={param.name}>
                      <td className="border-b border-edge py-2 pr-4 font-mono text-[0.82rem]">
                        {param.name}
                        {param.optional ? <span className="text-muted"> ?</span> : null}
                      </td>
                      <td className="border-b border-edge py-2 pr-4 font-mono text-[0.82rem] text-muted">
                        {param.type}
                      </td>
                      <td className="border-b border-edge py-2 text-muted">
                        {ROLE_NOTES[param.role] ?? param.role.replace(/_/g, ' ')}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : null}
          </section>
        ))}
      </div>
    </div>
  )
}

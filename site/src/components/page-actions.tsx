'use client'

import { useEffect, useState } from 'react'
import { Check, Copy, FileText, Pencil } from 'lucide-react'

const linkClass =
  'inline-flex items-center gap-1.5 rounded-[var(--radius)] border border-edge px-2.5 py-1 text-[0.78rem] text-muted transition-colors hover:bg-surface hover:text-ink'

/**
 * What a reader can do with the page as a whole: take its Markdown, read it
 * raw, or fix it.
 *
 * Copying fetches the same `index.html.md` file the link opens, rather than
 * carrying the text in the page, so the HTML does not ship every page twice.
 */
export function PageActions({ markdownHref, editHref }: { markdownHref: string; editHref?: string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle')

  useEffect(() => {
    if (state === 'idle') return
    const timer = window.setTimeout(() => setState('idle'), 1600)
    return () => window.clearTimeout(timer)
  }, [state])

  return (
    <div className="flex flex-wrap items-center gap-2">
      <button
        type="button"
        className={`${linkClass} cursor-pointer`}
        onClick={() => {
          void fetch(markdownHref)
            .then((response) => {
              if (!response.ok) throw new Error(String(response.status))
              return response.text()
            })
            .then((text) => navigator.clipboard.writeText(text))
            .then(
              () => setState('copied'),
              () => setState('failed'),
            )
        }}
      >
        {state === 'copied' ? <Check className="size-3.5" aria-hidden /> : <Copy className="size-3.5" aria-hidden />}
        {state === 'copied' ? 'Copied' : state === 'failed' ? 'Could not copy' : 'Copy as Markdown'}
      </button>
      <a href={markdownHref} className={linkClass}>
        <FileText className="size-3.5" aria-hidden /> View Markdown
      </a>
      {editHref ? (
        <a href={editHref} target="_blank" rel="noreferrer" className={linkClass}>
          <Pencil className="size-3.5" aria-hidden /> Edit on GitHub
        </a>
      ) : null}
    </div>
  )
}

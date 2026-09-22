'use client'

import { useEffect, useState } from 'react'
import { cn } from '@/lib/cn'

export interface TocEntry {
  readonly id: string
  readonly text: string
  readonly level: number
}

/**
 * The headings of the current page, with the one being read marked.
 *
 * `IntersectionObserver` rather than scroll arithmetic: the browser already
 * knows which headings are on screen, and asking it costs nothing per frame.
 */
export function TableOfContents({ entries }: { entries: readonly TocEntry[] }) {
  const [active, setActive] = useState<string | undefined>(entries[0]?.id)

  useEffect(() => {
    if (entries.length === 0) return
    const headings = entries
      .map((entry) => document.getElementById(entry.id))
      .filter((element): element is HTMLElement => element !== null)

    const observer = new IntersectionObserver(
      (records) => {
        const visible = records
          .filter((record) => record.isIntersecting)
          .sort((left, right) => left.boundingClientRect.top - right.boundingClientRect.top)
        if (visible[0]?.target.id) setActive(visible[0].target.id)
      },
      { rootMargin: '-80px 0px -70% 0px', threshold: 0 },
    )

    for (const heading of headings) observer.observe(heading)
    return () => observer.disconnect()
  }, [entries])

  if (entries.length < 2) return null

  return (
    <nav aria-label="On this page" className="text-sm">
      <h2 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-muted">
        On this page
      </h2>
      <ul className="space-y-1.5">
        {entries.map((entry) => (
          <li key={entry.id} style={{ paddingLeft: `${(entry.level - 2) * 0.75}rem` }}>
            <a
              href={`#${entry.id}`}
              className={cn(
                'block leading-snug transition-colors',
                active === entry.id ? 'text-accent' : 'text-muted hover:text-ink',
              )}
            >
              {entry.text}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  )
}

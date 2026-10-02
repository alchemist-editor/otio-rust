'use client'

import { Dialog } from '@base-ui/react/dialog'
import { useRouter } from 'next/navigation'
import { useEffect, useMemo, useRef, useState } from 'react'
import { CornerDownLeft, FileText, Hash, Search as SearchIcon, SquareFunction } from 'lucide-react'
import { cn } from '@/lib/cn'
import { searchEntries, type SearchEntry } from '@/lib/search-index-query'

const ICONS = { page: FileText, heading: Hash, call: SquareFunction } as const

let loaded: Promise<SearchEntry[]> | undefined

/** The index, fetched once per visit and only when someone opens the box. */
function loadIndex(): Promise<SearchEntry[]> {
  loaded ??= fetch('/search-index.json').then(
    (response) => (response.ok ? (response.json() as Promise<SearchEntry[]>) : []),
    () => {
      loaded = undefined
      return []
    },
  )
  return loaded
}

/**
 * Search across every page, heading and C ABI call.
 *
 * `/` or ⌘K / Ctrl+K opens it from anywhere; arrows move, Enter goes.
 */
export function Search() {
  const router = useRouter()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [entries, setEntries] = useState<SearchEntry[]>([])
  const [active, setActive] = useState(0)
  const list = useRef<HTMLUListElement>(null)

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const typing = target?.closest('input, textarea, [contenteditable="true"]')
      if ((event.key === 'k' && (event.metaKey || event.ctrlKey)) || (event.key === '/' && !typing)) {
        event.preventDefault()
        setOpen(true)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    if (open) void loadIndex().then(setEntries)
  }, [open])

  const results = useMemo(() => searchEntries(entries, query), [entries, query])

  useEffect(() => {
    list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [active])

  const go = (entry: SearchEntry | undefined) => {
    if (!entry) return
    setOpen(false)
    setQuery('')
    router.push(entry.href)
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger
        className="inline-flex h-8 cursor-pointer items-center gap-2 rounded-[var(--radius)] border border-edge px-2.5 text-sm text-muted transition-colors hover:bg-surface hover:text-ink"
        aria-label="Search the docs"
      >
        <SearchIcon className="size-3.5" aria-hidden />
        <span className="hidden md:inline">Search</span>
        <kbd className="hidden rounded border border-edge px-1 font-mono text-[0.65rem] md:inline">⌘K</kbd>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-50 bg-black/40 backdrop-blur-[2px]" />
        <Dialog.Popup className="fixed left-1/2 top-[12vh] z-50 w-[min(40rem,calc(100vw-2rem))] -translate-x-1/2 overflow-hidden rounded-[var(--radius)] border border-edge bg-canvas shadow-2xl outline-none">
          <Dialog.Title className="sr-only">Search the docs</Dialog.Title>
          <div className="flex items-center gap-2 border-b border-edge px-3">
            <SearchIcon className="size-4 text-muted" aria-hidden />
            <input
              autoFocus
              value={query}
              onChange={(event) => {
                setQuery(event.target.value)
                setActive(0)
              }}
              onKeyDown={(event) => {
                if (event.key === 'ArrowDown') {
                  event.preventDefault()
                  setActive((index) => Math.min(index + 1, results.length - 1))
                } else if (event.key === 'ArrowUp') {
                  event.preventDefault()
                  setActive((index) => Math.max(index - 1, 0))
                } else if (event.key === 'Enter') {
                  event.preventDefault()
                  go(results[active])
                }
              }}
              placeholder="Search pages, headings and calls"
              aria-label="Search"
              aria-controls="search-results"
              aria-activedescendant={results[active] ? `search-result-${active}` : undefined}
              className="h-12 flex-1 bg-transparent text-[0.95rem] outline-none placeholder:text-muted"
            />
            <kbd className="rounded border border-edge px-1.5 font-mono text-[0.65rem] text-muted">Esc</kbd>
          </div>
          <ul id="search-results" ref={list} role="listbox" className="max-h-[60vh] overflow-y-auto p-1.5">
            {query && results.length === 0 ? (
              <li className="px-3 py-6 text-center text-sm text-muted">Nothing matches “{query}”.</li>
            ) : null}
            {results.map((entry, index) => {
              const Icon = ICONS[entry.kind]
              return (
                <li
                  key={entry.href}
                  id={`search-result-${index}`}
                  data-index={index}
                  role="option"
                  aria-selected={index === active}
                  onMouseMove={() => setActive(index)}
                  onClick={() => go(entry)}
                  className={cn(
                    'flex cursor-pointer items-center gap-3 rounded-[calc(var(--radius)-2px)] px-3 py-2',
                    index === active ? 'bg-accent-soft text-ink' : 'text-muted',
                  )}
                >
                  <Icon className="size-4 shrink-0 opacity-70" aria-hidden />
                  <span className="min-w-0 flex-1">
                    <span
                      className={cn('block truncate text-sm text-ink', entry.kind === 'call' && 'font-mono text-[0.82rem]')}
                    >
                      {entry.title}
                    </span>
                    <span className="block truncate text-[0.75rem] text-muted">{entry.context}</span>
                  </span>
                  {index === active ? <CornerDownLeft className="size-3.5 opacity-60" aria-hidden /> : null}
                </li>
              )
            })}
          </ul>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

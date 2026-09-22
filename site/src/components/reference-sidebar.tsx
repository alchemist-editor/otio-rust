'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useSyncExternalStore } from 'react'
import { cn } from '@/lib/cn'
import type { ReferenceNode, ReferenceSection } from '@/lib/reference-nav'

function useLocationHash(): string {
  return useSyncExternalStore(
    (onStoreChange) => {
      window.addEventListener('hashchange', onStoreChange)
      return () => window.removeEventListener('hashchange', onStoreChange)
    },
    () => window.location.hash,
    () => '',
  )
}

function pagePath(pathname: string | null): string {
  return pathname?.replace(/\/$/, '') || '/'
}

function NodeList({
  nodes,
  pathname,
  hash,
  depth,
}: {
  nodes: readonly ReferenceNode[]
  pathname: string
  hash: string
  depth: number
}) {
  return (
    <ul className={cn('space-y-0.5', depth === 0 ? 'border-l border-edge' : 'ml-2.5 border-l border-edge')}>
      {nodes.map((node) => {
        const active = node.href !== undefined && pathname === node.href
        return (
          <li key={node.href ?? node.title}>
            {node.href ? (
              <Link
                href={node.href}
                aria-current={active ? 'page' : undefined}
                title={node.title}
                className={cn(
                  '-ml-px block truncate border-l py-1 pl-2.5 text-[0.8rem] transition-colors',
                  active
                    ? 'border-accent bg-accent-soft font-medium text-ink'
                    : 'border-transparent text-muted hover:border-edge hover:text-ink',
                )}
              >
                {node.title}
              </Link>
            ) : (
              <span
                title={node.title}
                className="block truncate py-1 pl-2.5 text-[0.72rem] font-medium text-muted"
              >
                {node.title}
              </span>
            )}
            {node.children.length > 0 ? (
              <NodeList nodes={node.children} pathname={pathname} hash={hash} depth={depth + 1} />
            ) : null}
            {active && node.calls && node.calls.length > 0 ? (
              <ul className="ml-2.5 space-y-0.5 border-l border-edge py-0.5">
                {node.calls.map((call) => {
                  const callHash = call.href.slice(call.href.indexOf('#'))
                  const callActive = hash === callHash
                  return (
                    <li key={call.href}>
                      <Link
                        href={call.href}
                        title={call.title}
                        onClick={() => {
                          // Same-page hash changes do not always emit hashchange.
                          window.setTimeout(() => window.dispatchEvent(new HashChangeEvent('hashchange')), 0)
                        }}
                        className={cn(
                          '-ml-px block truncate border-l py-0.5 pl-2.5 font-mono text-[0.72rem] transition-colors',
                          callActive
                            ? 'border-accent font-medium text-accent'
                            : 'border-transparent text-muted hover:text-ink',
                        )}
                      >
                        {call.title}
                      </Link>
                    </li>
                  )
                })}
              </ul>
            ) : null}
          </li>
        )
      })}
    </ul>
  )
}

/** Groups down the left of every reference page, nested by what they belong to. */
export function ReferenceSidebar({ sections }: { sections: readonly ReferenceSection[] }) {
  const pathname = pagePath(usePathname())
  const hash = useLocationHash()
  const overview = pathname === '/reference'

  return (
    <nav aria-label="Reference" className="space-y-6 text-sm">
      <ul className="border-l border-edge">
        <li>
          <Link
            href="/reference"
            aria-current={overview ? 'page' : undefined}
            className={cn(
              '-ml-px block border-l py-1 pl-2.5 text-[0.8rem] transition-colors',
              overview
                ? 'border-accent bg-accent-soft font-medium text-ink'
                : 'border-transparent text-muted hover:border-edge hover:text-ink',
            )}
          >
            Overview
          </Link>
        </li>
      </ul>
      {sections.map((section) => (
        <div key={section.title}>
          <h2 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-muted">
            {section.title}
          </h2>
          <NodeList nodes={section.items} pathname={pathname} hash={hash} depth={0} />
        </div>
      ))}
    </nav>
  )
}

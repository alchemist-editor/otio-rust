'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { cn } from '@/lib/cn'

export interface SidebarLink {
  readonly href: string
  readonly title: string
}

export interface SidebarSection {
  readonly section: string
  readonly links: readonly SidebarLink[]
}

/** The list of pages down the left of every documentation page. */
export function DocSidebar({ sections }: { sections: readonly SidebarSection[] }) {
  const pathname = usePathname()?.replace(/\/$/, '') || '/'

  return (
    <nav aria-label="Documentation" className="space-y-7 text-sm">
      {sections.map((group) => (
        <div key={group.section}>
          <h2 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-muted">
            {group.section}
          </h2>
          <ul className="space-y-0.5 border-l border-edge">
            {group.links.map((link) => {
              const active = pathname === link.href
              return (
                <li key={link.href}>
                  <Link
                    href={link.href}
                    aria-current={active ? 'page' : undefined}
                    className={cn(
                      '-ml-px block border-l py-1.5 pl-3.5 transition-colors',
                      active
                        ? 'border-accent font-medium text-ink'
                        : 'border-transparent text-muted hover:border-edge hover:text-ink',
                    )}
                  >
                    {link.title}
                  </Link>
                </li>
              )
            })}
          </ul>
        </div>
      ))}
    </nav>
  )
}

import Link from 'next/link'
import { Github } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Search } from '@/components/search'
import { ThemeToggle } from '@/components/theme-toggle'
import { REPOSITORY_URL } from '@/lib/site'

const LINKS = [
  { href: '/docs', label: 'Docs' },
  { href: '/docs/guides/reading-and-writing', label: 'Guides' },
  { href: '/languages', label: 'Languages' },
  { href: '/reference', label: 'Reference' },
]

export function SiteHeader() {
  return (
    <header className="sticky top-0 z-40 border-b border-edge bg-canvas/85 backdrop-blur">
      <div className="mx-auto flex h-14 max-w-7xl items-center gap-3 px-4 sm:gap-6 sm:px-6">
        <Link href="/" className="flex items-center gap-2.5 font-semibold tracking-tight">
          <span className="grid size-6 place-items-center rounded-[0.3rem] bg-accent text-[0.7rem] font-bold text-canvas">
            O
          </span>
          <span className="hidden sm:inline">OpenTimelineIO</span>
          <span className="hidden text-muted lg:inline">for Rust</span>
        </Link>

        <nav className="-mx-1 flex min-w-0 items-center gap-0.5 overflow-x-auto text-sm sm:gap-1">
          {LINKS.map((link) => (
            <Link
              key={link.href}
              href={link.href}
              className="shrink-0 rounded-[var(--radius)] px-2 py-1.5 text-muted transition-colors hover:bg-surface hover:text-ink sm:px-3"
            >
              {link.label}
            </Link>
          ))}
        </nav>

        <div className="ml-auto flex shrink-0 items-center gap-1">
          <Search />
          <ThemeToggle />
          <Button
            variant="ghost"
            size="icon"
            className="text-muted hover:text-ink"
            render={
              <a href={REPOSITORY_URL} target="_blank" rel="noreferrer" aria-label="The repository" />
            }
          >
            <Github aria-hidden />
          </Button>
        </div>
      </div>
    </header>
  )
}

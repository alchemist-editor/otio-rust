import type { Metadata, Viewport } from 'next'
import { SiteHeader } from '@/components/site-header'
import { syntaxThemeCss } from '@/lib/highlight-theme'
import { REPOSITORY_URL, SITE_DESCRIPTION, SITE_NAME, SITE_URL, absoluteUrl } from '@/lib/site'
import './globals.css'

/**
 * What every page's `<head>` starts from. Each page adds its own canonical
 * URL, card image and Markdown alternate through `pageMetadata`.
 */
export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: { default: SITE_NAME, template: `%s · ${SITE_NAME}` },
  description: SITE_DESCRIPTION,
  applicationName: SITE_NAME,
  keywords: [
    'OpenTimelineIO',
    'OTIO',
    'Rust',
    'editorial',
    'timeline',
    'EDL',
    'AAF',
    'FCPXML',
    'ALE',
    'interchange',
    'SDK',
  ],
  authors: [{ name: 'Alchemist', url: REPOSITORY_URL }],
  creator: 'Alchemist',
  category: 'technology',
  formatDetection: { telephone: false, email: false, address: false },
  robots: { index: true, follow: true },
}

export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: '#fcfbf9' },
    { media: '(prefers-color-scheme: dark)', color: '#16181d' },
  ],
  colorScheme: 'light dark',
}

/**
 * The token colours, from the highlighter's own themes.
 *
 * The variables and the `.th-*` rules are generated together. Leaving the
 * base rules out defines the colours and never applies them, so every
 * language renders as plain text.
 */
const THEME_CSS = syntaxThemeCss

/**
 * Picks the theme before the first paint.
 *
 * The site is a static export, so the HTML cannot know what this reader
 * chose last time. Without this the page renders light and then flips, which
 * is worse than a line of inline script.
 */
const THEME_SCRIPT = `
try {
  var stored = localStorage.getItem('otio-docs-theme');
  var dark = stored ? stored === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
  document.documentElement.classList.toggle('dark', dark);
} catch (error) {}
`.trim()

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
        <style dangerouslySetInnerHTML={{ __html: THEME_CSS }} />
        {/* For agents: the index of every page as Markdown, per llmstxt.org. */}
        <link rel="alternate" type="text/plain" title="llms.txt" href={absoluteUrl('/llms.txt')} />
      </head>
      <body className="min-h-dvh">
        <SiteHeader />
        {children}
        <footer className="border-t border-edge py-8 text-sm text-muted">
          <div className="mx-auto flex max-w-7xl flex-wrap items-center gap-x-6 gap-y-2 px-4 sm:px-6">
            <span>A port of OpenTimelineIO, which is a project of the Academy Software Foundation.</span>
            <nav aria-label="Footer" className="flex flex-wrap gap-x-4 gap-y-1 sm:ml-auto">
              <a href="/llms.txt" className="hover:text-ink">
                llms.txt
              </a>
              <a href="/sitemap.xml" className="hover:text-ink">
                Sitemap
              </a>
              <a href={REPOSITORY_URL} target="_blank" rel="noreferrer" className="hover:text-ink">
                GitHub
              </a>
            </nav>
          </div>
        </footer>
      </body>
    </html>
  )
}

import type { Metadata } from 'next'
import { SiteHeader } from '@/components/site-header'
import { syntaxThemeCss } from '@/lib/highlight-theme'
import { SITE_DESCRIPTION, SITE_NAME } from '@/lib/site'
import './globals.css'

export const metadata: Metadata = {
  title: { default: SITE_NAME, template: `%s · ${SITE_NAME}` },
  description: SITE_DESCRIPTION,
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
      </head>
      <body className="min-h-dvh">
        <SiteHeader />
        {children}
        <footer className="border-t border-edge py-8 text-sm text-muted">
          <div className="mx-auto max-w-7xl px-4 sm:px-6">
            A port of OpenTimelineIO, which is a project of the Academy Software Foundation.
          </div>
        </footer>
      </body>
    </html>
  )
}

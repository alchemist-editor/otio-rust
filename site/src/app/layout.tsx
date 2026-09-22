import type { Metadata } from 'next'
import { createThemeCss } from '@tanstack/highlight/theme'
import { githubDarkTheme } from '@tanstack/highlight/themes/github-dark'
import { githubLightTheme } from '@tanstack/highlight/themes/github-light'
import { SiteHeader } from '@/components/site-header'
import { SITE_DESCRIPTION, SITE_NAME } from '@/lib/site'
import './globals.css'

export const metadata: Metadata = {
  title: { default: SITE_NAME, template: `%s · ${SITE_NAME}` },
  description: SITE_DESCRIPTION,
}

/**
 * The token colours, from the highlighter's own themes.
 *
 * Generating this rather than writing it keeps one thing true: the classes
 * the tokenizer emits and the classes the stylesheet names are the same set,
 * so a token class can never quietly go uncoloured.
 */
const THEME_CSS = createThemeCss({
  light: githubLightTheme,
  dark: githubDarkTheme,
  darkSelector: '.dark',
  includeBaseStyles: false,
})

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

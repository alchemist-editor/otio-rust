import { createThemeCss } from '@tanstack/highlight/theme'
import { githubDarkTheme } from '@tanstack/highlight/themes/github-dark'
import { githubLightTheme } from '@tanstack/highlight/themes/github-light'

/**
 * Token colours for every highlighted block on the site.
 *
 * TanStack Markdown owns a fence's outer element (`pre.tm-code`). Samples and
 * the C declarations use Highlight's own element (`pre.th-code`). The inner
 * tokens are `th-*` spans either way, and they only receive a colour when the
 * theme's base rules are included. Pointing those rules at both wrappers is
 * what the Markdown guide asks for when the renderer and the highlighter do
 * not share a container.
 *
 * https://tanstack.com/markdown/latest/docs/guides/syntax-highlighting
 */
export const syntaxThemeCss = `${createThemeCss({
  light: githubLightTheme,
  dark: githubDarkTheme,
  lightSelector: ':root, .markdown-renderer',
  darkSelector: '.dark, .dark .markdown-renderer',
  codeBlockSelector: 'pre.th-code, .markdown-renderer pre.tm-code',
  lineNumbersSelector: '.th-code--line-numbers, .markdown-renderer .tm-code--line-numbers',
})}

.markdown-renderer .th-line--highlighted {
  background: color-mix(in srgb, var(--th-token) 10%, transparent);
}`

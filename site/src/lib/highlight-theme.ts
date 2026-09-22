import { createThemeCss } from '@tanstack/highlight/theme'
import { githubDarkTheme } from '@tanstack/highlight/themes/github-dark'
import { githubLightTheme } from '@tanstack/highlight/themes/github-light'

/**
 * Line numbers for the wrapper TanStack Markdown owns.
 *
 * `createThemeCss` builds its line-number rules by *appending* to the
 * selector it is given — `${selector} code` and `${selector} .th-line::before`
 * — so handing it a comma-separated list attaches the suffix to the last
 * alternative only and leaves the first one as a bare rule. Two wrappers
 * spelled that way would put `width: 2.5em` on the `<pre>` itself and squash
 * every numbered block to the width of its gutter.
 *
 * So the generated half below gets one selector, and the other wrapper's
 * rules are written out here. `highlight-theme.test.ts` holds this shut.
 */
const MARKDOWN_LINE_NUMBERS = `
.markdown-renderer .tm-code--line-numbers code { counter-reset: th-line; }
.markdown-renderer .tm-code--line-numbers .th-line::before {
  display: inline-block;
  width: 2.5em;
  padding-right: 1em;
  color: var(--th-comment);
  content: attr(data-line);
  text-align: right;
  user-select: none;
}`

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
  lineNumbersSelector: '.th-code--line-numbers',
})}
${MARKDOWN_LINE_NUMBERS}

.markdown-renderer .th-line--highlighted {
  background: color-mix(in srgb, var(--th-token) 10%, transparent);
}`

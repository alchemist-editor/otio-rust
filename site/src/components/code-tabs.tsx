'use client'

import { Tabs } from '@base-ui/react/tabs'
import { cn } from '@/lib/cn'
import { CopyButton } from '@/components/copy-button'
import { selectLanguage, useSelectedLanguage } from '@/components/language-store'

export interface CodeTabsVariant {
  readonly languageId: string
  readonly label: string
  readonly code: string
  /** The code already highlighted, on the server. */
  readonly html: string
  readonly source: string
  /** Why this language has no sample yet, when it has none. */
  readonly unavailable?: string
}

/**
 * One sample, in every language it was written in, with the switcher along
 * the top.
 *
 * The highlighting is done during the build and arrives as HTML, so nothing
 * about a grammar reaches the browser: this component picks which of the
 * variants to show and nothing else. Picking one sets the choice for every
 * other sample on the site as well, which is what a reader means by choosing
 * a language.
 */
export function CodeTabs({
  variants,
  title,
}: {
  variants: readonly CodeTabsVariant[]
  title?: string
}) {
  const selected = useSelectedLanguage()
  // A sample need not exist in the language the reader last chose — a target
  // still being generated has no files yet. Falling back to the first it does
  // have is better than showing an empty frame.
  const active = variants.some((variant) => variant.languageId === selected)
    ? selected
    : (variants[0]?.languageId ?? '')
  const shown = variants.find((variant) => variant.languageId === active)

  if (variants.length === 0) return null

  return (
    <Tabs.Root
      value={active}
      onValueChange={(value) => selectLanguage(String(value))}
      className="code-surface markdown-renderer my-6 not-prose"
    >
      <div className="flex items-center gap-2 border-b border-edge bg-canvas/40 pr-1">
        <Tabs.List className="flex min-w-0 flex-1 overflow-x-auto">
          {variants.map((variant) => (
            <Tabs.Tab
              key={variant.languageId}
              value={variant.languageId}
              className={cn(
                'relative shrink-0 cursor-pointer px-3.5 py-2 text-[0.8rem] font-medium text-muted transition-colors',
                'hover:text-ink data-[selected]:text-ink',
                'after:absolute after:inset-x-2 after:bottom-0 after:h-px after:bg-transparent',
                'data-[selected]:after:bg-accent',
              )}
            >
              {variant.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>
        {title ? (
          <span className="hidden truncate pl-2 font-mono text-[0.72rem] text-muted sm:block">
            {title}
          </span>
        ) : null}
        {shown && !shown.unavailable ? (
          <CopyButton text={shown.code} label={`Copy the ${shown.label} sample`} />
        ) : null}
      </div>

      {variants.map((variant) => (
        // Every panel stays in the DOM: the page is a static export, so a
        // reader without JavaScript still gets the code, and a search engine
        // still indexes all of it.
        <Tabs.Panel key={variant.languageId} value={variant.languageId} keepMounted>
          {variant.unavailable ? (
            <p className="px-4 py-5 text-sm leading-relaxed text-muted">{variant.unavailable}</p>
          ) : (
            <div
              // Highlighted at build time by `@tanstack/highlight`, which
              // escapes the source as it tokenizes it.
              dangerouslySetInnerHTML={{ __html: variant.html }}
            />
          )}
        </Tabs.Panel>
      ))}
    </Tabs.Root>
  )
}

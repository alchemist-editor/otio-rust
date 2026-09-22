'use client'

import { Menu } from '@base-ui/react/menu'
import { Tabs } from '@base-ui/react/tabs'
import { ChevronDown, FileCode2 } from 'lucide-react'
import { cn } from '@/lib/cn'
import { CopyButton } from '@/components/copy-button'
import { LanguageIcon } from '@/components/language-icon'
import { selectLanguage, useSelectedLanguage } from '@/components/language-store'
import { splitFeaturedLanguages } from '@/lib/sdk-languages'
import { repositoryFile } from '@/lib/site'

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

const languageButtonClass =
  'inline-flex cursor-pointer items-center gap-1.5 rounded-[var(--radius)] px-2.5 py-1.5 text-[0.8rem] font-medium text-muted transition-colors hover:bg-surface hover:text-ink data-[selected]:bg-accent-soft data-[selected]:text-ink data-[selected]:ring-1 data-[selected]:ring-inset data-[selected]:ring-accent'

/**
 * One sample, in every language it was written in, with the switcher along
 * the top.
 *
 * Python, TypeScript and C++ stay in the row. The rest live in a menu, and
 * whichever is selected — tab or menu — is filled so it is obvious. The
 * highlighting is done during the build and arrives as HTML, so nothing
 * about a grammar reaches the browser. Picking a language sets the choice
 * for every other sample on the site as well.
 */
export function CodeTabs({ variants }: { variants: readonly CodeTabsVariant[] }) {
  const selected = useSelectedLanguage()
  // A sample need not exist in the language the reader last chose — a target
  // still being generated has no files yet. Falling back to the first it does
  // have is better than showing an empty frame.
  const active = variants.some((variant) => variant.languageId === selected)
    ? selected
    : (variants[0]?.languageId ?? '')
  const shown = variants.find((variant) => variant.languageId === active)
  const { featured, more } = splitFeaturedLanguages(variants)
  const moreSelected = more.some((variant) => variant.languageId === active)

  if (variants.length === 0) return null

  return (
    <Tabs.Root
      value={active}
      onValueChange={(value) => selectLanguage(String(value))}
      className="code-surface markdown-renderer my-6 not-prose"
    >
      <div className="flex flex-wrap items-center gap-1 border-b border-edge bg-canvas/40 px-2 py-1.5 pr-1">
        <Tabs.List className="flex flex-wrap items-center gap-1">
          {featured.map((variant) => (
            <Tabs.Tab key={variant.languageId} value={variant.languageId} className={languageButtonClass}>
              <LanguageIcon id={variant.languageId} />
              {variant.label}
            </Tabs.Tab>
          ))}
        </Tabs.List>
        {more.length > 0 ? (
          <MoreLanguages variants={more} selectedId={moreSelected ? active : undefined} />
        ) : null}
        {shown ? <SourceLink source={shown.source} /> : <span className="flex-1" />}
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
            <div className="flex gap-3 px-4 py-5 text-sm leading-relaxed text-muted">
              <FileCode2 className="mt-0.5 size-4 shrink-0 opacity-60" aria-hidden />
              <p>
                <span className="font-medium text-ink">Not in {variant.label} yet. </span>
                {withCode(variant.unavailable)}
              </p>
            </div>
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

/**
 * A note's `backticked` names, as code.
 *
 * These notes are plain text rather than Markdown — they are one sentence in
 * a file whose whole job is to be that sentence — but they name calls and
 * modules, and a name set in the body font reads as prose about a thing
 * rather than the thing itself. This is the one piece of Markdown they get.
 */
function withCode(note: string) {
  return note.split('`').map((part, index) =>
    index % 2 === 0 ? (
      part
    ) : (
      <code key={index} className="font-mono text-[0.92em] text-ink">
        {part}
      </code>
    ),
  )
}

/**
 * The file this tab is showing, linked to it.
 *
 * Samples are real files that a real compiler builds in CI, which is the
 * claim the whole arrangement rests on. Naming the file and linking to it is
 * how a reader can check that rather than take it on trust.
 */
function SourceLink({ source }: { source: string }) {
  const shown = source.split('/').slice(-2).join('/')
  return (
    <a
      href={repositoryFile(source)}
      target="_blank"
      rel="noreferrer"
      title={source}
      className="hidden min-w-0 flex-1 truncate pl-2 font-mono text-[0.72rem] text-muted transition-colors hover:text-ink sm:block"
    >
      {shown}
    </a>
  )
}

function MoreLanguages({
  variants,
  selectedId,
}: {
  variants: readonly CodeTabsVariant[]
  selectedId: string | undefined
}) {
  const selected = variants.find((variant) => variant.languageId === selectedId)

  return (
    <Menu.Root modal={false}>
      <Menu.Trigger
        className={cn(languageButtonClass, selected && 'bg-accent-soft text-ink ring-1 ring-inset ring-accent')}
        aria-label={selected ? `${selected.label}, more languages` : 'More languages'}
      >
        {selected ? <LanguageIcon id={selected.languageId} /> : null}
        {selected ? selected.label : 'More'}
        <ChevronDown className="size-3.5 opacity-70" aria-hidden />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner side="bottom" align="start" sideOffset={6} className="z-50">
          <Menu.Popup className="min-w-44 rounded-[var(--radius)] border border-edge bg-canvas p-1 shadow-lg outline-none">
            <Menu.RadioGroup
              value={selectedId ?? null}
              onValueChange={(value) => selectLanguage(String(value))}
            >
              {variants.map((variant) => (
                <Menu.RadioItem
                  key={variant.languageId}
                  value={variant.languageId}
                  className={cn(
                    'flex cursor-pointer items-center gap-2 rounded-[calc(var(--radius)-2px)] px-2 py-1.5 text-sm text-muted outline-none',
                    'data-[highlighted]:bg-surface data-[highlighted]:text-ink',
                    'data-[checked]:bg-accent-soft data-[checked]:font-medium data-[checked]:text-ink',
                  )}
                >
                  <LanguageIcon id={variant.languageId} />
                  {variant.label}
                </Menu.RadioItem>
              ))}
            </Menu.RadioGroup>
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  )
}

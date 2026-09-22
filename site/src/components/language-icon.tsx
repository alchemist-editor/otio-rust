import { cn } from '@/lib/cn'
import { LANGUAGE_MARKS } from '@/components/language-marks'

/**
 * The mark that sits in front of a language name.
 *
 * A coloured logo is what makes a tab scannable once most languages live in
 * a menu instead of a row of words.
 */
export function LanguageIcon({ id, className }: { id: string; className?: string }) {
  const mark = LANGUAGE_MARKS[id]
  return (
    <svg viewBox="0 0 24 24" className={cn('size-3.5 shrink-0', className)} aria-hidden>
      {mark ? (
        <path d={mark.path} fill={mark.color} />
      ) : (
        <path
          d="M3.5 6.5h5.2v2.1H6.2v6.8h2.5v2.1H3.5V6.5zm11.8 0H20.5v11h-2.7v-2.1h.6V8.6h-.6V6.5z"
          fill="#5A9FD4"
        />
      )}
    </svg>
  )
}

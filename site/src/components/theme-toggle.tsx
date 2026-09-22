'use client'

import { useSyncExternalStore } from 'react'
import { Moon, Sun } from 'lucide-react'
import { Button } from '@/components/ui/button'

/** The key the inline script in the layout reads before the first paint. */
export const THEME_KEY = 'otio-docs-theme'

/**
 * Whether the page is dark right now.
 *
 * The theme is a class on `<html>`, set before the first paint by a script in
 * the layout, so it is state that lives outside React — read it rather than
 * mirror it. The observer is what makes a change from anywhere, including
 * another component, reach this button.
 */
function subscribe(listener: () => void): () => void {
  const observer = new MutationObserver(listener)
  observer.observe(document.documentElement, { attributeFilter: ['class'] })
  return () => observer.disconnect()
}

function isDark(): boolean {
  return document.documentElement.classList.contains('dark')
}

export function ThemeToggle() {
  // The server cannot know, and says light; the inline script has already
  // made the page right by the time this renders on the client.
  const dark = useSyncExternalStore(subscribe, isDark, () => false)

  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={dark ? 'Switch to light' : 'Switch to dark'}
      className="text-muted hover:text-ink"
      onClick={() => {
        const next = !isDark()
        document.documentElement.classList.toggle('dark', next)
        try {
          window.localStorage.setItem(THEME_KEY, next ? 'dark' : 'light')
        } catch {
          // Not remembering it is not a reason not to switch.
        }
      }}
    >
      {dark ? <Sun aria-hidden /> : <Moon aria-hidden />}
    </Button>
  )
}

'use client'

import { useSyncExternalStore } from 'react'
import { DEFAULT_LANGUAGE, SDK_LANGUAGES } from '@/lib/sdk-languages'

/**
 * Which language the reader is reading in.
 *
 * Every code sample on the site switches together, and the choice survives
 * navigation and a reload — a reader who came for Swift should not have to
 * say so on each page. That makes it one value shared by many components
 * rather than state inside any of them, so it lives outside React and the
 * components subscribe.
 *
 * `useSyncExternalStore` is what makes that safe under a static export: the
 * server snapshot is the default, the client snapshot is what was stored, and
 * React is told they may differ rather than finding out during hydration.
 */
const STORAGE_KEY = 'otio-docs-language'

const listeners = new Set<() => void>()
let current: string | undefined

function read(): string {
  if (current !== undefined) return current
  let stored: string | null = null
  try {
    stored = window.localStorage.getItem(STORAGE_KEY)
  } catch {
    // Private browsing, or storage turned off. The default is fine.
  }
  current = SDK_LANGUAGES.some((language) => language.id === stored) ? stored! : DEFAULT_LANGUAGE
  return current
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  // Another tab, or another window of the same docs, chose a language.
  const onStorage = (event: StorageEvent) => {
    if (event.key !== STORAGE_KEY) return
    current = undefined
    listener()
  }
  window.addEventListener('storage', onStorage)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

/** The language every sample on the page is showing. */
export function useSelectedLanguage(): string {
  return useSyncExternalStore(subscribe, read, () => DEFAULT_LANGUAGE)
}

/** Chooses a language, for every sample at once. */
export function selectLanguage(id: string): void {
  if (current === id) return
  current = id
  try {
    window.localStorage.setItem(STORAGE_KEY, id)
  } catch {
    // Not being able to remember it is not a reason not to switch.
  }
  for (const listener of listeners) listener()
}

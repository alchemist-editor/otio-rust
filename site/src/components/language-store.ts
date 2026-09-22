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
 *
 * A `?lang=` in the URL wins over what was stored, so a link can be sent that
 * opens on Swift for somebody who last read in Python. Choosing a language
 * writes it back into the URL, which makes the address bar shareable at every
 * moment rather than only when somebody thought to add it.
 */
const STORAGE_KEY = 'otio-docs-language'
const URL_KEY = 'lang'

function known(id: string | null): id is string {
  return SDK_LANGUAGES.some((language) => language.id === id)
}

/** The language named in the address bar, if it names one at all. */
function fromUrl(): string | null {
  try {
    const asked = new URLSearchParams(window.location.search).get(URL_KEY)
    return known(asked) ? asked : null
  } catch {
    return null
  }
}

const listeners = new Set<() => void>()
let current: string | undefined

function read(): string {
  if (current !== undefined) return current
  const asked = fromUrl()
  if (asked !== null) {
    current = asked
    return current
  }
  let stored: string | null = null
  try {
    stored = window.localStorage.getItem(STORAGE_KEY)
  } catch {
    // Private browsing, or storage turned off. The default is fine.
  }
  current = known(stored) ? stored : DEFAULT_LANGUAGE
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
  try {
    // `replaceState` rather than `pushState`: switching language is not a
    // place a reader meant to navigate to, and filling the back button with
    // it would be a nuisance.
    const url = new URL(window.location.href)
    url.searchParams.set(URL_KEY, id)
    window.history.replaceState(null, '', url)
  } catch {
    // Nor is not being able to say so in the URL.
  }
  for (const listener of listeners) listener()
}

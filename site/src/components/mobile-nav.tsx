'use client'

import { usePathname } from 'next/navigation'
import { useEffect, useRef } from 'react'

/**
 * A sidebar folded into a disclosure, for screens too narrow for the real
 * one. It closes itself when a link in it is followed, since the layout it
 * lives in stays mounted across that navigation.
 */
export function MobileNav({ label, children }: { label: string; children: React.ReactNode }) {
  const pathname = usePathname()
  const details = useRef<HTMLDetailsElement>(null)

  useEffect(() => {
    if (details.current) details.current.open = false
  }, [pathname])

  return (
    <details ref={details} className="mt-6 rounded-[var(--radius)] border border-edge px-3 py-2 lg:hidden">
      <summary className="cursor-pointer text-sm font-medium">{label}</summary>
      <div className="max-h-[60vh] overflow-y-auto pb-1 pt-3">{children}</div>
    </details>
  )
}

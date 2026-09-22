import { ReferenceSidebar } from '@/components/reference-sidebar'
import { referenceSections } from '@/lib/reference-nav'

/**
 * The reference shell. The nested list on the left is how you move between
 * groups; the page on the right is the group you are in.
 */
export default function ReferenceLayout({ children }: { children: React.ReactNode }) {
  const sections = referenceSections()

  return (
    <div className="mx-auto flex max-w-7xl gap-8 px-4 sm:px-6">
      <aside className="sticky top-14 hidden h-[calc(100dvh-3.5rem)] w-64 shrink-0 overflow-y-auto py-10 lg:block">
        <ReferenceSidebar sections={sections} />
      </aside>
      <div className="min-w-0 flex-1">
        <details className="mt-6 rounded-[var(--radius)] border border-edge px-3 py-2 lg:hidden">
          <summary className="cursor-pointer text-sm font-medium">Sections</summary>
          <div className="max-h-[50vh] overflow-y-auto pt-3 pb-1">
            <ReferenceSidebar sections={sections} />
          </div>
        </details>
        {children}
      </div>
    </div>
  )
}

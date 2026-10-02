import { MobileNav } from '@/components/mobile-nav'
import { DocSidebar, type SidebarSection } from '@/components/doc-sidebar'
import { docSections } from '@/lib/content'

/**
 * The documentation shell: pages down the left, content in the middle, and
 * whatever the page puts on the right.
 */
export default function DocsLayout({ children }: { children: React.ReactNode }) {
  const sections: SidebarSection[] = docSections().map((group) => ({
    section: group.section,
    links: group.pages.map((page) => ({ href: page.href, title: page.title })),
  }))

  sections.push({
    section: 'Reference',
    links: [{ href: '/reference', title: 'The C ABI' }],
  })

  return (
    <div className="mx-auto flex max-w-7xl gap-10 px-4 sm:px-6">
      <aside className="sticky top-14 hidden h-[calc(100dvh-3.5rem)] w-56 shrink-0 overflow-y-auto py-10 lg:block">
        <DocSidebar sections={sections} />
      </aside>
      <div className="min-w-0 flex-1">
        <MobileNav label="All pages">
          <DocSidebar sections={sections} />
        </MobileNav>
        {children}
      </div>
    </div>
  )
}

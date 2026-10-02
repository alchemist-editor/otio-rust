import { notFound } from 'next/navigation'
import { DocView } from '@/components/doc-view'
import { docBySlug } from '@/lib/content'
import { docMetadata } from '@/lib/doc-routes'

export const metadata = docMetadata([])

export default function DocsIndex() {
  const page = docBySlug([])
  if (!page) notFound()
  return <DocView page={page} />
}

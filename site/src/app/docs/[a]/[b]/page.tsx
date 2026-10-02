import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { DocView } from '@/components/doc-view'
import { docBySlug } from '@/lib/content'
import { MovedPage } from '@/components/moved-page'
import { docMetadata, docParams, movedParams, movedTo, slugOf, type DocParams } from '@/lib/doc-routes'

export const dynamicParams = false

export function generateStaticParams() {
  return [...docParams(2), ...movedParams(2)]
}

export async function generateMetadata({ params }: { params: Promise<DocParams> }): Promise<Metadata> {
  return docMetadata(slugOf(await params))
}

export default async function DocPage({ params }: { params: Promise<DocParams> }) {
  const slug = slugOf(await params)
  const moved = movedTo(slug)
  if (moved) return <MovedPage to={moved} />
  const page = docBySlug(slug)
  if (!page) notFound()
  return <DocView page={page} />
}

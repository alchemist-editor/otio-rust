import type { Metadata } from 'next'
import { notFound } from 'next/navigation'
import { DocView } from '@/components/doc-view'
import { docBySlug } from '@/lib/content'
import { docMetadata, docParams, slugOf, type DocParams } from '@/lib/doc-routes'

export const dynamicParams = false

export function generateStaticParams() {
  return docParams(1)
}

export async function generateMetadata({ params }: { params: Promise<DocParams> }): Promise<Metadata> {
  return docMetadata(slugOf(await params))
}

export default async function DocPage({ params }: { params: Promise<DocParams> }) {
  const page = docBySlug(slugOf(await params))
  if (!page) notFound()
  return <DocView page={page} />
}

import { docBySlug } from '@/lib/content'
import { docParams, slugOf, type DocParams } from '@/lib/doc-routes'
import { docMarkdown, markdownResponse } from '@/lib/markdown-export'

export const dynamic = 'force-static'
export const dynamicParams = false

export function generateStaticParams() {
  return docParams(1)
}

export async function GET(_request: Request, { params }: { params: Promise<DocParams> }) {
  const page = docBySlug(slugOf(await params))
  if (!page) return new Response('Not found', { status: 404 })
  return markdownResponse(docMarkdown(page))
}

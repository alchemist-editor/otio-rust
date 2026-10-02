import { api, groupBySlug, groupSlug } from '@/lib/api'
import { markdownResponse, referenceGroupMarkdown } from '@/lib/markdown-export'

export const dynamic = 'force-static'
export const dynamicParams = false

export function generateStaticParams() {
  return api().groups.map((group) => ({ group: groupSlug(group.name) }))
}

export async function GET(_request: Request, { params }: { params: Promise<{ group: string }> }) {
  const group = groupBySlug((await params).group)
  if (!group) return new Response('Not found', { status: 404 })
  return markdownResponse(referenceGroupMarkdown(group))
}

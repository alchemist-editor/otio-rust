import { docBySlug } from '@/lib/content'
import { docMarkdown, markdownResponse } from '@/lib/markdown-export'

export const dynamic = 'force-static'

export function GET() {
  return markdownResponse(docMarkdown(docBySlug([])!))
}

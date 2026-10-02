import { markdownResponse, referenceIndexMarkdown } from '@/lib/markdown-export'

export const dynamic = 'force-static'

export function GET() {
  return markdownResponse(referenceIndexMarkdown())
}

import { llmsFull, textResponse } from '@/lib/markdown-export'

export const dynamic = 'force-static'

export function GET() {
  return textResponse(llmsFull())
}

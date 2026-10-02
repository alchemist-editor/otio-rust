import { llmsLanguage, textResponse } from '@/lib/markdown-export'
import { SDK_LANGUAGES } from '@/lib/sdk-languages'

export const dynamic = 'force-static'
export const dynamicParams = false

/** One file per language: `/llms/python.txt`, `/llms/go.txt`, and so on. */
export function generateStaticParams() {
  return SDK_LANGUAGES.map((language) => ({ file: `${language.id}.txt` }))
}

export async function GET(_request: Request, { params }: { params: Promise<{ file: string }> }) {
  const text = llmsLanguage((await params).file.replace(/\.txt$/, ''))
  if (!text) return new Response('Not found', { status: 404 })
  return textResponse(text)
}

/**
 * Structured data for search engines, as one `<script>` per page.
 *
 * `<` is escaped so that text from a page's title or summary can never close
 * the script element it sits in.
 */
export function JsonLd({ data }: { data: Record<string, unknown> }) {
  return (
    <script
      type="application/ld+json"
      dangerouslySetInnerHTML={{ __html: JSON.stringify(data).replace(/</g, '\\u003c') }}
    />
  )
}

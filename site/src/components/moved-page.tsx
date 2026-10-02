import Link from 'next/link'

/**
 * What an old docs URL serves on a host with no redirects: an immediate
 * refresh to the new one, and a link for a reader whose browser ignores it.
 * React hoists the `<meta>` into the head.
 */
export function MovedPage({ to }: { to: string }) {
  const href = `${to}/`
  return (
    <div className="py-24">
      <meta httpEquiv="refresh" content={`0; url=${href}`} />
      <h1 className="text-2xl font-semibold tracking-tight">This page moved</h1>
      <p className="mt-3 text-muted">
        It is now at{' '}
        <Link href={href} className="text-accent underline underline-offset-4">
          {to}
        </Link>
        .
      </p>
    </div>
  )
}

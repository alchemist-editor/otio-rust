import Link from 'next/link'
import { Button } from '@/components/ui/button'

export default function NotFound() {
  return (
    <div className="mx-auto max-w-2xl px-4 py-32 text-center">
      <h1 className="text-3xl font-semibold tracking-tight">No page here</h1>
      <p className="mt-3 text-muted">
        The link is wrong, or the page moved. The documentation index is a good place to start.
      </p>
      <div className="mt-8">
        <Button variant="accent" render={<Link href="/docs" />}>
          Go to the docs
        </Button>
      </div>
    </div>
  )
}

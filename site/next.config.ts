import type { NextConfig } from 'next'

const config: NextConfig = {
  // The site is entirely static: every page is rendered from Markdown and
  // from `sdk/api.json` at build time, and nothing it serves depends on a
  // request. `output: 'export'` makes that a build-time guarantee rather
  // than a thing we believe about ourselves.
  output: 'export',
  trailingSlash: true,
  images: { unoptimized: true },
  // The content and the API description live outside this directory, so
  // Next has to be told the tracing root is the repository.
  outputFileTracingRoot: new URL('..', import.meta.url).pathname,
  typedRoutes: false,
}

export default config

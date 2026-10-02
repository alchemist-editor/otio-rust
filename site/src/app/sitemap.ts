import type { MetadataRoute } from 'next'
import { sitePages } from '@/lib/site-pages'
import { absoluteUrl } from '@/lib/site'

export const dynamic = 'force-static'

export default function sitemap(): MetadataRoute.Sitemap {
  return sitePages().map((page) => ({
    url: absoluteUrl(page.path),
    changeFrequency: 'weekly',
    priority: page.path === '/' ? 1 : page.path.startsWith('/docs') ? 0.8 : 0.6,
  }))
}

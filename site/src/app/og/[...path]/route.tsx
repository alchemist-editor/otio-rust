import { ImageResponse } from 'next/og'
import { OG_IMAGE_SIZE, ogImagePath } from '@/lib/page-meta'
import { sitePages } from '@/lib/site-pages'
import { SITE_NAME } from '@/lib/site'

export const dynamic = 'force-static'
export const dynamicParams = false

/**
 * A card image for every page, drawn at build time from its title and
 * summary.
 *
 * A route of its own rather than Next's `opengraph-image` convention, because
 * that writes a file with no extension, and a static host serves a file with
 * no extension as a download rather than as a PNG.
 */
export function generateStaticParams() {
  return sitePages().map((page) => ({ path: ogImagePath(page.path).replace(/^\/og\//, '').split('/') }))
}

const INK = '#f4f2ee'
const MUTED = '#a8a49c'
const ACCENT = '#f0a83a'
const CANVAS = '#16181d'

/** A strip of clips on tracks, drawn in boxes, which is what OTIO is about. */
function Timeline() {
  const tracks = [
    [0.0, 0.18, 0.2, 0.36, 0.38, 0.62, 0.64, 1.0],
    [0.1, 0.3, 0.42, 0.78, 0.8, 0.94],
    [0.0, 0.5, 0.52, 0.88],
  ]
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14, width: '100%' }}>
      {tracks.map((edges, row) => (
        <div key={row} style={{ display: 'flex', position: 'relative', height: 26, width: '100%' }}>
          {Array.from({ length: edges.length / 2 }, (_, index) => {
            const start = edges[index * 2]!
            const end = edges[index * 2 + 1]!
            return (
              <div
                key={index}
                style={{
                  position: 'absolute',
                  left: `${start * 100}%`,
                  width: `${(end - start) * 100}%`,
                  height: 26,
                  borderRadius: 6,
                  background: row === 0 && index === 2 ? ACCENT : 'rgba(255,255,255,0.12)',
                  border: '1px solid rgba(255,255,255,0.08)',
                }}
              />
            )
          })}
        </div>
      ))}
      <div style={{ position: 'absolute', left: '58%', top: -12, width: 3, height: 128, background: ACCENT }} />
    </div>
  )
}

export async function GET(_request: Request, { params }: { params: Promise<{ path: string[] }> }) {
  const wanted = `/og/${(await params).path.join('/')}`
  const page = sitePages().find((candidate) => ogImagePath(candidate.path) === wanted)
  if (!page) return new Response('Not found', { status: 404 })

  const title = page.title
  const titleSize = title.length > 40 ? 60 : title.length > 22 ? 72 : 88
  const description = page.description.length > 150 ? `${page.description.slice(0, 147)}…` : page.description

  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          padding: '64px 72px',
          background: CANVAS,
          color: INK,
          fontFamily: 'Geist',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 16, fontSize: 30 }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: 44,
              height: 44,
              borderRadius: 9,
              background: ACCENT,
              color: CANVAS,
              fontSize: 26,
            }}
          >
            O
          </div>
          <div style={{ display: 'flex' }}>{SITE_NAME}</div>
          {page.eyebrow ? (
            <div style={{ display: 'flex', marginLeft: 'auto', color: ACCENT, fontSize: 24, letterSpacing: 2 }}>
              {page.eyebrow.toUpperCase()}
            </div>
          ) : null}
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 22 }}>
          <div style={{ display: 'flex', fontSize: titleSize, lineHeight: 1.05, letterSpacing: -2 }}>{title}</div>
          <div style={{ display: 'flex', fontSize: 30, lineHeight: 1.4, color: MUTED, maxWidth: 1000 }}>
            {description}
          </div>
        </div>

        <div style={{ display: 'flex', position: 'relative' }}>
          <Timeline />
        </div>
      </div>
    ),
    OG_IMAGE_SIZE,
  )
}

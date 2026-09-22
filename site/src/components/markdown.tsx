import type { ComponentType } from 'react'
import { renderMarkdownReact } from '@tanstack/markdown/react'
import type { MarkdownDocument } from '@tanstack/markdown'
import { markdownHighlighter } from '@/lib/highlighter'
import { docsExtensions, SAMPLE_TAG } from '@/lib/sample-extension'
import { sampleById } from '@/lib/samples'
import { CodeTabs } from '@/components/code-tabs'

/**
 * A parsed Markdown page, rendered.
 *
 * This is a server component, and that is what makes the multi-language
 * samples work: `::sample` resolves to files on disk here, during the build,
 * and what reaches the browser is the highlighted markup and a component
 * that switches between it.
 */
function SampleSlot(props: Record<string, unknown>) {
  const id = String(props['data-sample-id'] ?? '')
  const sample = sampleById(id)
  if (!sample) {
    // A page naming a sample that does not exist is a broken page, and
    // saying so where it would have been beats rendering nothing at all.
    return (
      <div className="my-6 rounded-[var(--radius)] border border-dashed border-edge p-4 text-sm text-muted">
        No sample named <code className="font-mono">{id}</code>.
      </div>
    )
  }
  return <CodeTabs variants={sample.variants} />
}

const COMPONENTS: Record<string, ComponentType<Record<string, unknown>>> = {
  [SAMPLE_TAG]: SampleSlot,
}

export function Markdown({ document }: { document: MarkdownDocument }) {
  return (
    <div className="prose markdown-renderer">
      {renderMarkdownReact(document, {
        highlighter: markdownHighlighter,
        headingAnchors: true,
        headingIds: true,
        allowHtml: false,
        extensions: docsExtensions(),
        components: COMPONENTS,
      })}
    </div>
  )
}

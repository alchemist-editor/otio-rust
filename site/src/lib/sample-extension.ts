import type { ComponentNode, MarkdownExtension } from '@tanstack/markdown'
import { calloutsExtension } from '@tanstack/markdown/extensions/callouts'
import { commentComponentsExtension } from '@tanstack/markdown/extensions/comment-components'
import { headingCollectionExtension } from '@tanstack/markdown/extensions/headings'
import { transformDocsComponent } from '@tanstack/markdown/extensions/docs'

/** The tag the React renderer maps to the language switcher. */
export const SAMPLE_TAG = 'otio-sample'

/**
 * Lets a page drop a multi-language sample into the prose.
 *
 * TanStack Markdown spells a component as an HTML comment, which keeps the
 * file valid Markdown that GitHub still renders:
 *
 * ```md
 * <!-- ::sample id="read-an-edl" -->
 * ```
 *
 * This only rewrites the node. Resolving `id` to the files under
 * `content/samples/` happens where the page is rendered, because that is
 * where the filesystem is.
 */
function transformSampleComponent(node: ComponentNode): ComponentNode {
  if (node.name !== 'sample') return transformDocsComponent(node)
  return {
    ...node,
    tagName: SAMPLE_TAG,
    properties: { ...(node.properties ?? {}), 'data-sample-id': node.attributes.id ?? '' },
    children: [],
  }
}

/**
 * The extensions every page is parsed with.
 *
 * This is `docsMarkdownExtensions()` with one substitution rather than an
 * addition: the docs bundle already registers a comment-component parser, and
 * the first parser to claim a line wins, so a second one would never see a
 * `::sample`. Callouts and heading collection come straight from the library.
 */
export function docsExtensions(): MarkdownExtension[] {
  return [
    calloutsExtension(),
    commentComponentsExtension({ transformComponent: transformSampleComponent }),
    headingCollectionExtension(),
  ]
}

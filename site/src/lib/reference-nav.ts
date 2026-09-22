import { api, groupSlug, type ApiGroup } from './api'

/**
 * One row in the reference sidebar.
 *
 * A node without `href` is a heading in the schema tree: an abstract type
 * the ABI does not give its own page, kept so its children stay nested under
 * the type they extend. `calls` are the functions on that page. The sidebar
 * shows them only while the page is open.
 */
export interface ReferenceNode {
  readonly title: string
  readonly href?: string
  readonly children: readonly ReferenceNode[]
  readonly calls?: readonly { title: string; href: string }[]
}

export interface ReferenceSection {
  readonly title: string
  readonly items: readonly ReferenceNode[]
}

/** Groups that are not a schema type, and the heading they sit under. */
const SECTIONS: readonly { title: string; groups: readonly string[] }[] = [
  { title: 'Library', groups: ['Library', 'Document', 'Format'] },
  { title: 'Time', groups: ['Rate', 'RationalTime', 'TimeRange', 'TimeTransform'] },
  { title: 'Reading and writing', groups: ['Adapter', 'Edit', 'Algorithm'] },
]

/**
 * The group named for a schema type, where the two names differ.
 *
 * `Node` is the page for `SerializableObject`. Everything else shares a name
 * with the schema it is about.
 */
const GROUP_FOR_SCHEMA: Record<string, string> = {
  SerializableObject: 'Node',
}

function callsFor(group: ApiGroup): { title: string; href: string }[] {
  const page = `/reference/${groupSlug(group.name)}`
  return group.functions.map((fn) => ({ title: fn.name, href: `${page}#${fn.symbol}` }))
}

function nodeForGroup(group: ApiGroup): ReferenceNode {
  return {
    title: group.name,
    href: `/reference/${groupSlug(group.name)}`,
    children: [],
    calls: callsFor(group),
  }
}

function collectTitles(nodes: readonly ReferenceNode[]): string[] {
  return nodes.flatMap((node) => [node.title, ...collectTitles(node.children)])
}

/**
 * The reference sidebar: a few flat sections, then the schema tree nested
 * the way the types extend each other.
 *
 * Every group in the ABI description appears once. A group this file does
 * not know about yet lands at the end of Library, so a new page cannot
 * exist with no way to reach it.
 */
export function referenceSections(): ReferenceSection[] {
  const groups = api().groups
  const byName = new Map(groups.map((group) => [group.name, group]))
  const placed = new Set<string>()

  const take = (name: string): ReferenceNode | undefined => {
    const group = byName.get(name)
    if (!group || placed.has(name)) return undefined
    placed.add(name)
    return nodeForGroup(group)
  }

  const sections: { title: string; items: ReferenceNode[] }[] = SECTIONS.map((section) => ({
    title: section.title,
    items: section.groups.flatMap((name) => {
      const node = take(name)
      return node ? [node] : []
    }),
  }))

  const schema = api().schema
  const childrenOf = (parent: string | null) => schema.filter((entry) => entry.parent === parent)

  const schemaNode = (name: string): ReferenceNode | undefined => {
    const groupName = GROUP_FOR_SCHEMA[name] ?? name
    const group = byName.get(groupName)
    const children: ReferenceNode[] = []
    if (name === 'SerializableObjectWithMetadata') {
      const metadata = take('Metadata')
      if (metadata) children.push(metadata)
    }
    for (const child of childrenOf(name)) {
      const node = schemaNode(child.name)
      if (node) children.push(node)
    }
    const ownsGroup = group !== undefined && !placed.has(group.name)
    if (!ownsGroup && children.length === 0) return undefined
    if (ownsGroup) placed.add(group.name)
    return {
      title: ownsGroup ? group.name : name,
      href: ownsGroup ? `/reference/${groupSlug(group.name)}` : undefined,
      children,
      calls: ownsGroup ? callsFor(group) : undefined,
    }
  }

  sections.push({
    title: 'Schema',
    items: childrenOf(null).flatMap((entry) => {
      const node = schemaNode(entry.name)
      return node ? [node] : []
    }),
  })

  const leftover = groups.filter((group) => !placed.has(group.name))
  if (leftover.length > 0) {
    const library = sections.find((section) => section.title === 'Library')
    const extra = leftover.map((group) => {
      placed.add(group.name)
      return nodeForGroup(group)
    })
    if (library) library.items.push(...extra)
    else sections.unshift({ title: 'Library', items: extra })
  }

  const seen = sections.flatMap((section) => collectTitles(section.items))
  const groupNames = new Set(groups.map((group) => group.name))
  const missing = groups.filter((group) => !seen.includes(group.name))
  const duplicated = seen.filter((title) => groupNames.has(title)).filter((title, index, all) => all.indexOf(title) !== index)
  if (missing.length > 0 || duplicated.length > 0) {
    throw new Error(
      `reference nav is wrong: missing ${missing.map((group) => group.name).join(', ') || 'none'}; duplicated ${duplicated.join(', ') || 'none'}`,
    )
  }
  return sections
}

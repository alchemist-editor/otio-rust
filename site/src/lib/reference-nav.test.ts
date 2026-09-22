import { describe, expect, it } from 'vitest'
import { api } from './api'
import { referenceSections, type ReferenceNode } from './reference-nav'
import { FEATURED_LANGUAGES, splitFeaturedLanguages } from './sdk-languages'

function find(nodes: readonly ReferenceNode[], title: string): ReferenceNode | undefined {
  for (const node of nodes) {
    if (node.title === title) return node
    const nested = find(node.children, title)
    if (nested) return nested
  }
  return undefined
}

describe('reference sidebar', () => {
  const sections = referenceSections()

  it('names every ABI group once', () => {
    const titles = sections.flatMap((section) => {
      const walk = (nodes: readonly ReferenceNode[]): string[] =>
        nodes.flatMap((node) => [node.title, ...walk(node.children)])
      return walk(section.items)
    })
    const groups = api().groups.map((group) => group.name)
    expect(titles.filter((title) => groups.includes(title)).sort()).toEqual([...groups].sort())
    expect(new Set(titles.filter((title) => groups.includes(title))).size).toBe(groups.length)
  })

  it('nests a schema type under the type it extends', () => {
    const schema = sections.find((section) => section.title === 'Schema')
    expect(schema).toBeTruthy()
    const item = find(schema!.items, 'Item')
    expect(item?.children.map((child) => child.title)).toEqual(['Composition', 'Clip', 'Gap'])
    const composition = find(schema!.items, 'Composition')
    expect(composition?.children.map((child) => child.title)).toEqual(['Track', 'Stack'])
    const composable = find(schema!.items, 'Composable')
    expect(composable?.children.map((child) => child.title)).toEqual(['Item', 'Transition'])
    expect(find(schema!.items, 'Clip')?.href).toBe('/reference/clip')
    expect(find(schema!.items, 'Node')?.children.some((child) => child.title === 'SerializableObjectWithMetadata')).toBe(
      true,
    )
  })

  it('keeps time and the file adapters out of the schema tree', () => {
    expect(sections.find((section) => section.title === 'Time')?.items.map((item) => item.title)).toEqual([
      'Rate',
      'RationalTime',
      'TimeRange',
      'TimeTransform',
    ])
    expect(
      sections.find((section) => section.title === 'Reading and writing')?.items.map((item) => item.title),
    ).toEqual(['Adapter', 'Edit', 'Algorithm'])
  })
})

describe('featured languages', () => {
  it('keeps Python, TypeScript and C++ in front of the rest', () => {
    const variants = [
      { languageId: 'rust' },
      { languageId: 'python' },
      { languageId: 'typescript' },
      { languageId: 'go' },
      { languageId: 'cpp' },
      { languageId: 'zig' },
    ]
    expect(splitFeaturedLanguages(variants)).toEqual({
      featured: [{ languageId: 'python' }, { languageId: 'typescript' }, { languageId: 'cpp' }],
      more: [{ languageId: 'rust' }, { languageId: 'go' }, { languageId: 'zig' }],
    })
    expect(FEATURED_LANGUAGES).toEqual(['python', 'typescript', 'cpp'])
  })
})

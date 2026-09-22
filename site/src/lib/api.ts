import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { REPO_ROOT } from './paths'
import { highlighter } from './highlighter'

/**
 * The C ABI, as the library describes itself.
 *
 * `sdk/api.json` is written by `otio-sdk-model` out of the Rust source of
 * `crates/otio-capi`, and it is what every SDK is generated from. The
 * reference section of this site is generated from the same file for the
 * same reason the SDKs are: a page that was typed out by hand is a page that
 * goes quietly wrong the next time the ABI changes.
 *
 * Nothing here interprets the description — it renders it. Where a page
 * shows a C declaration it is lifted verbatim from the committed header
 * rather than reassembled from parts, so the site cannot invent a spelling
 * the compiler would reject.
 */

export interface ApiDocs {
  readonly summary: string
  readonly body: readonly string[]
  readonly references: readonly string[]
}

export interface ApiParam {
  readonly name: string
  readonly role: string
  readonly type: string
  readonly optional: boolean
}

export interface ApiOutput {
  readonly name: string
  readonly type: string
  readonly owned_buffer: boolean
}

export interface ApiFunction {
  /** The C symbol, as exported. */
  readonly symbol: string
  /** The same call with its group prefix removed, which is what SDKs name it. */
  readonly name: string
  readonly role: string
  readonly optional: boolean
  readonly result: string
  readonly params: readonly ApiParam[]
  readonly outputs: readonly ApiOutput[]
  readonly docs: ApiDocs
}

export interface ApiGroup {
  readonly name: string
  readonly docs: ApiDocs
  readonly prefixes: readonly string[]
  readonly receiver: string
  readonly view: boolean
  readonly functions: readonly ApiFunction[]
}

export interface ApiEnumVariant {
  readonly name: string
  readonly c_name: string
  readonly value: number
  readonly docs: ApiDocs
}

export interface ApiEnum {
  readonly name: string
  readonly docs: ApiDocs
  readonly variants: readonly ApiEnumVariant[]
}

export interface ApiSchema {
  readonly name: string
  readonly kind: string
  readonly parent: string | null
  readonly concrete: boolean
  readonly docs: ApiDocs
}

export interface ApiDescription {
  readonly version: string
  readonly enums: readonly ApiEnum[]
  readonly structs: readonly { name: string; docs: ApiDocs; fields?: readonly unknown[] }[]
  readonly schema: readonly ApiSchema[]
  readonly groups: readonly ApiGroup[]
}

let description: ApiDescription | undefined

export function api(): ApiDescription {
  if (!description) {
    description = JSON.parse(readFileSync(join(REPO_ROOT, 'sdk/api.json'), 'utf8')) as ApiDescription
  }
  return description
}

/** A URL-safe id for a group, so `RationalTime` becomes `rational-time`. */
export function groupSlug(name: string): string {
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .toLowerCase()
}

export function groupBySlug(slug: string): ApiGroup | undefined {
  return api().groups.find((group) => groupSlug(group.name) === slug)
}

let header: string | undefined

/**
 * The declaration of `symbol` as it stands in the committed header, or
 * undefined when the header has no such symbol.
 */
export function declarationOf(symbol: string): string | undefined {
  if (header === undefined) {
    header = readFileSync(join(REPO_ROOT, 'crates/otio-capi/include/otio.h'), 'utf8')
  }
  // A declaration starts at the line that names the symbol followed by `(`,
  // and runs to the first semicolon. The return type is on that same line.
  const pattern = new RegExp(`^[^\\n/*][^\\n]*\\b${symbol}\\s*\\([\\s\\S]*?;`, 'm')
  const found = header.match(pattern)
  return found?.[0]
}

/** That declaration, highlighted, ready to drop into a page. */
export function declarationHtml(symbol: string): string | undefined {
  const declaration = declarationOf(symbol)
  if (!declaration) return undefined
  return highlighter.highlightToHtml(declaration, { lang: 'c' })
}

/** Every schema, with its children, as a tree for the data-model page. */
export function schemaTree(): Array<{ schema: ApiSchema; depth: number }> {
  const bySchema = api().schema
  const childrenOf = (parent: string | null) => bySchema.filter((entry) => entry.parent === parent)
  const rows: Array<{ schema: ApiSchema; depth: number }> = []
  const walk = (parent: string | null, depth: number) => {
    for (const schema of childrenOf(parent)) {
      rows.push({ schema, depth })
      walk(schema.name, depth + 1)
    }
  }
  walk(null, 0)
  return rows
}

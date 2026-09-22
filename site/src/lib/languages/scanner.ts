import type { TokenRange } from '@tanstack/highlight/core'

/**
 * A single-pass scanner for the curly-brace languages this site documents.
 *
 * `@tanstack/highlight` ships thirty grammars and none of them is Rust,
 * Swift, Zig, C, C# or Objective-C — which is most of what an OpenTimelineIO
 * SDK is written in. Its `defineLanguage` contract is small enough that the
 * missing ones are ours to write: a tokenizer is a function from source to
 * non-overlapping `TokenRange`s.
 *
 * Layering regular expressions by priority is how that is usually attempted
 * and it is how it goes wrong: a keyword inside a string literal, an
 * apostrophe inside a comment, a `//` inside a URL. So this walks the source
 * once, left to right, and every byte is claimed by exactly one rule. Ranges
 * come out sorted and disjoint by construction, which is what the core asks
 * for.
 *
 * The bar is useful highlighting for valid documentation code, not grammar
 * conformance — the same bar the library sets for its own definitions.
 */

/** How a language opens and closes one flavour of string. */
export interface StringRule {
  /** The opening delimiter. */
  readonly open: string
  /** The closing delimiter, usually the same. */
  readonly close: string
  /** Whether a backslash escapes the next character. */
  readonly escapes?: boolean
  /** Whether the literal may cross a line break. */
  readonly multiline?: boolean
  /**
   * A prefix that may sit immediately before `open` and belongs to the
   * literal: `b"…"` in Rust and Zig, `@"…"` in C#, `u8"…"` in C++.
   */
  readonly prefixes?: readonly string[]
}

/** Everything the scanner needs to know about one language. */
export interface LanguageSpec {
  /** Sequences that comment out the rest of the line. */
  readonly lineComments: readonly string[]
  /** An opening and closing block-comment delimiter, if the language has one. */
  readonly blockComment?: readonly [open: string, close: string]
  /** Whether block comments nest, as Rust's do. */
  readonly nestedBlockComments?: boolean
  /** The string flavours, tried in the order given. */
  readonly strings: readonly StringRule[]
  /**
   * Whether `'x'` is a character literal. Languages that also use a bare
   * apostrophe for something else — Rust lifetimes — are handled by the
   * scanner, which only takes the literal when it closes on the same line
   * within a few characters.
   */
  readonly charLiteral?: boolean
  /** Words that colour as keywords. */
  readonly keywords: ReadonlySet<string>
  /** Words that name a built-in type. */
  readonly types: ReadonlySet<string>
  /** Words that are a literal value of their own: `true`, `null`, `nil`. */
  readonly literals: ReadonlySet<string>
  /**
   * A line whose first non-blank character is this is a preprocessor line,
   * and colours as `meta` up to its end. `#` for C, C++ and Objective-C.
   */
  readonly preprocessor?: string
  /**
   * Characters that introduce an attribute or annotation — `#` for Rust's
   * `#[derive]`, `@` for Swift's `@main` and Objective-C's `@interface`.
   * The character and the word after it colour as `meta`.
   */
  readonly annotationSigils?: readonly string[]
  /** Extra characters, beyond letters, digits and `_`, that identifiers may contain. */
  readonly identifierExtra?: readonly string[]
  /**
   * Whether an identifier beginning with a capital letter colours as a type.
   * True everywhere the SDKs follow a PascalCase type convention.
   */
  readonly capitalisedIsType?: boolean
}

const OPERATORS = new Set('+-*/%=<>!&|^~?:'.split(''))

function isDigit(ch: string): boolean {
  return ch >= '0' && ch <= '9'
}

function isIdentifierStart(ch: string, spec: LanguageSpec): boolean {
  return (
    (ch >= 'a' && ch <= 'z') ||
    (ch >= 'A' && ch <= 'Z') ||
    ch === '_' ||
    (spec.identifierExtra?.includes(ch) ?? false)
  )
}

function isIdentifierPart(ch: string, spec: LanguageSpec): boolean {
  return isIdentifierStart(ch, spec) || isDigit(ch)
}

/** The offset just past the run of blanks starting at `from`, on this line. */
function skipInlineBlanks(code: string, from: number): number {
  let at = from
  while (at < code.length && (code[at] === ' ' || code[at] === '\t')) at += 1
  return at
}

/** Whether only blanks separate `from` from the start of its line. */
function atLineStart(code: string, from: number): boolean {
  for (let at = from - 1; at >= 0; at -= 1) {
    const ch = code[at]
    if (ch === '\n') return true
    if (ch !== ' ' && ch !== '\t') return false
  }
  return true
}

/**
 * Builds a tokenizer for `spec`.
 *
 * The returned function is the `tokenize` a `LanguageDefinition` wants. It
 * never delegates, so it takes no `TokenizerContext`.
 */
export function scanner(spec: LanguageSpec): (code: string) => Array<TokenRange> {
  return function tokenize(code: string): Array<TokenRange> {
    const ranges: Array<TokenRange> = []
    const push = (start: number, end: number, className: TokenRange['className']) => {
      if (end > start) ranges.push({ start, end, className })
    }

    let at = 0
    while (at < code.length) {
      const ch = code[at]!

      // A preprocessor directive owns its whole line, comments and all, so
      // it is tested before anything else that could claim part of it.
      if (spec.preprocessor && ch === spec.preprocessor && atLineStart(code, at)) {
        let end = at
        while (end < code.length && code[end] !== '\n') end += 1
        push(at, end, 'meta')
        at = end
        continue
      }

      // Line comments.
      const lineComment = spec.lineComments.find((token) => code.startsWith(token, at))
      if (lineComment) {
        let end = at
        while (end < code.length && code[end] !== '\n') end += 1
        push(at, end, 'comment')
        at = end
        continue
      }

      // Block comments, nested where the language nests them.
      if (spec.blockComment && code.startsWith(spec.blockComment[0], at)) {
        const [open, close] = spec.blockComment
        let end = at + open.length
        let depth = 1
        while (end < code.length && depth > 0) {
          if (spec.nestedBlockComments && code.startsWith(open, end)) {
            depth += 1
            end += open.length
          } else if (code.startsWith(close, end)) {
            depth -= 1
            end += close.length
          } else {
            end += 1
          }
        }
        push(at, Math.min(end, code.length), 'comment')
        at = Math.min(end, code.length)
        continue
      }

      // Strings, including any prefix that belongs to the literal.
      const string = matchString(code, at, spec)
      if (string !== undefined) {
        push(at, string, 'string')
        at = string
        continue
      }

      // Character literals, where a bare apostrophe is not something else.
      if (spec.charLiteral && ch === "'") {
        const end = matchCharLiteral(code, at)
        if (end !== undefined) {
          push(at, end, 'string')
          at = end
          continue
        }
      }

      // Attributes and annotations: the sigil and the word after it.
      if (spec.annotationSigils?.includes(ch)) {
        let end = at + 1
        if (code[end] === '[') end += 1
        const wordStart = end
        while (end < code.length && isIdentifierPart(code[end]!, spec)) end += 1
        if (end > wordStart) {
          push(at, end, 'meta')
          at = end
          continue
        }
      }

      // Numbers, including hex, binary, floats, exponents and separators.
      if (isDigit(ch) || (ch === '.' && isDigit(code[at + 1] ?? ''))) {
        let end = at
        while (end < code.length && /[0-9a-fA-FxXbBoO._]/.test(code[end]!)) {
          // `1..2` is a range, not a number with two dots.
          if (code[end] === '.' && code[end + 1] === '.') break
          end += 1
        }
        if (code[end] === 'e' || code[end] === 'E') {
          const after = code[end + 1]
          if (after === '+' || after === '-' || isDigit(after ?? '')) {
            end += 2
            while (end < code.length && isDigit(code[end]!)) end += 1
          }
        }
        // A trailing type suffix — Rust's `24u32`, C's `1024UL` — is part of
        // the number as a reader sees it.
        while (end < code.length && /[a-zA-Z_0-9]/.test(code[end]!)) end += 1
        push(at, end, 'number')
        at = end
        continue
      }

      // Words: keywords, built-in types, literals, calls, and everything else.
      if (isIdentifierStart(ch, spec)) {
        let end = at
        while (end < code.length && isIdentifierPart(code[end]!, spec)) end += 1
        const word = code.slice(at, end)

        if (spec.keywords.has(word)) push(at, end, 'keyword')
        else if (spec.literals.has(word)) push(at, end, 'literal')
        else if (spec.types.has(word)) push(at, end, 'type')
        else if (code[skipInlineBlanks(code, end)] === '(') push(at, end, 'function')
        else if (spec.capitalisedIsType && /^[A-Z]/.test(word)) push(at, end, 'type')

        at = end
        continue
      }

      // Operators, one run at a time. Brackets, commas and semicolons are
      // left unclassified: colouring them adds noise and no information.
      if (OPERATORS.has(ch)) {
        let end = at
        while (end < code.length && OPERATORS.has(code[end]!)) end += 1
        push(at, end, 'operator')
        at = end
        continue
      }

      at += 1
    }

    return ranges
  }
}

/** The offset just past a string literal starting at `at`, or undefined. */
function matchString(code: string, at: number, spec: LanguageSpec): number | undefined {
  for (const rule of spec.strings) {
    let bodyStart = at
    if (rule.prefixes) {
      const prefix = rule.prefixes.find(
        (candidate) => code.startsWith(candidate, at) && code.startsWith(rule.open, at + candidate.length),
      )
      if (prefix !== undefined) bodyStart = at + prefix.length
      else if (!code.startsWith(rule.open, at)) continue
    } else if (!code.startsWith(rule.open, at)) {
      continue
    }

    let end = bodyStart + rule.open.length
    while (end < code.length) {
      const ch = code[end]!
      if (rule.escapes && ch === '\\') {
        end += 2
        continue
      }
      if (!rule.multiline && ch === '\n') return end
      if (code.startsWith(rule.close, end)) return end + rule.close.length
      end += 1
    }
    return code.length
  }
  return undefined
}

/**
 * The offset just past a character literal starting at `at`, or undefined
 * when the apostrophe is something else — a Rust lifetime, a Zig label.
 */
function matchCharLiteral(code: string, at: number): number | undefined {
  let end = at + 1
  if (code[end] === '\\') end += 2
  else end += 1
  // A literal is one character, or an escape; anything longer is not one.
  if (code[end] === "'") return end + 1
  return undefined
}

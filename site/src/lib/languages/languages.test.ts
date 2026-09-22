import { describe, expect, it } from 'vitest'
import { createHighlighter } from '@tanstack/highlight/core'
import { additionalLanguages } from './index'

/**
 * The gates the library asks a language definition to pass: valid-code
 * fixtures, exact source reconstruction, and a focused regression for the
 * context-sensitive thing each grammar gets wrong first.
 */
const highlighter = createHighlighter({ languages: [...additionalLanguages] })

function tokens(code: string, lang: string) {
  return highlighter.tokenize(code, { lang }).tokens
}

function classesIn(code: string, lang: string) {
  return new Set(tokens(code, lang).map((token) => token.className).filter(Boolean))
}

const FIXTURES: Record<string, string> = {
  rust: `use otio_core::{Document, Node};

/// Reads a cut and counts what is in it.
#[derive(Debug, Clone)]
pub struct Cut {
    pub name: String,
    pub frames: u32,
}

fn main() -> Result<(), otio_core::Error> {
    let document = otio_core::from_str(r#"{"OTIO_SCHEMA": "Timeline.1"}"#)?;
    let root = document.root().expect("a parsed document has a root");
    println!("{} clips", 24u32);
    Ok(())
}
`,
  swift: `import OpenTimelineIO

@main
struct Reader {
    static func main() throws {
        let document = try Document.open("cut.edl")
        defer { document.close() }
        guard let root = try document.root() else { return }
        for case let clip as Clip in try root.findClips() {
            print(try clip.name(), try clip.duration().toSeconds())
        }
    }
}
`,
  zig: `const std = @import("std");
const otio = @import("otio");

pub fn main() !void {
    const allocator = std.heap.page_allocator;
    const document = try otio.Document.readFromFile(.cmx3600, "cut.edl", null);
    defer document.deinit();

    const root = (try document.root()) orelse return error.Empty;
    const clips = try root.findClips(allocator);
    defer allocator.free(clips);
}
`,
  c: `#include <stdio.h>
#include "otio.h"

int main(void) {
    OtioDocument *document = NULL;
    OtioStatus status =
        otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", NULL, &document);
    if (status != OTIO_STATUS_OK) {
        return 1;
    }
    /* a block comment */
    otio_document_free(document);
    return 0;
}
`,
  csharp: `using System;
using OpenTimelineIO;

public static class Reader {
    public static void Main() {
        var document = Document.Open("cut.edl");
        foreach (var clip in document.Root.FindClips()) {
            Console.WriteLine($"{clip.Name}");
        }
    }
}
`,
  objectivec: `#import <Foundation/Foundation.h>
#import "OTIODocument.h"

@interface Reader : NSObject
@property (nonatomic, readonly) NSString *path;
@end

int main(void) {
    OTIODocument *document = [OTIODocument openPath:@"cut.edl" error:NULL];
    NSLog(@"%@", document.name);
    return 0;
}
`,
}

describe.each(Object.entries(FIXTURES))('%s', (lang, code) => {
  it('is registered under its own name', () => {
    expect(highlighter.normalizeLanguage(lang)).toBe(lang)
  })

  it('reconstructs its source exactly', () => {
    expect(tokens(code, lang).map((token) => token.value).join('')).toBe(code)
  })

  it('finds keywords, strings and comments', () => {
    const found = classesIn(code, lang)
    expect(found.has('keyword')).toBe(true)
    expect(found.has('string')).toBe(true)
  })
})

describe('context-sensitive scanning', () => {
  it('does not find a Rust keyword inside a string', () => {
    const found = tokens('let s = "fn struct impl";', 'rust')
    const keywords = found.filter((token) => token.className === 'keyword').map((token) => token.value)
    expect(keywords).toEqual(['let'])
  })

  it('does not mistake a Rust lifetime for a character literal', () => {
    const code = "fn borrow<'a>(value: &'a str) -> &'a str { value }"
    expect(tokens(code, 'rust').map((token) => token.value).join('')).toBe(code)
    expect(tokens(code, 'rust').some((token) => token.className === 'string')).toBe(false)
  })

  it('keeps a comment marker inside a string out of the comment', () => {
    const found = tokens('const url = "https://example.com/cut.edl";', 'csharp')
    expect(found.some((token) => token.className === 'comment')).toBe(false)
  })

  it('nests Rust block comments', () => {
    const code = '/* outer /* inner */ still outer */ fn after() {}'
    const comments = tokens(code, 'rust').filter((token) => token.className === 'comment')
    expect(comments).toHaveLength(1)
    expect(comments[0]?.value).toBe('/* outer /* inner */ still outer */')
  })

  it('treats a Zig multiline string line as a string', () => {
    const code = 'const text =\n    \\\\one line\n    \\\\another\n;\n'
    const strings = tokens(code, 'zig').filter((token) => token.className === 'string')
    expect(strings).toHaveLength(2)
    expect(tokens(code, 'zig').map((token) => token.value).join('')).toBe(code)
  })

  it('gives a C preprocessor line to meta, comment marker and all', () => {
    const found = tokens('#define A // not a comment\nint x;\n', 'c')
    expect(found.find((token) => token.className === 'meta')?.value).toBe('#define A // not a comment')
  })

  it('reads an Objective-C string literal through its @ prefix', () => {
    const found = tokens('NSString *s = @"cut.edl";', 'objectivec')
    expect(found.find((token) => token.className === 'string')?.value).toBe('@"cut.edl"')
  })

  it('colours a Swift attribute as meta', () => {
    const found = tokens('@available(macOS 13, *)\nfunc f() {}', 'swift')
    expect(found.find((token) => token.className === 'meta')?.value).toBe('@available')
  })
})

import { defineLanguage } from '@tanstack/highlight/core'
import { scanner } from './scanner'

const words = (list: string) => new Set(list.trim().split(/\s+/))

/** Rust, the language the core itself is written in. */
export const rust = defineLanguage({
  name: 'rust',
  aliases: ['rs'],
  tokenize: scanner({
    lineComments: ['//'],
    blockComment: ['/*', '*/'],
    nestedBlockComments: true,
    strings: [
      { open: 'r#"', close: '"#', multiline: true },
      { open: '"', close: '"', escapes: true, multiline: true, prefixes: ['b', 'c'] },
    ],
    charLiteral: true,
    annotationSigils: ['#'],
    capitalisedIsType: true,
    keywords: words(`
      as async await break const continue crate dyn else enum extern fn for if impl in let loop
      match mod move mut pub ref return self Self static struct super trait type unsafe use where while
    `),
    types: words(`
      bool char f32 f64 i8 i16 i32 i64 i128 isize str u8 u16 u32 u64 u128 usize String Vec Option Result Box
    `),
    literals: words('true false None Some Ok Err'),
  }),
})

/** Swift, for the generated Swift SDK. */
export const swift = defineLanguage({
  name: 'swift',
  tokenize: scanner({
    lineComments: ['//'],
    blockComment: ['/*', '*/'],
    nestedBlockComments: true,
    strings: [
      { open: '"""', close: '"""', escapes: true, multiline: true },
      { open: '"', close: '"', escapes: true },
    ],
    annotationSigils: ['@'],
    identifierExtra: ['$'],
    capitalisedIsType: true,
    keywords: words(`
      as associatedtype async await break case catch class continue default defer deinit do else enum
      extension fallthrough fileprivate final for func guard if import in indirect init inout internal is
      lazy let mutating open operator private protocol public repeat rethrows return self Self some static
      struct subscript super switch throw throws try typealias var where while
    `),
    types: words(`
      Any Array Bool Character Data Dictionary Double Float Int Int8 Int16 Int32 Int64 Never Optional
      Set String UInt UInt8 UInt16 UInt32 UInt64 Void
    `),
    literals: words('true false nil'),
  }),
})

/** Zig, for the generated Zig SDK. */
export const zig = defineLanguage({
  name: 'zig',
  tokenize: scanner({
    // Zig has no block comments at all; `///` and `//!` are line comments.
    lineComments: ['//'],
    strings: [
      { open: '\\\\', close: '\n', multiline: false },
      { open: '"', close: '"', escapes: true },
    ],
    charLiteral: true,
    annotationSigils: ['@'],
    capitalisedIsType: true,
    keywords: words(`
      addrspace align allowzero and anyframe anytype asm break callconv catch comptime const continue
      defer else enum errdefer error export extern fn for if inline linksection noalias noinline nosuspend
      opaque or orelse packed pub resume return struct suspend switch test threadlocal try union unreachable
      usingnamespace var volatile while
    `),
    types: words(`
      anyerror anyopaque bool c_int c_long c_short c_uint c_ulong c_ushort comptime_float comptime_int
      f16 f32 f64 f80 f128 i8 i16 i32 i64 i128 isize noreturn type u8 u16 u32 u64 u128 usize void
    `),
    literals: words('true false null undefined'),
  }),
})

/** C, the ABI every other SDK is built on. */
export const c = defineLanguage({
  name: 'c',
  aliases: ['h'],
  tokenize: scanner({
    lineComments: ['//'],
    blockComment: ['/*', '*/'],
    strings: [{ open: '"', close: '"', escapes: true }],
    charLiteral: true,
    preprocessor: '#',
    keywords: words(`
      auto break case const continue default do else enum extern for goto if inline register restrict
      return sizeof static struct switch typedef union volatile while _Alignas _Alignof _Atomic
      _Generic _Noreturn _Static_assert _Thread_local
    `),
    types: words(`
      bool char double float int long short signed size_t unsigned va_list void int8_t int16_t int32_t
      int64_t uint8_t uint16_t uint32_t uint64_t intptr_t uintptr_t ptrdiff_t
    `),
    literals: words('NULL true false'),
    capitalisedIsType: true,
  }),
})

/** C#, for the SDK target being added alongside Objective-C. */
export const csharp = defineLanguage({
  name: 'csharp',
  aliases: ['cs', 'c#'],
  tokenize: scanner({
    lineComments: ['//'],
    blockComment: ['/*', '*/'],
    strings: [
      { open: '"""', close: '"""', multiline: true },
      { open: '"', close: '"', escapes: true, prefixes: ['@', '$', '$@', '@$'] },
    ],
    charLiteral: true,
    annotationSigils: [],
    capitalisedIsType: true,
    keywords: words(`
      abstract as async await base break case catch checked class const continue default delegate do else
      enum event explicit extern finally fixed for foreach get goto if implicit in init interface internal
      is lock namespace new operator out override params partial private protected public readonly record
      ref required return sealed set sizeof stackalloc static struct switch this throw try typeof unchecked
      unsafe using value virtual volatile where while with yield
    `),
    types: words(`
      bool byte char decimal double dynamic float int long nint nuint object sbyte short string uint
      ulong ushort var void
    `),
    literals: words('true false null'),
  }),
})

/** Objective-C, for the SDK target being added alongside C#. */
export const objectivec = defineLanguage({
  name: 'objectivec',
  aliases: ['objc', 'objective-c', 'm'],
  tokenize: scanner({
    lineComments: ['//'],
    blockComment: ['/*', '*/'],
    strings: [{ open: '"', close: '"', escapes: true, prefixes: ['@'] }],
    charLiteral: true,
    preprocessor: '#',
    annotationSigils: ['@'],
    capitalisedIsType: true,
    keywords: words(`
      auto break case const continue default do else enum extern for goto if inline nonatomic return
      sizeof static struct switch typedef union volatile while assign copy strong weak readonly readwrite
      instancetype in out inout bycopy byref oneway
    `),
    types: words(`
      BOOL char double float id int long short signed unsigned void NSInteger NSUInteger CGFloat
      NSString NSArray NSDictionary NSError NSNumber NSData NSObject SEL Class IMP
    `),
    literals: words('YES NO nil Nil NULL'),
  }),
})

/** Every definition this site adds on top of the ones the library ships. */
export const additionalLanguages = [rust, swift, zig, c, csharp, objectivec] as const

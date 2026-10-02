---
title: Swift
summary: A SwiftPM package over libotio, with a class per schema, real enums and throwing calls.
section: Languages
order: 5
---

The Swift package links `libotio`, the C ABI built from the Rust core, and is
generated from that ABI, so it carries the whole data model: the schemas, the
composition algorithms, the ten edit operations and the file-format adapters.
Its shape is OpenTimelineIO's own Swift bindings: a class per schema deriving
as the schemas derive, an initializer per schema, values as structs, real
enums, `throws` for failure, and no document in the surface.

```swift
import OpenTimelineIO
```

## Install

The package links against a static `libotio` it expects to find in
`sdk/swift/lib/`, and the linker is told where that is on the command line.
The library is not checked in, so build it from the Rust core first:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/swift/lib/

cd sdk/swift
swift test -Xlinker -L"$PWD/lib"
```

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
also carries the Swift SDK's sources, laid out as in the repository with an
empty `lib/`, and a prebuilt `libotio-<version>-<target>.tar.gz` per target
whose `lib/` holds the library to put there.

## Your first program

<!-- ::sample id="read-an-edl" lang="swift" -->

Reading hands back what the file is about, its root object, rather than a
container to look inside. `OTIO.open` infers the format from the filename
instead of taking one, and `OTIO.save` writes back the same way. The rate is
the one option an EDL cannot do without; see [CMX 3600 EDL](/docs/formats/edl).

`for case let clip as Clip` works because every object arrives as the class of
its schema, so `as?` asks what one really is:

```swift
for child in try track.children() {
    if let clip = child as? Clip, let reference = try clip.mediaReference() as? ExternalReference {
        print(try reference.targetURL())
    }
}
```

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="swift" -->

Each object is made on its own and joins a timeline when you put it in one.
Values such as `RationalTime` and `TimeRange` are structs.

## How objects and documents work here

There is no document to hold. Underneath, the core keeps a timeline's objects
in an arena; the package does that bookkeeping, and objects keep their
timeline alive between them, so there is nothing to close. `close()` exists
for releasing a large one early, and every object that lived in it then fails
with `.nullPointer` rather than reading freed memory.

Objects made apart stay apart until one takes the other in. Appending or
inserting an object that is still a child in another timeline is refused as
the library refuses it, with `.coreError` and the library's own message, and
so is placing one whose handle has gone stale, with `.staleHandle`. Both are
refused before that timeline is brought over, so both stay whole, and closing
one leaves the other working.

Compositions are deliberately not Swift collections; children are read with
`children()`.

## Errors and missing values

A call that can fail throws an `OTIOError` carrying a `Status`.

A call that only *names* an object, such as `detachChild`, `indexOfChild` or
`hasChild`, refuses one that belongs to a different timeline, and refuses it
before asking the library, because merging the two and failing afterwards
would already have done the damage. That refusal is an `OTIOError` with
`.invalidArgument` and `isOtherTimeline` set; the other timeline is untouched.

Where "there is nothing here" is one of the answers, such as an item with no
source range or a clip with no active media reference, the call answers `nil`,
because that is an answer rather than a failure:

```swift
if let span = try clip.sourceRange() {
    print(span)
}
```

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="swift" -->

The ten edit operations and the algorithms are static functions on `OTIO`,
such as `OTIO.insert`, `OTIO.overwrite` and `OTIO.flattenTracks`, because each
is about two objects and belongs to neither.
[Editing a timeline](/docs/guides/editing) says what each edit does.

## Formats

`OTIO.readFromFile` and `OTIO.writeToFile` take a format, such as `.cmx3600`
or `.fcpxXML`. Every format the C ABI has is here: OTIO JSON, ALE, CMX 3600
EDL, both FCP XML flavours, AAF, and the `.otioz` and `.otiod` bundles, which
are read and written through a path only.

<!-- ::sample id="convert-a-format" lang="swift" -->

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Platforms

CI builds and tests the package on Linux and macOS. Windows is left out: the
package links the Rust static library through the platform's toolchain, which
CI does not have set up for MSVC.

## Reference

- [`sdk/swift/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/swift/README.md).
- The [C ABI reference](/reference) the package is generated from, and
  [ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md)
  for every deliberate departure from upstream's Swift bindings.

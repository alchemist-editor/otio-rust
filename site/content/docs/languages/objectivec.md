---
title: Objective-C
summary: A Cocoa library over libotio, on Apple's runtime and GNUstep's, with a class per schema, values as C structs and failure as an NSError.
section: Languages
order: 10
---

The Objective-C SDK links `libotio`, the C ABI built from the Rust core, and
runs on Apple's runtime and on GNUstep's. It is generated from that ABI by
`otio-sdk-gen`, so it carries the whole data model: the schemas, the
composition algorithms, the ten edit operations and the file-format adapters.
What things are called follows upstream OpenTimelineIO; what the binding *is*
follows Cocoa.

```objc
#import <OpenTimelineIO/OpenTimelineIO.h>
```

## Install

The Rust library it calls into is built by cargo and copied into
`sdk/objc/lib/`; it is not checked in:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/objc/lib/
```

Then, from `sdk/objc`:

```sh
make          # build/libOpenTimelineIO.a
make check    # build and run the tests
```

On Linux this needs GNUstep's base library and a clang that can build
Objective-C: `gnustep-devel` and `clang` on Debian and Ubuntu. On macOS it
needs nothing but Xcode's command line tools.

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
also carries the Objective-C SDK's sources, laid out as in the repository with
an empty `lib/`, and a prebuilt `libotio-<version>-<target>.tar.gz` per target
whose `lib/` holds the library to put there.

## Your first program

<!-- ::sample id="read-an-edl" lang="objectivec" -->

Reading hands back the object the file is about, which for an EDL is the
timeline it describes. `OTIOOpen` infers the format from the filename instead
of taking one, and `OTIOSave` writes back the same way. The rate is the one
option an EDL cannot do without; see [CMX 3600 EDL](/docs/formats/edl).

Every handle the library hands back arrives as the class its schema names, so
`isKindOfClass:` tells the truth.

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="objectivec" -->

Each object is made on its own with an ordinary class factory, such as
`clipWithName:error:`, and joins a timeline when you put it into one. Values
are C structs, as `NSRange` and `CGRect` are, with `OTIORationalTimeMake` and
`OTIOTimeRangeMake` to build one and C functions such as
`OTIORationalTimeToSeconds` to compute with one.

## How objects and documents work here

There is no document in the surface. Underneath, the core keeps a timeline's
objects in an arena and an object is an index into one; this SDK does that
bookkeeping. The objects hold the arena strongly between them, so it outlives
every handle into it and goes when the last object naming it does.

An object that has not joined anything is a timeline of one. Putting it into
another moves it there, and an object from a timeline it was never put into is
refused rather than quietly dragged along with everything around it.

`-[OTIOSerializableObject close]` is there for releasing a large timeline at a
moment you chose. Every object that lived in it fails with
`OTIOStatusNullPointer` afterwards rather than reading freed memory.

The sources build both with ARC and with manual retain and release, since
everything the SDK owns is confined to the runtime.

## Errors and missing values

Failure is an `NSError` out-parameter in the `OTIOErrorDomain`, whose code is
the `OTIOStatus`. A call that can fail answers `NO` or `nil`.

- **"There is nothing here" is a failure you can tell apart.** It fails with
  `OTIOStatusNoValue`, which `OTIOIsNoValue(error)` recognises.
- **So is an object from another timeline.** A call that only names an object
  refuses one from elsewhere before asking the library, with
  `OTIOStatusInvalidArgument`, and `OTIOIsOtherTimeline(error)` recognises it.
- **An object that already has a parent is refused before it moves.**
  Appending or inserting one that is still a child in another timeline fails
  as the library would fail it, with `OTIOStatusCoreError` and its message,
  and placing one whose handle has gone stale fails with
  `OTIOStatusStaleHandle`, but before that timeline is brought over, so both
  stay whole.

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="objectivec" -->

The ten edit operations and the algorithms are C functions, such as
`OTIOInsert`, `OTIOOverwrite` and `OTIOFlattenTracks`, each taking an
`NSError **` last. [Editing a timeline](/docs/guides/editing) says what each
edit does.

## Formats

`OTIOReadFromFile` and `OTIOWriteToFile` take an `OTIOFormat`:
`OTIOFormatOTIOJSON`, `OTIOFormatALE`, `OTIOFormatCMX3600`,
`OTIOFormatFcp7XML`, `OTIOFormatFcpxXML`, `OTIOFormatAAF`, and the two bundles,
`OTIOFormatOTIOZ` and `OTIOFormatOTIOD`, which are read and written through a
path only.

<!-- ::sample id="convert-a-format" lang="objectivec" -->

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Platforms

CI builds and tests the SDK on Linux, with GNUstep, and on macOS, with Apple's
runtime.

## Reference

- [`sdk/objc/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/objc/README.md).
- The [C ABI reference](/reference) the SDK is generated from, and
  [ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md)
  for every deliberate departure from upstream.

---
title: C#
summary: A .NET class library that loads libotio at run time, with a class per schema, exceptions, and nothing to dispose.
section: Languages
order: 9
---

The C# SDK is a .NET class library over `libotio`, the C ABI built from the
Rust core, which it loads at run time rather than linking. It is generated
from that ABI, so it carries the whole data model: the schemas, the
composition algorithms, the ten edit operations and the file-format adapters.
C# has no upstream OpenTimelineIO binding to copy, so what things are *called*
follows upstream's Python and C++, spelled the way .NET spells names, and what
the binding *is* follows upstream's Java bindings, the nearest thing upstream
has to a managed language.

```csharp
using OpenTimelineIO;
```

## Install

The assembly loads a native `libotio` it expects to find beside itself, which
the project copies out of `sdk/csharp/lib/`. Because it is loaded rather than
linked, this is the shared library, not the static one:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.so sdk/csharp/lib/     # libotio.dylib on macOS

cd sdk/csharp
dotnet run --project tests
```

The library is not checked in. The project targets .NET 8 (`net8.0`).

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
also carries the C# SDK's sources, laid out as in the repository with an empty
`lib/`, and a prebuilt `libotio-<version>-<target>.tar.gz` per target whose
`lib/` holds the library, shared where it was built.

## Your first program

<!-- ::sample id="read-an-edl" lang="csharp" -->

Reading hands back the object the file is about, which for an EDL is the
timeline it describes. `Otio.Open` infers the format from the filename instead
of taking one, and `Otio.Save` writes back the same way. Values are
`readonly struct`s, so the EDL's rate is chosen when the `ReadOptions` are made
rather than set afterwards; it is the one option an EDL cannot do without (see
[CMX 3600 EDL](/docs/formats/edl)).

An object is a class of its schema, so a pattern match asks what one really
is:

```csharp
foreach (var child in track.Children())
{
    if (child is Clip clip && clip.MediaReference(null) is ExternalReference reference)
    {
        Console.WriteLine(reference.TargetUrl());
    }
}
```

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="csharp" -->

Every object is made on its own with `new` and joins a timeline when you put it
into one, so nothing has to exist before the thing it goes into.

## How objects and documents work here

There is no document in the surface, as there is none in upstream's own
bindings. Underneath, the core keeps a timeline's objects in an arena and an
object is an index into one; the SDK does that bookkeeping. The objects hold
the arena between them, so it goes when the last of them does and there is
nothing to dispose. `Close()` is there for releasing a large timeline at a
moment you chose; every object that lived in it fails afterwards rather than
reading freed memory.

An object that has not joined anything is a timeline of one. Putting it into
another moves it there. An object that is still a child in another timeline is
refused when it is appended or inserted, and so is one whose handle has gone
stale, wherever it is placed: the refusal is the library's own,
`Status.CoreError` or `Status.StaleHandle` and its message, but it is made
before that timeline is brought over, so both stay whole.

## Errors and missing values

A call that can fail throws an `OtioException` carrying a `Status`.

A call that only names an object refuses one from another timeline before
asking the library, with `OtherTimelineException`, which derives from
`OtioException` and so can be caught on its own with
`catch (OtherTimelineException)`.

Where "there is nothing here" is one of the answers, such as an item with no
source range or a clip with no active media reference, the call answers `null`,
because that is an answer rather than a failure:

```csharp
if (clip.SourceRange() is TimeRange span)
{
    Console.WriteLine(span);
}
```

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="csharp" -->

The ten edit operations and the algorithms are static methods on `Otio`, such
as `Otio.Insert`, `Otio.Overwrite` and `Otio.FlattenTracks`, because each is
about two objects and belongs to neither.
[Editing a timeline](/docs/guides/editing) says what each edit does.

## Formats

`Otio.ReadFromFile` and `Otio.WriteToFile` take a `Format`: `OtioJson`, `Ale`,
`Cmx3600`, `Fcp7Xml`, `FcpxXml`, `Aaf`, and the two bundles, `Otioz` and
`Otiod`, which are read and written through a path only.

<!-- ::sample id="convert-a-format" lang="csharp" -->

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Platforms

CI builds and tests the SDK on Linux and macOS. Windows is left out, as it is
for the other SDKs except Zig.

## Reference

- [`sdk/csharp/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/csharp/README.md).
- The [C ABI reference](/reference) the library is generated from, and
  [ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md)
  for every deliberate departure from upstream.

---
title: Zig
summary: The one generated SDK that keeps the document in the open, because an arena is how Zig already works.
section: Languages
order: 7
---

The Zig package links `libotio`, the C ABI built from the Rust core, and is
generated from that ABI, so it carries the whole data model: the schemas, the
composition algorithms, the ten edit operations and the file-format adapters.
Zig has no upstream binding to copy, so what the binding *is* was decided
here, and it differs from the other SDKs in one large way: the document is
visible, held the way a Zig programmer holds any other arena. It needs Zig
0.16 or newer.

```zig
const otio = @import("otio");
```

There is no `@cImport` and no header is read at build time. The declarations
are written out directly from the same description the rest of the SDK is
generated from, so the package builds wherever Zig builds and cross-compiles
with nothing but the static library.

## Install

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
carries the package with the static library for every target it is built for,
under `lib/<target>/`, so one fetch covers them all:

```sh
zig fetch --save https://github.com/alchemist-editor/otio-rust/releases/download/v0.1.0/otio-zig-0.1.0.tar.gz
```

```zig
const otio = b.dependency("otio", .{ .target = target, .optimize = optimize });
exe.root_module.addImport("otio", otio.module("otio"));
```

The targets are `x86_64-linux-gnu`, `aarch64-linux-gnu`, `aarch64-macos`,
`x86_64-macos`, `x86_64-windows-msvc`, `aarch64-windows-msvc` and
`x86_64-windows-gnu`.

To build from the repository instead, put a static library built from the Rust
core in `lib/<target>/` or in `lib/` itself:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/zig/lib/

cd sdk/zig
zig build test
```

If the library lives somewhere else, say so: `zig build test -Dlibrary=/path/to/dir`.
For an MSVC target the library is `otio.lib`, built against the static C
runtime and stripped of Rust's own copy of compiler-rt with
`scripts/strip-msvc-builtins.sh`; the
[README](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/zig/README.md#windows)
has the commands.

## Your first program

<!-- ::sample id="read-an-edl" lang="zig" -->

Reading hands back a `Document`, and `defer document.deinit()` is how it goes.
`otio.open` infers the format from the filename instead of taking one, and
`document.save` writes back the same way. The rate is the one option an EDL
cannot do without; see [CMX 3600 EDL](/docs/formats/edl).

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="zig" -->

Every object is made in a document, which each `init` takes first. An object is
a `Node`: a handle, and the document it can be resolved against. The schemas
are Zig types holding one, and each writes out the methods of every schema it
derives from, so a `Clip` answers to everything an `Item`, a `Composable` and
a `Node` answer to. Ask a node what it is with its `as` method, and cast back
up the ladder the same way:

```zig
if (node.asClip()) |clip| {
    const reference = try clip.mediaReference(null);
}
try track.appendChild(clip.asNode());
```

## How objects and documents work here

The other SDKs hide the document, so that a clip is built on its own and
adopts one when it is appended. That rests on a finalizer and a cache of live
handles, which Zig has neither of and would have to pay for with an allocator
inside every object. A Zig programmer already holds an arena and hands it to
the things that allocate from it, which is exactly what a document is.

Allocation is the caller's: nothing here allocates behind your back. Anything
the library hands back that has to be freed, such as a name, a JSON document
or a list of children, is copied into an allocator you pass, and freed with
`allocator.free`. A call that needs one takes it as its first argument after
the receiver, which is the only signal you need that it allocates:

```zig
const name = try clip.name(allocator);
defer allocator.free(name);
```

A call that answers with several things at once hands back a small record with
a `deinit` of its own:

```zig
const both = try track.rangesOfChildren(allocator);
defer both.deinit(allocator);
```

## Errors and missing values

A call that can fail answers with an error union over `Error`. A Zig error
carries no message, so the failing call's sentence is kept for the thread that
made the call and read with `otio.lastErrorMessage()`. Read it before the next
failure on that thread, which replaces it. Asking an object for something it
does not have fails rather than answering with a zero value: a clip asked for a
track's kind reports `error.CoreError`.

An object from another document is refused with `error.ForeignObject`.

Where "there is nothing here" is one of the answers, such as an item with no
source range or a clip with no active media reference, the answer is an
optional rather than an error, because that is what Zig has optionals for:

```zig
if (try clip.sourceRange()) |span| {
    // the clip is trimmed to span
} else {
    // it is untrimmed
}
```

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="zig" -->

The ten edit operations and the algorithms are methods of the `Document`, such
as `document.insert`, `document.overwrite` and `document.flattenTracks`,
because each is about two objects and belongs to neither.
[Editing a timeline](/docs/guides/editing) says what each edit does.

## Formats

`Document.readFromFile` and `document.writeToFile` take a `Format`:
`.otio_json`, `.ale`, `.cmx3600`, `.fcp7_xml`, `.fcpx_xml`, `.aaf`, and the
two bundles, `.otioz` and `.otiod`, which are read and written through a path
only.

<!-- ::sample id="convert-a-format" lang="zig" -->

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Platforms

Wherever Zig builds and the Rust core builds. CI runs the tests on Linux,
macOS and Windows (MSVC), and each release builds and runs a program that
depends on the released package on every target a runner can run.

The description the package is generated from carries struct layouts for
64-bit targets and `wasm32`. A target that aligns a `double` to four bytes,
such as `i386`, lays the structs out differently, and the package refuses to
compile there with a message saying so rather than getting the offsets wrong.

## Reference

- [`sdk/zig/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/sdk/zig/README.md).
- [How the SDKs are made](/docs/internals/how-the-sdks-are-made), which says
  why Zig is the exception.
- The [C ABI reference](/reference) the package is generated from, and
  [ADR 0003](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0003-sdk-generation.md).

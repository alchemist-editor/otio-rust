# OpenTimelineIO for Zig

Read, write and edit OpenTimelineIO timelines from Zig.

```zig
const otio = @import("otio");
```

This package is generated from the C interface of the otio-rust core, so it
carries the whole data model: the schemas, the composition algorithms, the ten
edit operations and the file-format adapters. Do not edit the files under
`src/` by hand — see [`../README.md`](../README.md) for how they are made and
regenerated.

Zig 0.16 or newer.

## From a release

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
carries this package with the static library for every target it is built
for, under `lib/<target>/`, so one fetch covers them all:

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

## Building

The package links against a static library built from the Rust core, which it
expects to find in `lib/<target>/` (as a release has it) or in `lib/` itself:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/zig/lib/

cd sdk/zig
zig build test
```

The library itself is not checked in; `lib/.gitignore` keeps it out. If it
lives somewhere else, say so: `zig build test -Dlibrary=/path/to/dir`.

### Windows

For an MSVC target the library is `otio.lib`, built against the static C
runtime (the one Zig links) and stripped of Rust's own copy of compiler-rt:

```sh
RUSTFLAGS="-C target-feature=+crt-static" \
  cargo build -p otio-capi --release --target x86_64-pc-windows-msvc
scripts/strip-msvc-builtins.sh target/x86_64-pc-windows-msvc/release/otio.lib
cp target/x86_64-pc-windows-msvc/release/otio.lib sdk/zig/lib/

cd sdk/zig
zig build test -Dtarget=x86_64-windows-msvc
```

The strip is not optional. Every Rust static library carries compiler-rt
routines (`__divti3`, `__multf3`, …), Zig links its own, and lld-link rejects
the two copies as duplicate symbols for MSVC targets. The script needs `zig`
on `PATH`, and runs anywhere Zig does. Zig finds the Windows SDK itself, so
no Visual Studio developer prompt is needed.

There is no `@cImport` anywhere and no header is read at build time. The
declarations are written out directly from the same description the rest of
the SDK is generated from, so the package builds wherever Zig builds and
cross-compiles with nothing but the static library.

The description carries a struct layout for 32-bit pointers and one for
64-bit, and both were computed for an ABI that aligns a 64-bit scalar to
eight bytes. That covers the 64-bit targets and `wasm32`. A target that
aligns a `double` to four — `i386` is the one people meet — lays these
structs out differently, and the package refuses to compile there with a
message saying so rather than getting the offsets wrong. Supporting it would
need a third layout in the description, which is shared with the other SDKs.

## Using it

Everything lives in a `Document`, which owns the objects in it. It is an
arena, and it is held in the open the way a Zig programmer holds any other
arena:

```zig
const document = try otio.Document.readFromFile(.cmx3600, "cut.edl", null);
defer document.deinit();

const root = (try document.root()) orelse return error.Empty;
const clips = try root.findClips(allocator);
defer allocator.free(clips);
```

`otio.open` infers the format from the filename, and `document.save` writes it
back the same way.

An object is a `Node`: a handle, and the document it can be resolved against.
The schemas are Zig types holding one, and each writes out the methods of
every schema it derives from, so a `Clip` answers to everything an `Item`, a
`Composable` and a `Node` answer to. Ask a node what it is with its `as`
method, and cast back up the ladder the same way:

```zig
if (node.asClip()) |clip| {
    const reference = try clip.mediaReference(null);
}
try track.appendChild(clip.asNode());
```

Asking an object for something it does not have fails rather than answering
with a zero value: a clip asked for a track's kind reports
`error.CoreError`, and `lastErrorMessage()` says why.

### Memory

Anything the library hands back that has to be freed — a name, a JSON
document, a list of children — is copied into an allocator you pass, and freed
with `allocator.free`. A call that needs one takes it as its first argument
after the receiver, which is the only signal you need that it allocates:

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

### Errors, and "there is nothing here"

A call that can fail answers with an error union over `Error`. Where "there is
nothing here" is one of the answers — an item with no source range, a clip
with no active media reference — the answer is an optional rather than an
error, because that is what Zig has optionals for:

```zig
if (try clip.sourceRange()) |span| {
    // the clip is trimmed to span
} else {
    // it is untrimmed
}
```

A Zig error carries no message. The failing call hands its sentence back
beside its status, and the package keeps a copy for the thread that made the
call, read with `otio.lastErrorMessage()`. It is worth reading before the next
failure on that thread, which replaces it.

## Following upstream

What things are called and which members exist follow upstream
OpenTimelineIO's own bindings rather than being invented here: a stored field
is a property and a computed one is a method, getters are bare nouns and
setters take a `set` prefix, and the schema names and member names are
upstream's, spelled in Zig's case.

Zig is the one target with no upstream binding to copy, so what the binding
*is* was decided here. Five things differ from the other SDKs, each because
Zig is not Go or Python:

- **The document is visible.** The other SDKs are hiding it, so that a clip is
  built on its own and adopts a document when it is appended. That rests on a
  finalizer and a cache of live handles, which Zig has neither of and would
  have to pay for with an allocator inside every object. A Zig programmer
  already holds an arena and hands it to the things that allocate from it,
  which is exactly what a document is.
- **Allocation is the caller's.** Nothing here allocates behind your back.
- **"No value" is an optional**, not a sentinel error.
- **Failure is an error union**, and the message is read separately, because a
  Zig error is a value with no room for one.
- **The schema ladder is written out.** Zig has no inheritance and no
  `usingnamespace` any more, so every inherited method is a real declaration
  on the type that inherits it.

[ADR 0003](../../docs/adr/0003-sdk-generation.md) records these in full.

## Supported platforms

Wherever Zig builds and the Rust core builds. CI runs Linux and macOS;
Windows is untested, as it is for the C ABI and the Go SDK.

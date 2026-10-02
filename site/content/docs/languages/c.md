---
title: C
summary: libotio and otio.h, the C ABI every generated SDK is built on, with handles, out-parameters and status codes.
section: Languages
order: 8
---

The C ABI is `libotio`, plus the header in `crates/otio-capi/include/otio.h`.
It is the layer every language binding other than Python sits on: Go, Swift,
Zig, C++, C# and Objective-C link it, and TypeScript is the same ABI compiled
to WebAssembly. From C you use it directly, which means the document is
explicit, values are copied out into buffers you free, and every call that can
fail returns a status.

## Install

```sh
cargo build -p otio-capi --release
```

That writes `libotio.so`, `libotio.dylib` or `otio.dll` alongside the static
`libotio.a`, under `target/release/`. Compile against `include/otio.h` and link
`otio`.

Each [GitHub release](https://github.com/alchemist-editor/otio-rust/releases)
carries a prebuilt `libotio-<version>-<target>.tar.gz` per target, holding
`include/otio.h`, `lib/` (static, and shared where built) and the licence. The
targets are `x86_64-linux-gnu`, `aarch64-linux-gnu`, `aarch64-macos`,
`x86_64-macos`, `x86_64-windows-msvc`, `aarch64-windows-msvc` and
`x86_64-windows-gnu`, the last static only. A `SHA256SUMS` file covers every
asset.

## Your first program

<!-- ::sample id="read-an-edl" lang="c" -->

Most of what the C ABI asks of a caller is in that program. The format is an
`OtioFormat`, and the options start from `otio_read_options_default()`; the
rate is the one an EDL cannot do without (see [CMX 3600 EDL](/docs/formats/edl)).
A list is asked for twice, once with a capacity of zero to learn the count and
once to fill a buffer of that size. A string comes back as an `OtioBuffer` the
caller frees with `otio_buffer_free`.

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="c" -->

`otio_document_new` makes a document, and every constructor, such as
`otio_clip_new`, takes the document to build the object in. The root is set
explicitly with `otio_document_set_root`, and `otio_document_free` releases the
document and everything in it.

## How objects and documents work here

**An object is a handle, not a pointer.** `OtioNode` is the index of a slot in
the document's arena and the generation of that slot. Nothing hands out a
pointer into a document, so an edit that moves objects around cannot leave a
caller holding a dangling one, and a handle to something that has been removed
fails a lookup instead of reaching whatever took the slot. Two `OtioNode`
values compare equal when they name the same object, and that is the whole of
object identity here.

**A document is explicit.** There is one document, created and freed when the
caller says. Each document owns its objects, and there is no call that moves an
object from one document into another: build in one document, or write and
read.

**A value is copied out, never borrowed.** Times are plain structs passed by
value; strings and whole written files come back as owned `OtioBuffer`s.
Returning a pointer into the document would dangle the moment anything was
edited.

**Threads.** A document is not internally synchronized. Several threads may
read one at the same time; a thread that edits one must be the only thread
touching it. Two threads working on two documents never interfere.

## Errors and missing values

Every call that can fail returns an `OtioStatus`, delivers its result through
out-parameters, and takes one more out-parameter last: `OtioBuffer *out_error`,
where it writes the sentence saying what went wrong. `OTIO_STATUS_OK` is zero,
so `if (otio_...(...))` reads as "if it failed". Pass `NULL` to get only the
status. When it is not `NULL` it is written on every return, empty after
success, so a caller can free it unconditionally.

The message comes back from the call that failed rather than from a second
call asking about "the last failure", so it does not matter which thread asks.

`OTIO_STATUS_NO_VALUE` means the question was answered and the answer is
"nothing", which is what an item with no source range reports. It is not an
error, but it still carries a message saying what had no value. A panic in the
Rust core is caught at the boundary and reported as `OTIO_STATUS_PANIC` rather
than unwinding into C.

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="c" -->

The ten edit operations are `otio_edit_*` calls, such as `otio_edit_insert` and
`otio_edit_overwrite`, and the algorithms are `otio_algorithm_*`, such as
`otio_algorithm_flatten_stack`. [Editing a timeline](/docs/guides/editing) says
what each edit does.

## Formats

Every format goes through the same four calls, `otio_read_from_bytes`,
`otio_read_from_file`, `otio_write_to_bytes` and `otio_write_to_file`, picked
by an `OtioFormat`: OTIO JSON, ALE, CMX 3600 EDL, both FCP XML flavours, AAF,
and the two bundle formats, `.otioz` and `.otiod`. A bundle is read and written
through a path only; the calls that take bytes refuse one with
`OTIO_STATUS_UNSUPPORTED`. What is written is the document's root, which for a
bundle has to be a timeline.

<!-- ::sample id="convert-a-format" lang="c" -->

A few adapter options are not exposed yet: ALE's explicit column order, AAF's
`transcribe_log`, and upstream's bundle `dry_run`. See
[Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

Metadata is named by a path rather than a key, such as `"cmx_3600.reel"` or
`"comments[0]"`, read with calls such as `otio_metadata_get_string`.

## Platforms

CI compiles and runs the C ABI's own C test program on Linux and macOS. The
library builds on Windows and its struct layouts are asserted from the Rust
side, but no C program is linked there in CI.

The ABI is not stable while the version is 0.x: structs passed by value may
gain fields and enums may gain values. `OTIO_NODE_KIND_OTHER` and
`OTIO_VALUE_OTHER` exist so that a newer core reports something honest to a
caller built against an older header.

## Reference

- [`crates/otio-capi/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-capi/README.md):
  the four decisions the ABI makes, and how the header is kept honest.
- [The reference](/reference), generated from the same description as every
  SDK, with each declaration lifted verbatim from `otio.h`.
- [How the SDKs are made](/docs/internals/how-the-sdks-are-made).

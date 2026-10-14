# otio-capi

A C ABI for the Rust core: `libotio`, plus the header in
[`include/otio.h`](include/otio.h).

This is the layer every language binding other than Python is meant to sit
on. A language with a C FFI — Swift, C#, Go, Java, Node, C++ — can drive the
whole data model, the composition algorithms, the ten edit operations and the
file-format adapters without knowing anything about Rust.

```c
OtioDocument *document = NULL;
OtioBuffer error;
if (otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", NULL, &document, &error)) {
    fprintf(stderr, "%s\n", error.data);
    otio_buffer_free(error);
    return 1;
}

OtioNode timeline;
otio_document_root(document, &timeline, NULL);

size_t count = 0;
otio_node_find_clips(document, timeline, NULL, 0, &count, NULL);
printf("%zu clips\n", count);

otio_write_to_file(OTIO_FORMAT_OTIO_JSON, document, "cut.otio", NULL, NULL);
otio_document_free(document);
```

Every format goes through the same four calls, `otio_read_from_bytes`,
`otio_read_from_file`, `otio_write_to_bytes` and `otio_write_to_file`, picked
by an `OtioFormat`: OTIO JSON, ALE, CMX 3600 EDL, both FCP XML flavours, AAF,
and the two bundle formats, `.otioz` and `.otiod`. A bundle is a timeline
packaged with copies of its media, so it is read and written through a path
only; the calls that take bytes refuse one with `OTIO_STATUS_UNSUPPORTED`.
What is written is the document's root, which for a bundle has to be a
timeline.

## Building

```sh
cargo build -p otio-capi --release
```

That writes `libotio.so`, `libotio.dylib` or `otio.dll` alongside the static
`libotio.a`, under `target/release/`. Compile against `include/otio.h` and
link `otio`.

## The four decisions this crate makes

The PyO3 crate had to answer the same questions first, and where the answers
can be the same they are. Where they differ, it is because Python has a
garbage collector and C does not.

**An object is a handle, not a pointer.** `OtioNode` is `otio-core`'s
`NodeId` spelled for C: the index of a slot in the document's arena and the
generation of that slot. Nothing hands out a pointer into a document, so an
edit that moves objects around cannot leave a caller holding a dangling one,
and a handle to something that has been removed fails a lookup instead of
reaching whatever took the slot. This is the property
[ADR 0001](../../docs/adr/0001-ownership-model.md) picked the arena for.

The PyO3 crate needs more than this, because `track[0] is track[0]` has to be
true in Python, so it keeps a cache of the wrapper handed out for each node.
C has no such expectation: two `OtioNode` values compare equal when they name
the same object, and that is the whole of object identity here.

**A document is explicit.** In Python, `Clip("a")` exists on its own, so the
bindings give every freshly built object a document of its own. C is not
troubled by that: `otio_clip_new` takes the document to build the clip in, so
there is one document, created and freed when the caller says.

**A value is copied out, never borrowed.** Every string comes back as an
`OtioBuffer` the caller frees. Returning a pointer into the document would be
faster and would dangle the moment anything was edited — in C, silently. The
PyO3 crate takes the same borrow-for-one-call discipline for the same reason.

**Failure is a status and a message.** Python has exceptions; C gets an
`OtioStatus` return, and every call that can fail takes one more parameter,
last: `OtioBuffer *out_error`, where it writes the sentence saying what went
wrong. Pass `NULL` to get only the status. When it is not `NULL` it is written
on every return — empty after success, the message otherwise — so a caller
can free it unconditionally.

The message comes back from the call that failed rather than from a second
call asking about "the last failure". That used to be how it worked, with the
message kept per thread, and it meant every binding whose runtime can move
work between OS threads — goroutines, Swift's async tasks, .NET's thread pool
— had to pin the two calls to one thread or risk reading another call's
message. Handing it back directly removes the question for all of them
([#63](https://github.com/alchemist-editor/otio-rust/issues/63)).

`OTIO_STATUS_NO_VALUE` is worth knowing: it means the question was answered
and the answer is "nothing", which is what an item with no source range
reports. It is not an error. `OTIO_STATUS_WRONG_KIND` is the other one to
know: it says the call was made on the wrong kind of object or metadata value,
such as a clip asked for its children, which is a bug in the caller rather
than something about the document.

## How the header is kept honest

A C header is a second statement of the ABI, so it can drift from the first
one — and a declaration whose parameters have drifted is a crash in somebody
else's program that no compiler will catch. Two tests stand between that and
a release:

- [`tests/header.rs`](tests/header.rs) reads every `#[unsafe(no_mangle)]`
  function out of this crate's source and every declaration out of
  `include/otio.h`, reduces both to a name and the C spelling of their types,
  and insists the two sets are equal. A missing declaration, an extra one, or
  a parameter that changed type all fail it.
- [`tests/abi.c`](tests/abi.c) is a C program that includes the header, links
  against the library and drives it the way a binding would: it builds a
  timeline, walks it, edits it, writes an EDL and reads it back, and checks
  the failure paths. [`tests/c_abi.rs`](tests/c_abi.rs) compiles and runs it.
  Both sides also assert the size of every struct that crosses by value, so a
  layout disagreement is a compile error rather than a wrong answer.

A machine-written header was the other option. It would mean depending on
`cbindgen`, and this workspace has no third-party dependencies; writing one
by hand would be more code than the header it produced, and the header is
also the interface's documentation, which reads better written than
generated. The tests above are what a generator would have bought.

## Metadata

OTIO metadata is a tree of dictionaries and arrays, so a value is named by a
path rather than a key: `otio_metadata_get_string(document, clip,
"cmx_3600.reel", &out)`, `"comments[0]"` for an array, and the two mix. A key
that itself holds a `.`, a `[` or a `\` writes it after a `\`, so
`"com\.avid\.reel"` is the one key `com.avid.reel`; keys that
`otio_metadata_key_at` hands back are not escaped. The full grammar, and which
status a malformed path or a missing value reports, is under METADATA PATHS at
the top of [`include/otio.h`](include/otio.h).

Setters write one value at a path whose parent exists already, so a nested
structure is built by creating the containers first with
`otio_metadata_set_dictionary` and `otio_metadata_set_vector`.

## What is not here yet

- **A few adapter options.** `OtioReadOptions` and `OtioWriteOptions` carry
  the options a caller actually has to state — an EDL's rate above all, since
  nothing in the file says it. ALE's explicit column order is not exposed,
  and neither is AAF's `transcribe_log`, which prints as it reads and so
  would need a callback across the boundary. A bundle's `content.otio` is
  always written at the default indent, and upstream's `dry_run`, which
  sizes a bundle without writing it, has no call of its own.
- **Moving one object between documents.** Each document owns its objects.
  `otio_document_absorb` moves every object of one document into another and
  hands back the old and new handles, which is how the SDKs let a clip be
  built on its own and appended later; there is no call that moves a single
  object out of a document that keeps the rest.
- **A C program linked on Windows.** The library builds there and its struct
  layouts are asserted from the Rust side, but `tests/abi.c` is compiled and
  run by CI on Linux and macOS only: a Windows runner has `clang` without the
  MSVC environment it needs to link, so attempting it would produce a failure
  that says nothing about this library. Set `CC` from a developer command
  prompt to run it there by hand.
- **A stable ABI.** `OTIO_ABI_VERSION` is 0 until the ABI is declared
  stable, so any release may still change it and a caller should insist on
  the exact `otio_version()` it was built for. What is already in place for
  that day: `otio_abi_version()` to check against the header at startup; the
  two options structs open with `struct_size`, so they can gain fields
  without breaking an older caller, and their defaults are filled into the
  caller's struct by `otio_*_options_init` rather than returned by value,
  which would have the library's size rather than the caller's; every enum is documented as open; and
  the header states how a packaged release names the shared library for each
  ABI version (`libotio.so.1`, `libotio.1.dylib`, `otio1.dll`). The
  packaging that produces those names is not built yet.

## Threads

A document is not internally synchronized. Several threads may read one at
the same time; a thread that edits one must be the only thread touching it.
Documents are independent, so two threads working on two documents never
interfere. A failing call hands its message back through its own `out_error`,
so there is no per-thread error state.

## Handles across documents

A handle is scoped to the document that issued it. `otio_document_clone`
keeps the arena's layout, so a handle into the original names the copy of the
same object in the clone; after that the two documents go their own ways.
`otio_node_equal` compares handle values, which is object identity within one
document and says nothing across two. A handle carries no document id, so the
library cannot always catch one passed to the wrong document: that is a
programming error, like an index into the wrong array. Widening `OtioNode` to
carry one was considered and left out, because it would double every handle
and change every SDK's handle type for a mistake the SDKs already rule out by
keeping each object tied to its document.

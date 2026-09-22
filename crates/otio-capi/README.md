# otio-capi

A C ABI for the Rust core: `libotio`, plus the header in
[`include/otio.h`](include/otio.h).

This is the layer every language binding other than Python is meant to sit
on. A language with a C FFI — Swift, C#, Go, Java, Node, C++ — can drive the
whole data model, the composition algorithms, the ten edit operations and the
file-format adapters without knowing anything about Rust.

```c
OtioDocument *document = NULL;
if (otio_read_from_file(OTIO_FORMAT_CMX_3600, "cut.edl", NULL, &document)) {
    fprintf(stderr, "%s\n", otio_error_message());
    return 1;
}

OtioNode timeline;
otio_document_root(document, &timeline);

size_t count = 0;
otio_node_find_clips(document, timeline, NULL, 0, &count);
printf("%zu clips\n", count);

otio_write_to_file(OTIO_FORMAT_OTIO_JSON, document, "cut.otio", NULL);
otio_document_free(document);
```

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
`OtioStatus` return and `otio_error_message()`. `OTIO_STATUS_NO_VALUE` is
worth knowing: it means the question was answered and the answer is
"nothing", which is what an item with no source range reports. It is not an
error.

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
"cmx_3600.reel", &out)`, `"comments[0]"` for an array, and the two mix. The
separators are the syntax, so a key containing a `.` or a `[` cannot be
addressed this way; no adapter in this repository writes one, and a caller
that meets one can still walk the keys with `otio_metadata_key_at` and read
the object whole with `otio_node_to_json`.

Setters write one value at a path whose parent exists already, so a nested
structure is built by creating the containers first with
`otio_metadata_set_dictionary` and `otio_metadata_set_vector`.

## What is not here yet

- **AAF.** `OtioFormat` covers OTIO JSON, ALE, CMX 3600 EDL and the two Final
  Cut XML flavours. Reading and writing an AAF live in the `otio-aaf` crate;
  adding it here is a variant and two match arms, tracked by issue #59.
- **A few adapter options.** `OtioReadOptions` and `OtioWriteOptions` carry
  the options a caller actually has to state — an EDL's rate above all, since
  nothing in the file says it. ALE's explicit column order is not exposed.
- **Moving an object between documents.** Each document owns its objects, and
  there is no call that takes one out of one document and puts it in another,
  because `otio-core` has no operation that remaps handles across arenas.
  Build in one document, or write and read.
- **A C program linked on Windows.** The library builds there and its struct
  layouts are asserted from the Rust side, but `tests/abi.c` is compiled and
  run by CI on Linux and macOS only: a Windows runner has `clang` without the
  MSVC environment it needs to link, so attempting it would produce a failure
  that says nothing about this library. Set `CC` from a developer command
  prompt to run it there by hand.
- **A stable ABI.** While the version is 0.x, the structs passed by value may
  gain fields and the enums may gain values. `OTIO_NODE_KIND_OTHER` and
  `OTIO_VALUE_OTHER` already exist so that a core that grows a schema reports
  something honest to a caller built against an older header.

## Threads

A document is not internally synchronized. Several threads may read one at
the same time; a thread that edits one must be the only thread touching it.
Documents are independent, so two threads working on two documents never
interfere. The last error message is per-thread.

---
title: Rust
summary: Using the core directly, with a Document that owns its objects and one crate per file format.
section: Languages
order: 1
---

Rust is not a binding. The crates in this repository are the library: a port
of upstream OpenTimelineIO's C++ core, with each file format in a crate of its
own. Every other language on this site reaches these same crates, through the
C ABI or, for Python, through PyO3. From Rust you skip that layer and hold the
document yourself.

## Install

Add the crates you need. `otio-core` is the data model; each adapter is its
own crate so that a program reading EDLs does not compile an AAF parser.

```toml
[dependencies]
otio-core = { git = "https://github.com/alchemist-editor/otio-rust" }
otio-cmx3600 = { git = "https://github.com/alchemist-editor/otio-rust" }
```

| Crate | What it holds |
| --- | --- |
| `opentime` | `RationalTime`, `TimeRange` and `TimeTransform` |
| `otio-core` | The object model, the `.otio` reader and writer, the algorithms and the edits |
| `otio-adapter` | The `Adapter` trait every format implements, and the error they share |
| `otio-cmx3600` | CMX 3600 EDL (`Cmx3600`) |
| `otio-ale` | Avid Log Exchange (`Ale`) |
| `otio-fcp7` | Final Cut Pro 7 XML (`Fcp7Xml`) |
| `otio-fcpx` | Final Cut Pro X XML (`FcpxXml`) |
| `otio-aaf` | AAF (`Aaf`) |
| `otio-bundle` | The `.otioz` and `.otiod` bundles |

The workspace is edition 2024 with a `rust-version` of 1.85. `otio-core`
depends on nothing but `opentime`, and the workspace has no third-party
dependencies.

To build and test everything from a clone:

```sh
git clone https://github.com/alchemist-editor/otio-rust
cd otio-rust
cargo build --workspace
cargo test --workspace
```

## Your first program

<!-- ::sample id="read-an-edl" lang="rust" -->

Reading goes through the `Adapter` trait from `otio-adapter`, implemented by a
unit struct per format. Each format's options are a struct of their own, so an
option meant for one format cannot be handed to another, and a misspelled one
is a compile error. The rate is the option that matters for an EDL: see
[CMX 3600 EDL](/docs/formats/edl).

What comes back is a `Document`. Its `root()` is an `Option<NodeId>`, and
`try_get` resolves a handle to the `Node` it names.

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="rust" -->

Objects are plain structs wrapped in the `Node` enum and inserted into the
document, which hands back a `NodeId` for each. Links between objects, such as
the timeline's `tracks` and a track's children, are handles too, so building
is inserting and then linking with `append_child`.

## How objects and documents work here

Upstream's C++ core uses intrusive reference counting plus raw parent
back-pointers, which is a reference cycle. `otio-core` stores every object in
a generational arena inside a `Document` and refers to them by `NodeId`, a
`Copy` handle. Parent and child links are handles, so a cycle is not
representable and nothing leaks. Removing an object bumps its slot's
generation, so a stale handle reports itself as stale rather than silently
reaching whatever took its place. The reasoning is in
[ADR 0001](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0001-ownership-model.md).

The other bindings hide this: an object built on its own gets a document of
its own, and putting it into another moves it. In Rust that move is explicit.
`Document::absorb` empties one document into another and hands back the map
from old handles to new ones, rewriting every link on the way, including the
ones inside metadata.

There are two deep copies, and they differ only for an object held in two
places. `Document::clone_object` is upstream's `clone()`: an object held twice
comes out as two objects, and an object that holds itself is refused with
`ObjectCycle`. `Document::deep_clone` copies an object held twice once, and
copies a cycle.

## Errors and missing values

Failure is a `Result`. `otio-core`'s `Error` displays as upstream's message,
character for character, because code in the wild matches on that text; where
upstream appends the object concerned, `Error::object` names it. Reading
errors follow upstream's reader too, including RapidJSON's message and
position for a JSON syntax error. The adapters return `otio_adapter::Error`.

"There is nothing here" is an `Option`: a document's `root()`, an item's
`source_range`. One case is an error rather than an answer: asking a bare
`Composition` where a child sits returns `Error::NoLayout`, because unlike a
track or a stack it does not say how its children are laid out.

The reader is more lenient than upstream's on purpose. A missing field or an
explicit `null` takes its default, and a schema `otio-core` does not know is
kept whole and written back out unchanged.

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="rust" -->

The ten edit operations are functions in `otio_core::edit`, each taking the
document first: `slice`, `overwrite`, `insert`, `trim`, `ripple`, `roll`,
`slip`, `slide`, `fill` and `remove`. They are upstream's `editAlgorithm`,
working in place on a track; [Editing a timeline](/docs/guides/editing) says
what each does to its neighbours.

`otio_core::algorithm` holds upstream's stack and track algorithms:
`flatten_stack` collapses a stack into the track a viewer would see, and
`track_trimmed_to_range` cuts a track down to a span. Both leave the original
untouched and leave nothing behind in the document but the answer.

## Formats

`.otio` itself is `otio-core`'s: `otio_core::from_str` reads a document and
`otio_core::to_string` writes one. OTIO files are not quite JSON, since
upstream writes `NaN` and `Inf` as bare literals, so `otio-core` carries its
own parser and writer. Older files are upgraded as they are read, following
upstream's registered upgrade functions.

Every other format is its adapter crate, and converting is a read with one
and a write with another:

<!-- ::sample id="convert-a-format" lang="rust" -->

Bundles are `otio_bundle::write_otioz`, `write_otiod` and `read_otioz`. See
[Reading and writing files](/docs/guides/reading-and-writing) for what each
format carries, and [AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles)
for those two.

## Platforms

CI builds and tests the workspace on Linux, macOS and Windows.

## Reference

- [`crates/otio-core/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-core/README.md):
  ownership, copying, editing, old files and error messages in full.
- [`crates/otio-adapter/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-adapter/README.md):
  the `Adapter` and `TextAdapter` traits.
- [The C ABI](/docs/languages/c), which is the layer every generated SDK sits
  on, and its [reference](/reference).

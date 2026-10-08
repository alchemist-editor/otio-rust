---
title: Python
summary: A drop-in replacement for upstream's opentimelineio package, built on the Rust core with PyO3.
section: Languages
order: 2
---

The Python package is called `opentimelineio`, and the name is the point: it
aims to be a drop-in replacement for upstream OpenTimelineIO's own package, so
that code written against upstream runs against this one without changes. It
is built with [PyO3](https://pyo3.rs) straight over the Rust crates rather
than over the C ABI, and the claim is measured the strongest way available:
upstream's own test files, and four of its adapter suites, are vendored into
the repository and run unmodified.

## Install

From a clone of the repository:

```sh
cd crates/otio-python
pip install .
```

`pip install .` runs [maturin](https://www.maturin.rs), which builds the Rust
extension module and packages it with the Python sources under `python/`.
Installing it also installs upstream's console tools: `otiocat`,
`otioconvert`, `otiostat`, `otiotool`, `otiopluginfo` and
`otioautogen_serialized_schema_docs`.

To run upstream's tests and the adapter suites against what you built:

```sh
python tests/run_upstream_tests.py
pip install pytest
python tests/run_adapter_tests.py
```

## Your first program

<!-- ::sample id="read-an-edl" lang="python" -->

This is upstream's code, unchanged. `otio.adapters.read_from_file` picks the
EDL adapter from the suffix, and `rate` is upstream's keyword argument for it.
The rate is the one thing to get right with an EDL; see
[CMX 3600 EDL](/docs/formats/edl).

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="python" -->

`otio.schema` has the whole object model: `Clip`, `Gap`, `Track`, `Stack`,
`Timeline`, `Transition`, `SerializableCollection`, `Marker`, `Effect`,
`TimeEffect`, `LinearTimeWarp`, `FreezeFrame`, `ExternalReference`,
`MissingReference`, `GeneratorReference`, `ImageSequenceReference`, `V2d` and
`Box2d`. `otio.opentime` has `RationalTime`, `TimeRange` and
`TimeTransform`, with upstream's module-level helpers. A composition is a
mutable sequence, slices and all.

## How objects and documents work here

Underneath, the core keeps every object in a document's arena and names it
with a handle. Upstream's API has no document in it, so the bindings
reconcile the two:

- **Each object built from Python gets a document of its own**, holding just
  it and whatever hangs off it.
- **Appending moves it.** The document it came from is left as a forwarding
  note, so every wrapper already handed out keeps working.
- **Identity is kept by hand.** `track[0] is track[0]` is `True`, as upstream's
  is: each document caches a weak reference to the wrapper it handed out for
  each node.
- **`metadata` is a view** that reads and writes through to the document, so
  `obj.metadata["k"] = v` changes the object, and a list read out of it is a
  live `AnyVector`.
- **Objects live as long as upstream's would.** An object built in Python is
  freed when its wrapper goes; a child removed from a composition lives on
  without a parent while Python still holds it.

Two things differ from upstream on purpose. Writing an object writes that
object, even when its document holds more. And a `SerializableCollection`
parents what it holds, so a clip in one reports the collection as its
`parent()`, where upstream's reports none.

Classes registered with `register_type` and `serializable_field` work as
upstream's do, including subclasses of built-ins such as `Clip`, and round-trip
through any document.

## Errors and missing values

Errors are upstream's exceptions. Every error from the core reaches Python as
the exception upstream's error handler picks (`NotAChildError`,
`CannotComputeAvailableRangeError`, `IndexError`, `NotImplementedError`,
otherwise `ValueError`), in upstream's words. The adapters raise what
upstream's adapters raise: the EDL adapter its own `EDLParseError`, ALE its
own `ALEParseError`, both FCP XML flavours plain `ValueError`. The one
improvement is that an argument no adapter knows is a `TypeError` rather than
silently ignored.

A missing value is what it is upstream, `None`: the `source_range` of an
untrimmed clip, for one.

The odd corners of upstream's pybind11 bindings are reproduced rather than
tidied, because a binding that is nearly the same surfaces its differences as
bugs in somebody else's code. Operators raise `TypeError` instead of returning
`NotImplemented`, `+=` on a time rebinds rather than mutates, and `str()` and
`repr()` format numbers with C's `%g`.

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="python" -->

`opentimelineio.algorithms` is upstream's module, with its filter, stack,
track and timeline functions. It also has the ten edit operations,
`overwrite`, `insert`, `trim`, `slice`, `slip`, `slide`, `ripple`, `roll`,
`fill` and `remove`, with a `ReferencePoint` enum. Upstream's Python does not
bind those; their names, parameters and defaults follow upstream's C++
`editAlgorithm.h`. [Editing a timeline](/docs/guides/editing) walks through
them.

## Formats

Files are read and written through `opentimelineio.adapters`, as upstream
reads and writes them: `read_from_file`, `read_from_string`, `write_to_file`,
`write_to_string` and the rest. Behind them are `otio_json`, `cmx_3600`,
`ale`, `fcp_xml`, `fcpx_xml` and `AAF`, under upstream's names and suffixes,
each keeping the keyword arguments, defaults and exception types of the
upstream adapter it stands in for. The `.otioz` and `.otiod` bundle adapters
are there too.

AAF is the adapter held closest to upstream: reading matches upstream's
adapter byte for byte, and writing, given the same times and identifiers,
writes the file upstream's adapter writes.

<!-- ::sample id="write-an-aaf" lang="python" -->

Upstream's plugin system is ported as well. Manifests are loaded from
`OTIO_PLUGIN_MANIFEST_PATH` and from installed packages' entry points, and the
formats written in Rust are declared as plugins themselves, so a third-party
adapter, media linker, hook or schemadef composes with them as it would
upstream. A media linker named with `media_linker_name`, or set as
`OTIO_DEFAULT_MEDIA_LINKER`, runs on every clip a read returns; hooks run at
`post_adapter_read`, `post_media_linker`, `pre_adapter_write` and
`post_adapter_write` with `hook_function_argument_map`; and a schemadef's
module is loaded the first time `otio.schemadef.<name>` is reached. The AAF
adapter's own four hooks run too, as described under [AAF](/docs/formats/aaf).

See [Reading and writing files](/docs/guides/reading-and-writing),
[AAF](/docs/formats/aaf) and [Bundles](/docs/formats/bundles).

## Platforms

CI builds the package and runs the tests on Linux, macOS and Windows.

## Reference

- [`crates/otio-python/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-python/README.md):
  the upstream test results file by file, every adapter's behaviour, and what
  makes the object model harder to bind than `opentime`.
- [Getting started](/docs/getting-started) for building the core alongside it.

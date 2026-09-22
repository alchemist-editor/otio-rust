# otio-python

Python bindings for the Rust core, built with [PyO3](https://pyo3.rs).

The package they build is called `opentimelineio`, and the name is the point:
the aim is a drop-in replacement for upstream OpenTimelineIO's Python package,
so that code written against upstream runs against this one without changes.
That is a strong claim, so it is measured the strongest way available —
upstream's own test files are vendored in [`tests/upstream`](tests/upstream)
and run unmodified.

| Upstream test file | Result |
| --- | --- |
| `test_opentime.py` | 83 of 83 passing |
| `test_composable.py` | 4 of 4 passing |
| `test_effect.py` | 8 of 8 passing |
| `test_media_reference.py` | 5 of 5 passing |
| `test_generator_reference.py` | 4 of 4 passing |
| `test_image_sequence_reference.py` | 24 of 24 passing |
| `test_clip.py` | 8 of 8 passing |
| `test_item.py` | 18 of 18 passing |
| `test_track.py` | 5 of 5 passing |
| `test_transition.py` | 5 of 5 passing |
| `test_timeline.py` | 16 of 16 passing |
| `test_serializable_collection.py` | 8 of 8 passing |
| `test_serializable_object.py` | 15 of 16 passing; 1 skipped by upstream itself |
| `test_marker.py` | 9 of 9 passing |
| `test_unknown_schema.py` | 3 of 3 passing |
| `test_json_backend.py` | 16 of 16 passing |
| `test_core.py` | 2 of 3 passing; 1 is Windows-only and skipped elsewhere |
| `test_cxx_sdk_bindings.py` | 1 of 1 passing |
| `test_adapter_plugin.py` | 13 of 13 passing |
| `test_hooks_plugins.py` | 11 of 11 passing |
| `test_media_linker.py` | 7 of 7 passing |
| `test_plugin_detection.py` | 6 of 6 passing |
| `test_builtin_adapters.py` | 6 of 6 passing |
| `test_otiod.py` | 1 of 1 passing |
| `test_otioz.py` | 1 of 1 passing |
| `test_schemadef_plugin.py` | 3 of 3 passing |
| `test_version_manifest.py` | 6 of 6 passing |
| `test_console.py` | 72 of 72 passing |
| `test_serialized_schema.py` | 3 of 3 passing |
| `test_url_conversions.py` | 3 of 3 passing |
| `test_filter_algorithms.py` | 15 of 15 passing |
| `test_stack_algo.py` | 10 of 10 passing |
| `test_track_algo.py` | 6 of 6 passing |
| `test_timeline_algo.py` | 5 of 5 passing |
| `test_composition.py` | 46 of 46 passing |
| `test_examples.py` | 1 of 1 passing |
| `test_documentation.py` | 1 of 1 passing |
| `test_v2d.py` | 9 of 9 passing |
| `test_box2d.py` | 5 of 5 passing |
| `test_color.py` | 6 of 6 passing |
| `test_core_utils.py` | 6 of 6 passing |
| `test_multithreading.py` | 4 of 4 passing |

Upstream's file-format adapters are separate repositories with suites of their
own, and four of them are vendored in [`tests/adapters`](tests/adapters) and
run unmodified too. They drive the adapters the way a user does, through
`otio.adapters.read_from_file`, adapter names, keyword arguments and exception
types.

| Upstream adapter suite | Result |
| --- | --- |
| `otio-ale-adapter` | 8 of 8 passing |
| `otio-cmx3600-adapter` | 42 of 42 passing |
| `otio-fcpx-xml-adapter` | 5 of 5 passing |
| `otio-fcp-adapter` | 9 of 9 passing; 30 more left out |

The 30 left out of the FCP 7 suite test upstream's Python implementation from
the inside, through private helpers such as `_Context` and `FCP7XMLParser`.
The format is read and written in Rust here, so there is nothing for them to
reach; `otio-fcp7` ports the behaviour they pin. The AAF suite is not vendored:
most of it tests writing, and the rest needs pyaaf2 and 36 MB of fixtures.
AAF is instead checked in [`tests/bindings`](tests/bindings): reading against
the baseline upstream's own adapter produced from the same file, and writing
against the files upstream's adapter wrote, both byte for byte.

Tests that cannot pass are deselected by the runner from one file per module
under [`tests/excluded`](tests/excluded), each with its reason. None is left
there now.

Alongside them, [`tests/bindings`](tests/bindings) covers what these bindings
have to do that upstream's C++ does not: moving an object from one document
into another when it is appended to something, and finding adapters without a
plugin manifest.

## What is bound so far

`opentimelineio.opentime`, in full: `RationalTime`, `TimeRange` and
`TimeTransform`, with the module-level helpers (`from_frames`, `to_timecode`
and the rest) carried over from upstream's `opentime.py` as-is.

The whole object model. `opentimelineio.core` has `SerializableObject`,
`SerializableObjectWithMetadata`, `Composable`, `Item`, `Composition`,
`MediaReference` and `Color`; `opentimelineio.schema` has `Clip`, `Gap`,
`Track`, `Stack`, `Timeline`, `Transition`, `SerializableCollection`,
`Marker`, `Effect`, `TimeEffect`, `LinearTimeWarp`, `FreezeFrame`,
`ExternalReference`, `MissingReference`, `GeneratorReference`,
`ImageSequenceReference`, `V2d` and `Box2d`.

A composition is a mutable sequence, slices and all. `metadata`, a
generator's `parameters` and a registered type's dynamic fields are upstream's
`AnyDictionary`, and a list read out of one is a live `AnyVector`, so
`obj.metadata["k"].append(x)` changes the object at any depth of nesting.
`effects` and `markers` are sequences that write through, and `deepcopy`,
`copy` and `clone` copy an object and everything it owns. `V2d` and `Box2d`
have Imath's arithmetic, as upstream's do.
`opentimelineio.exceptions` carries upstream's four extension-defined
exception types and the dozen Python-defined ones built on them.
`opentimelineio.adapters.otio_json` reads and writes any object, and — as
upstream's does — any list or plain value as well.

The adapters. `opentimelineio.adapters` has upstream's `read_from_file`,
`read_from_string`, `write_to_file`, `write_to_string`, `from_filepath`,
`from_name`, `available_adapter_names` and `suffixes_with_defined_adapters`,
and `opentimelineio.plugins.ActiveManifest()` answers as upstream's does.
Behind them are `otio_json`, `cmx_3600`, `ale`, `fcp_xml`, `fcpx_xml` and
`AAF`, under upstream's names and suffixes, each a module keeping the
functions, keyword arguments, defaults and exception types of the upstream
adapter it stands in for, over the Rust crate that implements it. See
[Adapters](#adapters).

The algorithms. `opentimelineio.algorithms` is upstream's module, with its
filter, stack, track and timeline functions; `flatten_stack` and
`track_trimmed_to_range` run in `otio-core`. It also has the ten edit
operations, `overwrite`, `insert`, `trim`, `slice`, `slip`, `slide`, `ripple`,
`roll`, `fill` and `remove`, with a `ReferencePoint` enum. Upstream's Python
does not bind those; their names, parameters and defaults follow upstream's
C++ `editAlgorithm.h`, and their errors are upstream's error handler's.

Types defined in Python. `opentimelineio.core` has upstream's
`register_type`, `serializable_field`, `deprecated_field`, upgrade and
downgrade functions, `type_version_map`, `release_to_schema_version_map`, and
writing with `schema_version_targets`. See "Schemas registered from Python"
below for how they are held.

Upstream's plugin system, ported from its Python: manifests from
`OTIO_PLUGIN_MANIFEST_PATH` and from packages' `opentimelineio.plugins` entry
points, adapters, media linkers (`OTIO_DEFAULT_MEDIA_LINKER`), hook scripts,
schemadefs and version manifests. The `.otioz` and `.otiod` bundle adapters
call `_otio.bundle`, which is the [`otio-bundle`](../otio-bundle) crate. The
console tools install as upstream's do: `otiocat`, `otioconvert`, `otiostat`,
`otiotool`, `otiopluginfo` and `otioautogen_serialized_schema_docs`.
`opentimelineio.url_utils` is upstream's, over the same URL decoding the
bundles use.

## Adapters

Upstream finds adapters through JSON plugin manifests, so that a third party
can ship one as a Python package, and so does this package: `plugins/` is
upstream's Python. The formats written in Rust are declared the same way, in
[`plugin_manifest.json`](python/opentimelineio/plugin_manifest.json), loaded
right after upstream's own `builtin_adapters.plugin_manifest.json`, so a
third-party manifest, hook or media linker composes with them as it would
upstream. Each is a Python module holding upstream's function signatures over
a pair of functions in [`src/adapters.rs`](src/adapters.rs). Code that calls
`otio.adapters.read_from_file("cut.edl", rate=24)` does not see the
difference.

Where upstream's own adapters disagree, the modules disagree the same way. The
EDL adapter raises its own `EDLParseError` and ALE its own `ALEParseError`;
both FCP XML flavours raise plain `ValueError`. ALE takes its name column
through `**adapter_argument_map` as `ale_name_column_key` rather than as a
parameter. The one improvement is that an argument no adapter knows is a
`TypeError` rather than silently ignored.

Three things differ, each on purpose:

- **AAF does not embed essence.** Reading runs upstream's passes with
  upstream's defaults and matches its adapter byte for byte, with `simplify`
  and `attach_markers` on or off; `bake_keyframed_properties` bakes as
  upstream does, and `transcribe_log` prints what upstream prints, through
  Python's `print` once the read is done. Writing takes upstream's
  `prefer_file_mob_id`, `use_empty_mob_ids` and `create_edgecode` and,
  given the same times and random identifiers, writes the file upstream's
  adapter writes, byte for byte; the tests replay the ones recorded when
  upstream wrote each fixture. `embed_essence=True` raises
  `NotImplementedError`, since importing the media needs decoding it
  ([#66](https://github.com/alchemist-editor/otio-rust/issues/66)), and the
  file is only created once the whole AAF has been built.
- **Writing an object writes that object.** An object built in Python lives
  in a document that can hold more than it — the timeline a track sits in —
  so the writer is pointed at the object for the length of the write.
- **A `SerializableCollection` parents what it holds.** Upstream's does not,
  so a clip in an upstream collection reports no `parent()`. Here the
  collection is its parent, as a composition is: an object can sit in one
  place, and the arena needs to know which. The FCP 7, ALE and AAF readers all
  return collections, and none of their suites notice.

## Building

```sh
pip install .
python tests/run_upstream_tests.py
pip install pytest
python tests/run_adapter_tests.py
```

`pip install .` runs [maturin](https://www.maturin.rs), which builds the Rust
extension module and packages it with the Python sources under `python/`.

## Imitating upstream on purpose

A binding that is *nearly* the same is worse than one that is obviously
different, because the difference surfaces as a bug in somebody else's code
rather than as an import error. So the odd corners of upstream's pybind11
bindings are reproduced here rather than tidied:

- **Operators raise `TypeError` instead of returning `NotImplemented`.**
  `time < -1` is an error, not `False`. Upstream type-checks every operand by
  hand and its tests pin the behaviour.
- **`+=` does not mutate.** A time is a value; mutating one in place would
  reach every other name bound to it. Upstream builds a new object and
  rebinds. There is deliberately no `__iadd__` here, so Python falls back to
  `__add__` and rebinds, which is the same thing by a shorter route.
- **`str()` and `repr()` are formatted with C's `%g`.** `opentime::cfmt` is
  public for this reason: a binding that prints `100000000000000000000` where
  upstream prints `1e+20` is not a drop-in replacement.
- **`TimeRange` takes either two times or a start, a duration and a rate.**
  Upstream offers both and its tests use both.
- **Error messages are upstream's, character for character.** They reach
  Python as the text of a `ValueError`, and one upstream test compares one of
  them exactly. That made them part of the observable behaviour, so
  `opentime`'s own error type now renders upstream's wording — including the
  two spaces in `"Frame rate mismatch.  Timecode ..."`.

## What makes the rest harder

Binding `opentime` was mechanical, because a `RationalTime` is a value with no
identity. The object model is not. Three of the four problems it raised are
now settled, and the reasoning is in [`src/arena.rs`](src/arena.rs) where the
code that acts on it lives.

**A free-floating object has no document.** In upstream, `Clip("a")` exists on
its own and is appended to a track later. *Settled:* each object built from
Python gets a `Document` of its own, holding just it and whatever hangs off
it. The alternative, one document per interpreter, was rejected because it
never gets smaller — every object anyone builds would stay alive until the
process exits — and because it lets two unrelated timelines share a pool.

Appending therefore moves the object, which `Document::absorb` does, and the
document it came from is left as a forwarding note: where its contents went,
and the map from its old handles to the new ones. Every wrapper already handed
out keeps working, because each resolves its handle through that chain before
touching it. Updating each wrapper's handle in place instead would work for
the object being appended and quietly fail for everything under it.

**Object identity has to be maintained by hand.** Upstream's `track[0] is
track[0]` is `True`. *Settled:* each document caches a weak reference to the
Python wrapper it handed out for each node, and hands the same one back. The
reference is weak on purpose: a strong one makes a cycle running from the
wrapper through the cache back to the wrapper, and Python's collector cannot
see through Rust to break it.

**Mutation needs a short borrow.** *Settled:* every method takes the borrow,
does its work and drops it before returning to Python, and `metadata` is a
view that reads and writes through to the document rather than a copy of
it — which is what makes upstream's `obj.metadata["k"] = v` change the
object. Holding a borrow across a call back into Python would deadlock the
moment that code touched the same document.

**Objects live as long as upstream's would.** *Settled:* an object built in
Python owns its document and is freed when its wrapper goes. An object owned
by another is kept alive by its document, through a keeper Python's collector
can see. Removing a child frees it at once unless Python still holds it, in
which case it lives on without a parent, as it would upstream. An edit
operation that takes objects out of a track leaves them the same way.

**Schemas registered from Python.** *Settled:* a class registered with
`register_type` is held in `otio-core` as a dynamic object, a schema name, a
version and a map of fields, which is what upstream's C++ does with a type
defined in Python. Python keeps a table from schema name to class and wraps
such a node in its class whenever it reaches Python, and `serializable_field`
reads and writes the field map. The core never holds a Python object, so a
registered type round-trips through any document and any binding, and
unregistered ones still read as `UnknownSchema`. Subclassing a concrete
built-in such as `Clip` raises `NotImplementedError`: only
`SerializableObject` and `SerializableObjectWithMetadata` can be subclassed.

Upgrade and downgrade functions live in one registry in `otio-core`, keyed by
schema and version, holding the built-in steps and any registered from
Python. Reading runs the upgrades; writing with `schema_version_targets`
runs the downgrades, innermost object first.

One more thing worth knowing:

- **`otio-core`'s errors are upstream's too.** Every one reaches Python as
  the exception upstream's `ErrorStatusHandler` picks (`NotAChildError`,
  `CannotComputeAvailableRangeError`, `IndexError`, `NotImplementedError`,
  otherwise `ValueError`), in upstream's words and ending, as upstream's do,
  in `: ` and the object's `str()`. Errors reading JSON give RapidJSON's
  message and position, or name the object being decoded by its C++ type as
  a Linux GCC or Clang build of upstream spells it.

Two upstream behaviours reproduced here that look like bugs, because they are:

- **`Color.to_agbr_integer` and `Color.from_agbr_int` disagree.** One writes
  blue at bits 16-23 and green at 8-15; the other reads them the other way
  round, so a round trip through the packed form swaps green and blue.
  Anything that went through upstream carries what upstream produced, so
  matching it is the only way to agree with every other reader.
  `otio-core/tests/color.rs` pins it.
- **`Track.available_image_bounds` does not descend and `Stack`'s does.** A
  track unions the bounds of the clips sitting directly on it; a stack unions
  every clip below it. `otio-core/tests/image_bounds.rs` pins both.

## Where the containers differ from upstream

Upstream's `AnyDictionary` and `AnyVector` wrap a C++ container. Here they are
views onto a place in a document, and three things follow from that:

- A view finds its container by its path from the object that owns it. A view
  of an entry that has since been replaced sees the new entry, where upstream's
  reports that the container was destroyed.
- A `metadata` view keeps its object alive, so `read_from_string(s).metadata`
  goes on working after the timeline is gone. Upstream's does not.
- The top-level result of `deserialize_json_from_string` is a plain `list` or
  `dict`, where upstream's is a free-standing `AnyVector` or `AnyDictionary`.

## License

Apache-2.0, matching upstream OpenTimelineIO.

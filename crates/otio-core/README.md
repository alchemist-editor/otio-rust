# otio-core

The OpenTimelineIO data model, and JSON in and out of it.

A Rust port of upstream OpenTimelineIO's C++ core: the timeline object tree
(`Timeline`, `Stack`, `Track`, `Clip`, `Gap`, `Transition`, markers, effects,
media references), the `AnyDictionary` metadata type, and readers and writers
for the `.otio` format.

```rust
use otio_core::Node;

let json = std::fs::read_to_string("cut.otio")?;
let document = otio_core::from_str(&json)?;

let root = document.root().expect("a parsed document has a root");
if let Node::Timeline(timeline) = document.try_get(root)? {
    println!("{}", timeline.base.name);
}

let rewritten = otio_core::to_string(&document)?;
```

## Ownership

Upstream's C++ core uses intrusive reference counting plus raw parent
back-pointers, which is a reference cycle. This crate stores every object in a
generational arena inside a `Document` and refers to them by `NodeId`, a
`Copy` handle. Parent and child links are handles, so a cycle is not
representable and nothing leaks. Removing an object bumps its slot's
generation, so a stale handle reports itself as stale rather than silently
reaching whatever took its place.

The reasoning is written up in
[ADR 0001](../../docs/adr/0001-ownership-model.md).

## OTIO files are not quite JSON

Upstream parses with RapidJSON's `kParseNanAndInfFlag` and writes with
`kWriteNanAndInfFlag`, so `NaN`, `Inf`, `Infinity` and their negatives appear
in real files as bare literals — upstream's own `big_int.otio` sample contains
all three, along with a 256-bit integer. A strict JSON library rejects those
files, so this crate carries its own parser and writer in
[`json`](src/json.rs). The upshot is that `otio-core` depends on nothing but
`opentime`.

## Where things sit in time

`otio-core` answers the questions every adapter and editing tool asks of a
timeline: how long is this, where does it sit in its parent, what is under the
playhead, what restates this time in that item's clock. A clip's `source_range`
is stated in its media's time, its place on a track in the track's, and the
track's place in a stack in the stack's, so most of these need a walk up or
down that chain. [`composition`](src/composition.rs) does that walking, and
also holds the editing operations — inserting, removing and reparenting
children, and deep-copying a subtree.

[`algorithm`](src/algorithm.rs) builds on it with upstream's `stackAlgorithm`
and `trackAlgorithm`:

- `flatten_stack` collapses a stack of tracks into the single track a viewer
  would see, with gaps and disabled items letting lower tracks show through.
- `track_trimmed_to_range` cuts a track down to a span, dropping what falls
  outside and pulling in whatever straddles an edge.

Both leave the original untouched and clean up after themselves: whatever an
algorithm builds along the way is removed again, so the only thing left in the
document is the answer.

## Editing

[`edit`](src/edit.rs) is upstream's `editAlgorithm`: the ten operations an
NLE's timeline offers, working in place on a track.

- `slice` cuts an item in two at a time.
- `overwrite` drops an item over a span, splitting or shortening whatever was
  there; `insert` pushes what follows along instead.
- `trim`, `ripple` and `roll` move an edit point: `trim` leaves a hole,
  `ripple` slides everything after it, `roll` moves the cut between two
  neighbours without changing the track's length.
- `slip` moves which part of its media an item shows without moving the item;
  `slide` moves the item without changing what it shows.
- `fill` drops an item into a gap, and `remove` takes one out.

`fill` takes a reference point that decides what happens when the media and
the gap are different lengths. `Source` uses the media as it is and lets the
track grow or a remnant of the gap stay. `Sequence` trims the media to the part
of the gap it lines up with, so the track keeps its length. `Fit` hangs a
`LinearTimeWarp` off the item at the ratio of gap to media — and, as upstream
does, leaves the item's own length alone, because nothing in the track layout
reads that warp. The last one is worth knowing about before it surprises you:
fitting a 35-frame clip into a 30-frame gap still occupies 35 frames of track.

Two upstream quirks are reproduced deliberately, both pinned by tests that say
so. A slice one frame past the last frame of a track reports "not an item"
rather than doing nothing quietly, though a slice on the first frame does
nothing quietly; and a cut that lands where a transition is the child found
first is refused for the same reason instead of reaching past it.

One upstream refusal is not reproduced. `slice`, `insert`, `overwrite` and
`fill` copy an item, and an item whose metadata holds itself is copied here
with the cycle intact, the copy holding itself. Upstream makes that copy with
`clone()`, which goes through its JSON writer and so refuses the cycle with
`OBJECT_CYCLE`; three of the four refuse only after changing the track,
leaving the item cut short and the rest of it gone. That is a limit of how
upstream copies rather than anything about the edit, so the edit is allowed.
Writing the result as JSON is still refused, as upstream refuses it.

## The base classes

Upstream's five base classes — `SerializableObject`,
`SerializableObjectWithMetadata`, `Composable`, `Composition` and
`MediaReference` — are schemas in their own right, not just C++ base classes,
and a file may legitimately contain one: upstream's own smallest object-model
test builds an `otio.core.Composable` directly. So each of them is a `Node`
variant here too, and round-trips.

A bare `Composition` is the odd one. It holds children, but unlike a `Track`
(which lays them end to end) or a `Stack` (which starts them together) it does
not say where they sit, so asking for a child's range returns `Error::NoLayout`
rather than a made-up answer. Upstream reports the same case as
`NOT_IMPLEMENTED` from `Composition::range_of_child_at_index`. Everything that
does not need a layout — adding, removing and listing children — works.

## Moving objects between documents

`Document::absorb` empties one document into another and hands back the map
from old handles to new ones. Every link inside the moved objects is rewritten
on the way, including the ones hiding in metadata, which may hold whole
objects; `Node::visit_links_mut` is the single place that knows where a
`NodeId` can be, so nothing has to re-derive that list and miss one.

This is what an arena needs and reference counting does not. The Python
bindings hit it on the first line of anything real: two objects built
separately live in separate documents, so putting one inside the other means
moving it rather than pointing at it.

## Old files

A field can move between schema versions, and reading an older file has to
follow it rather than quietly dropping it. The upgrades in
[`upgrade`](src/upgrade.rs) mirror upstream's registered upgrade functions:

- `Clip.1`'s `media_reference` becomes `media_references["DEFAULT_MEDIA"]`.
- `Marker.1`'s `range` becomes `marked_range`.
- `Marker.2`'s colour name becomes a `Color.1`.
- `Filler` reads as `Gap`, and `Sequence` as `Track`.
- `SerializeableCollection`, a misspelling an old release wrote, reads as
  `SerializableCollection`.

A schema this crate does not know is kept whole, tag and version included, and
written back out unchanged, so a third-party plugin's objects survive a trip
through a tool built on this crate.

## Error messages

Each `Error` displays as upstream's message, character for character. Upstream
reports a failure as an outcome and some details, and its Python bindings raise
the text as an exception; code in the wild matches on that text, so it counts
as observable behaviour. Where upstream appends `": "` and the `str()` of the
object concerned, `Error::object` names that object, and a binding that can
print it adds it. The Python bindings do.

Reading errors follow upstream's reader too:

- **JSON syntax.** RapidJSON's message and position, such as `Missing a comma
  or '}' after an object member. (line 3, column 2)`. The column is the number
  of bytes read on that line.
- **Decoding.** Upstream names the innermost object it was decoding: `While
  reading object named 'shot' (of type 'N14opentimelineio5v0_194ClipE'): ...
  (near line 100)`, where the line is that object's closing brace. Inside a
  value type such as a `TimeRange`, only the line is given.
- **References.** Two objects declaring the same `OTIO_REF_ID` are refused,
  as upstream refuses them, with `Duplicated object reference while reading:
  near line 12`, the line being the second object's closing brace. A reference
  could not otherwise say which of the two it meant. An empty id declares
  nothing and may repeat.
- **C++ type names** are spelled as a GCC or Clang build on Linux spells them,
  for 0.19's `v0_19` namespace and Imath 3.2. A macOS build of upstream says
  `x` for `int64_t`, and MSVC does not mangle names at all, so the same file
  gives slightly different text depending on where upstream was built.

The reader stays more lenient than upstream's, on purpose, because files
written by older versions leave fields out. A missing field or an explicit
`null` takes its default where upstream raises `KeyError` or a type mismatch.
A schema this crate does not know is kept wherever it appears, even among a
track's children, where upstream refuses it. A negative schema version is
refused as malformed where upstream crashes. So a file gets an error here only
if upstream would refuse it too, and the error then has upstream's wording. The
one exception is a document whose root is not an object: upstream reads it as a
plain value, but `from_str` reads documents, so it refuses one.

## What it is measured against

`tests/composition.rs`, `tests/algorithm.rs` and `tests/edit.rs` are ported
from upstream's own `test_composition.py`, `test_track_algo.py`,
`test_stack_algo.py` and `test_editAlgorithm.cpp`, keeping upstream's fixture
names and its exact ranges so a failure can be read against the test it came
from.

`tests/round_trip.rs` runs every sample document from upstream 0.19.0's
`tests/sample_data`. The bar is:

1. every file parses;
2. serialization is idempotent — parse, write, parse, write gives identical
   bytes, so a file does not drift each time a tool touches it;
3. nothing the input says is missing from the output.

Byte-for-byte equality with the *input* is deliberately not the bar, and
upstream does not clear it either: its own baseline tests compare parsed JSON
rather than bytes, because a newer release writes fields an older file does
not carry.

## License

Apache-2.0, matching upstream OpenTimelineIO.

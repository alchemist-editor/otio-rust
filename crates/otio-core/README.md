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

## Old files

A field can move between schema versions, and reading an older file has to
follow it rather than quietly dropping it. The upgrades in
[`upgrade`](src/upgrade.rs) mirror upstream's registered upgrade functions:

- `Clip.1`'s `media_reference` becomes `media_references["DEFAULT_MEDIA"]`.
- `Marker.1`'s `range` becomes `marked_range`.
- `Marker.2`'s colour name becomes a `Color.1`.
- `Filler` reads as `Gap`, and `Sequence` as `Track`.

A schema this crate does not know is kept whole, tag and version included, and
written back out unchanged, so a third-party plugin's objects survive a trip
through a tool built on this crate.

## What it is measured against

`tests/composition.rs` and `tests/algorithm.rs` are ported from upstream's own
`test_composition.py`, `test_track_algo.py` and `test_stack_algo.py`, keeping
upstream's fixture names so a failure can be read against the test it came
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

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

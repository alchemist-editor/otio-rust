# otio-rust

A Rust implementation of [OpenTimelineIO](https://opentimeline.io), the
interchange format for editorial timeline information, and of the time math
library beneath it.

The goal is a pure-Rust core — no C++ dependency, no Python dependency — with
language bindings and SDKs built on top of it.

## Status

Early. The workspace currently contains:

| Crate | What it is | State |
|---|---|---|
| [`opentime`](crates/opentime) | Rational time, time ranges, SMPTE timecode | Ported, with upstream's test suite passing |
| [`otio-core`](crates/otio-core) | The timeline data model and `.otio` serialization | Ported, round-tripping upstream's sample documents |
| [`aaf`](crates/aaf) | The AAF file format, a port of `pyaaf2` | Reading the container and the object tree, checked against upstream |

Still to come, in roughly this order: the editing algorithms, Python bindings
via PyO3, then the file format adapters (ALE, CMX 3600 EDL, FCP 7 XML, FCP X
XML, and AAF), and a C ABI.

AAF is much the longest item on that list, and it shares no code with the
others, so the `aaf` crate is being built alongside them rather than after.

## Compatibility

This is a port, not a reimplementation with its own ideas. Files written here
are meant to open unchanged in every existing OpenTimelineIO tool, so the
arithmetic, rounding and timecode behaviour follow upstream exactly.

Where a port knowingly differs from upstream, the difference is documented on
the item itself and is always in the direction of accepting more input, never
of producing different output for input upstream accepts.

## Design decisions

Decisions that shape the whole project are recorded in [docs/adr](docs/adr).

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## License

Apache-2.0, matching upstream OpenTimelineIO.

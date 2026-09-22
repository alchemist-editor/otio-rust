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
| [`otio-adapter`](crates/otio-adapter) | The trait every file-format adapter implements | Written, with ALE as its first implementation |
| [`otio-ale`](crates/otio-ale) | Avid Log Exchange (ALE) | Ported, round-tripping upstream's sample files byte for byte |
| [`otio-cmx3600`](crates/otio-cmx3600) | CMX 3600 Edit Decision Lists | Ported, with upstream's test suite as the measure |
| [`otio-xml`](crates/otio-xml) | A small XML tree and parser, for the XML adapters | Written |
| [`otio-fcp7`](crates/otio-fcp7) | Final Cut Pro 7 interchange XML | Ported, round-tripping upstream's sample files |
| [`otio-fcpx`](crates/otio-fcpx) | Final Cut Pro X XML | Ported, round-tripping upstream's sample files |
| [`aaf`](crates/aaf) | The AAF file format, a port of `pyaaf2` | Reading the container and the object tree, checked against upstream |
| [`otio-aaf`](crates/otio-aaf) | AAF read as OpenTimelineIO | Reading the structural cases; upstream's follow-up passes and writing still to come |
| [`otio-capi`](crates/otio-capi) | The C ABI, `libotio`, with a generated header | Complete over the core, proven by a linked C program |
| [`otio-python`](crates/otio-python) | Python bindings, via PyO3 | `opentime` bound, with upstream's `test_opentime.py` passing unmodified |
| [`otio-sdk-model`](crates/otio-sdk-model) | A description of the C ABI, read out of its own source | Written, emitted as [`sdk/api.json`](sdk/api.json) |
| [`otio-sdk-gen`](crates/otio-sdk-gen) | Generates a language SDK from that description | Written, with Go as its first target |

Beyond the crates, [`sdk/`](sdk) holds the generated language SDKs — see
[`sdk/README.md`](sdk/README.md).

Still to come: the rest of the object model in Python, AAF's remaining
reading passes and AAF writing, and the other SDK targets.

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
cargo +1.85.0 check --workspace
```

The Go and Zig SDKs are built and tested separately, because each needs its
own toolchain and a built C library:

```sh
cargo build -p otio-capi --release

cp target/release/libotio.a sdk/go/lib/
cd sdk/go && go test ./...

cp target/release/libotio.a sdk/zig/lib/
cd sdk/zig && zig build test
```

The generated SDKs themselves are checked in, and `cargo test -p otio-sdk-gen`
fails if they no longer match what the C ABI says they should be. Regenerate
them with `cargo run -p otio-sdk-gen`.

The Python bindings are built and tested separately too, because they need a
Python interpreter:

```sh
cd crates/otio-python
pip install .
python tests/run_upstream_tests.py
```

That last one is the one people forget. The crate's minimum supported Rust
version is 1.85, and a recent toolchain will happily accept syntax that is
newer than that — let-chains, stable since 1.88, are the easy trap. CI checks
it, but checking locally saves a round trip.

## License

Apache-2.0, matching upstream OpenTimelineIO.

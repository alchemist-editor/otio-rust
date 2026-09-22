# Baseline generators

Two scripts, one for each direction.

[`gen_otio.py`](gen_otio.py) produces the `*.otio.json` files in the
directory above, by reading each AAF with upstream's own Python adapter.

```
python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter
```

[`gen_written.py`](gen_written.py) produces the files in
[`../written`](../written), by writing timelines as AAF with upstream's
adapter and recording every time, identifier and user name it asked for.
Run `gen_otio.py` first: the samples are written from its baselines, and
`gen_written.py` stops if one is stale. It imports `gen_otio.py` for the
upgrade to OTIO 0.19, so the two stay beside each other.

```
python3 gen_written.py ~/src/pyaaf2 ~/src/otio-aaf-adapter
```

It re-runs itself with `TZ=UTC`, because the adapter reads a new marker's
date as local time and then converts it to seconds since the epoch, and
with `PYTHONHASHSEED=0`, and it pins the platform pyaaf2 records to
`linux`, so its output is the same on every machine.

Both take a [`pyaaf2`][pyaaf2] checkout and an [`otio-aaf-adapter`][adapter]
checkout, and need `opentimelineio` 0.18 installed. Re-run them only when the
pinned revisions change or a fixture is added; running them should otherwise
leave every file byte-for-byte as it was.

## Checking the whole corpus

Only eleven of upstream's 37 sample files are vendored. To check the port
against all of them, write their baselines somewhere else and compare with
the crate's `aaf2otio` example, which prints what this crate reads, and with
`--log`, what it logs on standard error:

```
python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter --all /tmp/aaf-baselines
cargo build --release -p otio-aaf --example aaf2otio
out=/tmp/aaf-baselines
for aaf in ~/src/otio-aaf-adapter/tests/sample_data/*.aaf; do
  name=$(basename "$aaf" .aaf)
  run() { target/release/examples/aaf2otio "$@" "$aaf"; }
  run --structural | cmp -s - "$out/$name.structural.otio.json" || echo "structural: $name"
  run | cmp -s - "$out/$name.otio.json" || echo "default: $name"
  baked="$out/$name.baked.otio.json"; [ -f "$baked" ] || baked="$out/$name.otio.json"
  run --bake | cmp -s - "$baked" || echo "baked: $name"
  run --structural --log 2>&1 >/dev/null | cmp -s - "$out/$name.structural.log" || echo "structural log: $name"
  run --log 2>&1 >/dev/null | cmp -s - "$out/$name.log" || echo "log: $name"
done
```

At the pinned revisions this prints nothing on Linux: all 37 match, read
both ways, baked, and logged both ways.

Writing is checked the same way. `gen_written.py --all DIR` writes every
sample upstream's adapter can write back into `DIR`, with the timeline it
wrote from beside each, and the ignored test in `tests/write.rs` checks them
all:

```
python3 gen_written.py ~/src/pyaaf2 ~/src/otio-aaf-adapter --all /tmp/aaf-written
OTIO_AAF_WRITTEN_ALL=/tmp/aaf-written cargo test -p otio-aaf --test write -- --ignored
```

Upstream writes 33 of the 37. It refuses four: `2997fps-DFTC` mixes rates,
`avid_data_track_example` has a data track, and `empty` and
`multiple_top_level_mobs` read as collections rather than timelines. At the
pinned revisions all 33 are written byte for byte as upstream writes them,
and read back as upstream reads its own.

## Why a Python script sits in a crate that runs no Python

The same reason the `aaf` crate has them, and the same distinction applies:
this is a developer tool, not a dependency. Nothing in `cargo build` or
`cargo test` touches it, the crate has no dependencies outside this workspace,
and it builds and passes on a machine with no Python on it. What the script
produces is checked in, and the tests read the checked-in files.

The alternative is writing the expected timelines by hand, which would mean
deciding by hand what upstream produces — and the whole point of a baseline is
that upstream decides.

[pyaaf2]: https://github.com/markreidvfx/pyaaf2
[adapter]: https://github.com/OpenTimelineIO/otio-aaf-adapter

# Baseline generator

The script that produces the `*.otio.json` files in the directory above, by
reading each AAF with upstream's own Python adapter.

```
python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter
```

It takes a [`pyaaf2`][pyaaf2] checkout and an [`otio-aaf-adapter`][adapter]
checkout, and needs `opentimelineio` 0.18 installed. Re-run it only when the
pinned revisions change or a fixture is added; running it should otherwise
leave every baseline byte-for-byte as it was.

## Checking the whole corpus

Only ten of upstream's 37 sample files are vendored. To check the port
against all of them, write their baselines somewhere else and compare with
the crate's `aaf2otio` example, which prints what this crate reads:

```
python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter --all /tmp/aaf-baselines
cargo build --release -p otio-aaf --example aaf2otio
for aaf in ~/src/otio-aaf-adapter/tests/sample_data/*.aaf; do
  name=$(basename "$aaf" .aaf)
  target/release/examples/aaf2otio --structural "$aaf" |
    cmp -s - "/tmp/aaf-baselines/$name.structural.otio.json" || echo "structural: $name"
  target/release/examples/aaf2otio "$aaf" |
    cmp -s - "/tmp/aaf-baselines/$name.otio.json" || echo "default: $name"
done
```

At the pinned revisions this prints nothing: all 37 match, both ways.

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

# Baseline generator

The script that produces the `*.otio.json` files in the directory above, by
reading each AAF with upstream's own Python adapter.

```
python3 gen_otio.py ~/src/pyaaf2 ~/src/otio-aaf-adapter
```

It takes a [`pyaaf2`][pyaaf2] checkout and an [`otio-aaf-adapter`][adapter]
checkout, and needs `opentimelineio` installed. Re-run it only when the pinned
revisions change or a new fixture is added; running it should otherwise leave
both baselines byte-for-byte as they were.

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

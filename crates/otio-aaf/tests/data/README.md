# Test baselines

What upstream's own adapter reads out of each of two AAF files.

The AAF files themselves are not here. They live in the [`aaf`
crate](../../../aaf/tests/data), which vendors them from [`pyaaf2`][pyaaf2]'s
test suite at revision `08dcc3dfe823ea5781db1cb657d9c2606557fcdc` under that
project's MIT licence, and documents their provenance and what each covers.
The tests here read them from there: half a megabyte of fixtures is not worth
a second copy that can drift from the first.

## The baselines

`*.otio.json` is the timeline [`otio-aaf-adapter`][adapter] produces from the
matching file, written as OTIO JSON. A test that matches one is checking this
port against the library it is a port of, not against its own earlier output.

They were taken with `simplify=False` and `attach_markers=False`, which is the
structural transcription on its own. Upstream runs three more passes over that
result — `_fix_transitions`, `_attach_markers` and `_simplify` — and with them
on a baseline would exercise four things at once, so a mismatch would not say
which of them disagreed. The passes get their own baselines when they get
ported.

Regenerate them only against a checkout of the adapter and of pyaaf2, never
from this crate: a baseline produced here would agree with any bug it has. The
script is [`gen_otio.py`](generators/gen_otio.py), which takes both checkouts
as arguments, and the same note applies to it as to the generators in the
`aaf` crate — it is a developer tool, not a dependency. Nothing in
`cargo build` or `cargo test` runs Python.

## What these two files reach

Between them they cover almost the whole mapping: a timeline, a serializable
collection, stacks, tracks, clips, gaps, effects, and both kinds of media
reference. `sector_size_512.aaf` is a Pro Tools session exported as AAF, with
two audio tracks of fillers and effects, a timecode track, and a chain of mobs
three deep from each clip down to the WAVE file behind it. `empty.aaf` is a
valid AAF holding nothing, which is the case where the walk has to reach the
content storage and then come back empty rather than fail.

What they do not reach: transitions, markers, nested scopes, selectors,
pulldowns, time warps, and video of any kind. Those parts of the mapping are
written against upstream's source rather than against a file, and are noted as
such where they are implemented.

[pyaaf2]: https://github.com/markreidvfx/pyaaf2
[adapter]: https://github.com/OpenTimelineIO/otio-aaf-adapter

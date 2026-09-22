# Test fixtures and baselines

AAF files, what upstream's own adapter reads out of each of them, and, in
[`written/`](written), the AAF files upstream's adapter writes.

## The AAF files

Two come from [`pyaaf2`][pyaaf2]'s test suite and live in the [`aaf`
crate](../../../aaf/tests/data), which vendors them at revision
`08dcc3dfe823ea5781db1cb657d9c2606557fcdc` under that project's MIT licence
and documents what each covers. The tests read them from there rather than
keeping a second copy that could drift from the first.

The rest are here, copied byte for byte from [`otio-aaf-adapter`][adapter]'s
`tests/sample_data/` at revision `47886982d67c00573ad4a565ae51ad0e73f4caff`.
They are Apache-2.0 licensed, the same licence as this repository. Upstream
has 37; these ten were picked to reach every part of the mapping and every
pass while keeping the repository a few megabytes lighter:

| File | What it reaches |
|---|---|
| `2997fps-DFTC.aaf` | drop-frame timecode at 29.97 |
| `bad_marker_track_from_avid.aaf` | a marker naming a track the file does not have, which goes on the stack |
| `colored_clips.aaf` | clip colours |
| `essence_group.aaf` | an essence group, of which the first choice is read |
| `marker-over-transition.aaf` | markers moved onto clips either side of a transition |
| `misc_speed_effects.aaf` | time warps, linear and otherwise, and what an effect renders to |
| `nested_audio_dissolve.aaf` | a transition inside nested audio |
| `nesting_test.aaf` | nested sequences, and what simplifying keeps of them |
| `normalclip_sourceclip_references_compositionmob_with_usercomments_no_mastermob_usercomments.aaf` | a clip naming a composition with user comments, which simplifying must keep |
| `utf8.aaf` | names outside ASCII |

`*.aaf` is marked binary in the repository's `.gitattributes`, so git never
converts them.

## The baselines

Each file has two, both written by upstream's adapter as OTIO JSON:

- `<name>.structural.otio.json`, read with `simplify=False` and
  `attach_markers=False`. That is the transcription with only the one pass
  upstream always runs, so a mismatch there is in the mapping, not in the
  passes that reshape it.
- `<name>.otio.json`, read with upstream's defaults, which is what a caller
  of either library gets.

A test that matches one is checking this port against the library it is a
port of, not against its own earlier output. Regenerate them only against a
checkout of the adapter and of pyaaf2, never from this crate: a baseline
produced here would agree with any bug it has. The script is
[`gen_otio.py`](generators/gen_otio.py), and the same note applies to it as to
the generators in the `aaf` crate — it is a developer tool, not a dependency.
Nothing in `cargo build` or `cargo test` runs Python.

### From OTIO 0.18 to 0.19

The adapter targets OTIO 0.18, which names a marker's colour and gives a
transition no `enabled` flag. This workspace targets OTIO 0.19, which gives a
marker a colour object and a transition the flag, and upgrades a 0.18 file to
that on reading. The generator applies the same upgrade to what the adapter
writes, so a baseline is the adapter's result as OTIO 0.19 would write it. It
refuses to run under any other OTIO than 0.18, where the upgrade would need
checking first.

## The written files

[`written/`](written) holds what upstream's adapter writes, for the tests in
[`tests/write.rs`](../write.rs) to match byte for byte. The script is
[`gen_written.py`](generators/gen_written.py). For each file there are:

- `<name>.aaf`, the file upstream's adapter wrote through pyaaf2.
- `<name>.calls.tsv`, how it was written and what it was written with: the
  options, the user name, and every time and random identifier the adapter
  and pyaaf2 asked for, in order. The script replaces the clock, the
  identifier source and the user lookup with deterministic stand-ins and
  records what they handed out, because otherwise no two runs would write
  the same file. The Rust test hands its writer the same values and fails if
  the writer asks for anything else, in any other order.
- `<name>.roundtrip.otio.json`, what upstream's adapter reads back out of the
  file it wrote, upgraded to OTIO 0.19 as the baselines are. The Rust test
  reads its own file back and compares.

The inputs are of two kinds:

| Name | Input | What it reaches |
|---|---|---|
| `colored_clips` | `../colored_clips.otio.json` | clip colours, as component attributes |
| `essence_group` | `../essence_group.otio.json` | the clip an essence group was read as |
| `marker-over-transition` | `../marker-over-transition.otio.json` | markers either side of a transition, with the dates and users they were read with |
| `misc_speed_effects` | `../misc_speed_effects.otio.json` | clips under speed effects |
| `nested_audio_dissolve` | `../nested_audio_dissolve.otio.json` | a dissolve in sound, inside nesting |
| `nesting_test` | `../nesting_test.otio.json` | nested tracks, which become stacks |
| `sector_size_512` | `../sector_size_512.otio.json` | the file from pyaaf2's tests, with user comments and sound |
| `edit` | `edit.otio.json` | a cut built by the script: clips with and without an AAF behind them, a slug, a nested track, a shared master mob, a transition the writer skips, markers old and new, and sound with pan points and a dissolve |
| `options` | `options.otio.json` | the writer's options at 29.97: a MobID from the AAF a clip's media names, made-up MobIDs, and edge code |

The samples are written from their read baselines, which are exactly what
upstream's adapter reads from them, so the Rust test starts from the same
timeline upstream wrote. The two built timelines are saved as OTIO 0.18
writes them, and OTIO 0.19 upgrades them on reading. `options` names
`../aaf/tests/data/written_mobs.aaf` as a clip's media, a path relative to
this crate's directory, which is where both the script and `cargo test` run.

`2997fps-DFTC` and `empty` are not written, because upstream's adapter refuses
them: the first mixes rates, the second is not a timeline. The tests check
that this crate refuses them too, with the same reasons.

Each file is about half a megabyte, most of it the dictionary of Avid
definitions pyaaf2 writes into every file, and compresses to about fifty
kilobytes.

[pyaaf2]: https://github.com/markreidvfx/pyaaf2
[adapter]: https://github.com/OpenTimelineIO/otio-aaf-adapter

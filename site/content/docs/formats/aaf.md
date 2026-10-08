---
title: AAF
summary: Reading, writing and changing AAF files, the options each takes, and how a written file relinks in Media Composer.
section: Formats
order: 4
---

AAF is how a cut reaches Avid Media Composer. Reading and writing are ports
of upstream's AAF adapter, checked byte for byte against what upstream reads
and writes, and AAF is available from every language.

## Reading an AAF

An AAF reads as upstream's adapter reads it, byte for byte once written as
OTIO JSON, on every sample file in upstream's test data. Upstream's four
reading options are all here, with upstream's defaults:

- `simplify` (on) collapses the nesting AAF has and OTIO does not need.
- `attach_markers` (on) moves each marker from the slot that carries it onto
  the item it points at.
- `bake_keyframed_properties` (off) records each keyframed effect
  parameter's value at every frame of its effect as `keyframe_baked_values`,
  interpolated the way pyaaf2 interpolates it.
- `transcribe_log` (off) prints a line for each thing the reader makes,
  word for word what upstream prints. From Rust it takes a `TranscribeLog`,
  which hands each line to a function of your choosing. It is not in the C
  ABI or the SDKs built on it, since it would need a callback across it.

From C and the SDKs, the first two are spelled as what turning them off
does, `aaf_keep_nesting` and `aaf_markers_on_slots`, so that options left at
zero read an AAF the way upstream does. The third is `aaf_bake_keyframes`.

Baked curves go through the platform's `pow`, `acos` and `cos`, so off Linux
the last digit of a baked value can differ from upstream's. Three log lines
upstream prints for rare failures embed a Python object's memory address or
a whole track, and those are worded differently here.

## Writing an AAF

AAF is how a cut reaches Avid Media Composer, and writing one is a port of
upstream's AAF adapter rather than a design of its own. Each track becomes a
slot of a composition. Each clip becomes a chain of three objects: a master
clip, a file describing the media, and a tape carrying its timecode, which
is the chain Media Composer expects to relink through. Gaps, dissolves,
nested tracks, stacks and markers all have an AAF form; any other transition
is left out, as upstream leaves it out.

<!-- ::sample id="write-an-aaf" -->

A clip is tied to its media by a MobID. A cut read from an AAF already has
one on every clip, kept under `metadata["AAF"]`, and it is written back, so
the file relinks to the media it came from. A clip whose media is itself an
AAF holding one master clip takes that clip's MobID. Anything else, such as
a cut built from scratch, has none, and writing it needs
`use_empty_mob_ids`, which makes MobIDs up. A made-up MobID links to no
media Media Composer knows, which is why upstream refuses a clip without one
by default, and so does this.

Before writing anything, the writer checks the whole timeline the way
upstream does: every item at the timeline's rate, every clip saying how much
media it has, and every transition carrying what an AAF dissolve is built
from. It reports everything it finds wrong at once rather than stopping at
the first.

The file is the same, byte for byte, as the one upstream's adapter writes
from the same timeline, given the same clock and the same random
identifiers. The tests check that on twelve files, and it holds on all 33
samples in upstream's own test data that upstream can write.
That parity is the evidence the file suits Media Composer: none of the files
has been imported into Media Composer as part of testing.

Embedding the media in the file works as upstream's `embed_essence` does.
Each clip's media URL is taken as a path, relative to the working directory
unless it is absolute. An `.aaf` there has the master mob with the clip's
MobID copied out of it, with its source mob and essence. A `.dnx` on a video
track is imported as a raw DNxHD stream. Anything else is refused as
upstream refuses it, and that includes a WAV file, which upstream sends to
the DNxHD import too.

From Python, upstream's four AAF hooks run where upstream runs them:
`otio_aaf_pre_read_transcribe` and `otio_aaf_post_read_transcribe` before and
after the file is transcribed, and `otio_aaf_pre_write_transcribe` and
`otio_aaf_post_write_transcribe` before and after it is written. A
`pre_write` hook is where to transcode other media into something
`embed_essence` can embed, as upstream suggests: what it returns is what is
written. Each hook is handed upstream's arguments, except that `aaf_handle`,
upstream's open pyaaf2 file, is `None`, because there is no pyaaf2 file here.
From Rust, `ReadOptions::with_post_transcribe` runs a callback at the one
point inside the read; the other three run around the read or write, so a
caller runs them itself.

AAF writing is available from every language. From Python,
`otio.adapters.write_to_file(timeline, "cut.aaf")` takes upstream's keyword
arguments, `embed_essence=True` among them. From C and the SDKs, the same options are `aaf_`-prefixed fields of the write
options, along with three a library caller needs and a Python one does not:
`aaf_user`, whom a new marker is credited to when no login name is set;
`aaf_time`, the time the file records; and `aaf_id_seed`, which seeds the
identifiers it makes up. The same time, seed and timeline write the same
file. WebAssembly has no clock or randomness of its own, so the TypeScript
package passes the time and a fresh seed on every write unless you give
your own. Nor has it a file system, so it cannot embed media: with
`aafEmbedEssence` set, a clip whose media names a file stops the write, as
the file cannot be found.

## Changing an existing AAF

Below the adapter, the `aaf` crate can open an AAF file that already exists,
change it and save it, as pyaaf2 does when it opens a file with `'r+'`. The
same edits in the same order leave the same bytes pyaaf2 leaves, which
twenty scenarios in its tests check against files pyaaf2 changed: properties
changed, added and removed, mobs and slots added and taken out, new
definitions and classes, and essence moved and dropped. It works on the AAF
object model, not on a timeline, and is available from Rust only. The
[crate's README](https://github.com/alchemist-editor/otio-rust/blob/main/crates/aaf/README.md#changing-a-file)
has an example, and
[ADR 0005](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0005-aaf-modify-path.md)
the design.

---
title: ALE (Avid Log Exchange)
summary: Reading and writing shot logs, and where each column lands on a clip.
section: Formats
order: 2
---

An ALE is a tab-separated shot log: a `Heading` of key/value pairs, a
`Column` line naming the fields, and a `Data` section with one row per clip.
It is a list of clips rather than a cut, so a file reads as a
`SerializableCollection` of clips, not a timeline.

## What becomes what

| ALE | Document |
| --- | --- |
| `Heading` pairs | the collection's `metadata["ALE"]["header"]` |
| `Column` order | the collection's `metadata["ALE"]["columns"]` |
| `Name` | the clip's name (and stays as a column) |
| `Start`, `Duration`, `End` | the clip's `source_range` |
| `Source File` | an `ExternalReference` on the clip |
| `CDL`, `ASC_SOP`, `ASC_SAT` | the clip's `metadata["cdl"]` |
| every other column | the clip's `metadata["ALE"]` |

Keeping the unrecognized columns and the column order is what makes a round
trip lossless: upstream's own `sample.ale` reads and writes back byte for
byte.

## Rates

Unlike an EDL, an ALE states its rate, in the heading's `FPS`, and that rate
wins over the one you pass, because the file was written for it. Timecode
exists only at SMPTE rates, so a heading saying `23.976` is read at
24000/1001; the heading keeps the spelling it arrived with. A rate that is not
near a SMPTE rate is refused rather than quietly rounded.

## Quirks kept from upstream

- A value containing a tab is written as-is and becomes two columns when read
  back. Upstream means to replace tabs with spaces and throws the result away.
- The `Name` column stays in `metadata["ALE"]` as well as becoming the clip's
  name, so it is in the document twice. The writer depends on this: it
  discovers columns from that metadata.

## Where it differs from upstream

- A doubled sign such as `--1.0` in an `ASC_SOP` column simply does not start
  a number. Upstream matches it and then fails to convert, failing the whole
  row.
- Reading moves `ASC_SOP`, `ASC_SAT` and `CDL` into `metadata["cdl"]`, and the
  writer rebuilds those columns from there. Upstream's writer looks only at
  the `ALE` metadata, so a graded file it has just read writes those columns
  blank. The numbers go back out through a float, so `-0.0870` is written as
  `-0.087`.
- The writer snaps the heading's rate to a SMPTE rate the same way the reader
  does. Upstream writes at the stated decimal, so a file at `23.976` comes
  back with every time slid by a couple of frames.

The [crate's README](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-ale/README.md)
has the Rust options.

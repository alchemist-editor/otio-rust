---
title: EDL (CMX 3600)
summary: Reading and writing Edit Decision Lists: the rate you must supply, the dialects, and what writing refuses.
section: Formats
order: 1
---

An Edit Decision List is the oldest interchange format in post and the most
widely understood: a title, then a numbered list of events, each one or two
lines of timecode with free-form comments beneath. A file reads as a
`Timeline`.

<!-- ::sample id="read-an-edl" -->

## An EDL does not know its own rate

This is the one thing to get right. A CMX 3600 file is timecode and nothing
else: it never states the rate that timecode is at, and nothing can infer it
from the contents. The rate you pass is believed. Pass the wrong one and the
file still reads — every event simply lands somewhere it should not.

There is a usual guess, 24, and it is a guess rather than a safe default.

## What becomes what

| EDL | Document |
| --- | --- |
| `TITLE:` | the timeline's name |
| each channel the events name (`V`, `A1`, `AA/V`) | a track |
| an event | a clip on every track its channel maps to |
| a hole in the record timecode | a gap |
| `D` and `W###` edits | a `Transition` |
| `* FROM CLIP NAME:` | the clip's name |
| `* FROM CLIP:` / `* FROM FILE:` / `* OTIO REFERENCE FROM:` | the clip's media |
| a path with a `[1001-1020]` range in it | an `ImageSequenceReference` |
| `* LOC:` | a `Marker`, with its colour |
| `*ASC_SOP` and `*ASC_SAT` | the clip's `metadata["cdl"]` |
| `M2` | a `LinearTimeWarp` |
| a freeze-frame comment | a `FreezeFrame` |
| `BL`, `BLACK`, `BARS` reels | a `GeneratorReference` |
| everything else | the clip's `metadata["cmx_3600"]` |

Keeping the unrecognized comments is what lets a file survive a round trip:
reading each of upstream's samples, writing it back and reading it again
gives the same timeline.

## Dialects

The three systems that read EDLs disagree about the comment that names a
clip's media, so the writer takes a style:

| Style | Names media with |
| --- | --- |
| Avid (the default) | `* FROM CLIP:` |
| Nucoda | `* FROM FILE:` |
| Premiere | nothing, with the path in `* OTIO REFERENCE FROM:` |

Premiere reads any `FROM` comment as meaning the clip has no name and calls it
`UNKNOWN`, so that dialect writes `AX` as every reel and puts the path in a
comment Premiere ignores and this adapter can read back. From Rust the style
is an enum, so an unknown dialect cannot be spelled at all; from Python it is
upstream's `style` keyword argument.

## Reel names

Most systems will only read a reel name of eight characters, so the writer
pads or truncates to that by default. A truncated name is recorded in an
`* OTIO TRUNCATED REEL NAME FROM:` comment, so reading the file back gets the
original. Asking for no limit writes the name in full, which loses nothing
but which most systems will not read.

## What writing will not do

Writing is narrower than reading, as it is upstream:

- Only one enabled video track, and at most two audio tracks. An EDL describes
  a single strand of picture, so a timeline with two video tracks has no EDL
  form and is refused.
- Dissolves, but not wipes. A wipe reads, and writes back out as a dissolve.
- One timing effect per clip, and only a speed change or a freeze frame.

## Where it differs from upstream

The adapter is a port of upstream's `otio-cmx3600-adapter` and reproduces its
behaviour, with two differences:

- The writer works on a copy of the document, so writing does not change what
  you handed it. Upstream rewrites the timeline in place, and a caller who
  writes twice gets different text the second time.
- A marker's colour is a real colour here rather than the string `"RED"`, so
  the name is canonicalized to `"Red"` and given its components. The name the
  file used is still on the marker's `metadata["cmx_3600"]`.

The [crate's README](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-cmx3600/README.md)
has the Rust options in full.

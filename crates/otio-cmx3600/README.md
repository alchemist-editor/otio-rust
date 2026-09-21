# otio-cmx3600

Reads and writes CMX 3600 Edit Decision Lists as OpenTimelineIO documents.

An EDL is the oldest interchange format in post and the most widely
understood: a title, then a numbered list of events, each one or two lines of
timecode with free-form comments beneath. A file reads as a `Timeline`.

```rust
use otio_adapter::{Adapter, TextAdapter};
use otio_cmx3600::{Cmx3600, ReadOptions, Style, WriteOptions};

let document = Cmx3600::read_from_file("cut.edl", &ReadOptions::default())?;

let nucoda = WriteOptions { style: Style::Nucoda, ..WriteOptions::default() };
let text = Cmx3600::write_to_string(&document, &nucoda)?;
# Ok::<(), otio_adapter::Error>(())
```

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
`tests/cmx3600.rs` reads each of upstream's samples, writes it back and reads
it again, and the timeline comes back the same.

## Rates

An EDL does not say what rate its timecode is at, and nothing can infer it.
`ReadOptions::rate` has to be right, and a file read at the wrong rate
produces a timeline whose events are all in the wrong place. The default of 24
is the usual guess, not a safe one.

## Dialects

The three systems that read EDLs disagree about the comment that names a
clip's media, so `WriteOptions::style` picks one:

| `Style` | names media with |
| --- | --- |
| `Avid` (the default) | `* FROM CLIP:` |
| `Nucoda` | `* FROM FILE:` |
| `Premiere` | nothing, with the path in `* OTIO REFERENCE FROM:` |

Premiere reads any `FROM` comment as meaning the clip has no name and calls it
`UNKNOWN`, so that dialect writes `AX` as every reel and puts the path in a
comment Premiere ignores and this adapter can read back.

Upstream takes the dialect as a string and raises at write time for one it
does not know. Here it is an enum, so an unknown dialect cannot be spelled at
all, and `Style::from_str` is where a name from outside the library is checked.

## Reel names

Most systems will only read a reel name of eight characters, so
`WriteOptions::reelname_len` pads or truncates to that by default. A truncated
name is recorded in an `* OTIO TRUNCATED REEL NAME FROM:` comment, so reading
the file back gets the original. `None` writes the name in full, which loses
nothing but which most systems will not read.

## What writing will not do

Writing is narrower than reading, as it is upstream:

- Only one enabled video track, and at most two audio tracks. An EDL describes
  a single strand of picture, so a timeline with two video tracks has no EDL
  form and is refused.
- Dissolves, but not wipes. A wipe reads, and writes back out as a dissolve.
- One timing effect per clip, and only a speed change or a freeze frame.

## Fidelity

This is a port of upstream's `otio-cmx3600-adapter`, and reproduces its
behaviour rather than improving on it. Upstream's own test suite is carried
across in `tests/cmx3600.rs`, including the four tests that pin the exact text
a dissolve, a fade in, a fade out and two back-to-back dissolves write out as.

Two differences worth naming:

- The writer works on a clone of the document, so writing does not change what
  the caller handed it. Upstream rewrites the timeline in place, and a caller
  who writes twice gets different text the second time.
- A marker's colour is a real colour here rather than the string `"RED"`, so
  `otio-core` canonicalizes the name to `"Red"` and gives it its components.
  The name the file used is still on the marker's `metadata["cmx_3600"]`.

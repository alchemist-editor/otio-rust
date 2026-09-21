# otio-ale

Reads and writes Avid Log Exchange (ALE) files as OpenTimelineIO documents.

An ALE is a tab-separated shot log: a `Heading` of key/value pairs, a `Column`
line naming the fields, and a `Data` section with one row per clip. A file
reads as a `SerializableCollection` of clips.

```rust
use otio_adapter::{Adapter, TextAdapter};
use otio_ale::{Ale, ReadOptions, WriteOptions};

let document = Ale::read_from_file("shots.ale", &ReadOptions::default())?;
let text = Ale::write_to_string(&document, &WriteOptions::default())?;
# Ok::<(), otio_adapter::Error>(())
```

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
trip lossless: `tests/ale.rs` reads upstream's own `sample.ale` and writes it
back byte for byte.

## Rates

An ALE states its rate in the heading's `FPS`, and that rate wins over the one
a caller passes, because the file was written for it. Timecode exists only at
SMPTE rates, so a heading saying `23.976` is read at 24000/1001; the heading
keeps the spelling it arrived with. A rate that is not near a SMPTE rate is
refused rather than quietly rounded.

## Fidelity

This is a port of upstream's `otio-ale-adapter`, and reproduces its behaviour
rather than improving on it. Two quirks worth knowing about, both pinned by
tests that say why:

- A value containing a tab is written as-is and becomes two columns when read
  back. Upstream means to replace tabs with spaces and throws the result away.
- The `Name` column stays in `metadata["ALE"]` as well as becoming the clip's
  name, so it is in the document twice. The writer depends on this: it
  discovers columns from that metadata.

Three deliberate deviations, each pinned by a test:

- On input that is already malformed: upstream matches the sign of a decimal
  as `-*`, so `--1.0` in an `ASC_SOP` column matches and then fails to
  convert, failing the whole row. Here a doubled sign simply does not start a
  number.
- Reading moves `ASC_SOP`, `ASC_SAT` and `CDL` out of a clip's `ALE` metadata
  and into `metadata["cdl"]`. Upstream's writer looks only at the `ALE`
  metadata, so writing a graded file it has just read leaves those columns
  blank; here the writer rebuilds them. The numbers go back out through a
  float, so `-0.0870` is written as `-0.087`.
- Upstream writes timecode at whatever decimal the heading states, while its
  reader snaps that decimal to the nearest SMPTE rate first, so a file at
  `23.976` comes back with every time slid by a couple of frames. Here the
  writer snaps the same way the reader does.

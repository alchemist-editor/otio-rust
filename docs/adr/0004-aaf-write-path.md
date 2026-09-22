# ADR 0004: The AAF write path: pyaaf2's state machine, handles, and injected time and identity

- **Status:** Proposed
- **Date:** 2026-09-22

## Context

The `aaf` crate reads AAF. The OpenTimelineIO adapter also writes it, and
its users send those files to editing applications that do not all accept
the same things. [ADR 0002](0002-aaf-container.md) already requires the
container layout to match pyaaf2's byte for byte. The write path has to meet
the same standard one layer up, for three reasons:

- The files known to import are the files pyaaf2 writes.
- Byte identity is the only property a test can check completely.
- A reviewer can reproduce any fixture from upstream.

Three things decide whether a Rust writer can produce pyaaf2's bytes:

1. **The order of operations is part of the file.** pyaaf2 creates a storage
   when an object is attached. It writes a `properties` stream and the
   collection indexes when the file is saved. It walks objects in the order
   they were first attached, keyed by path. The directory slot, the sector,
   and the red-black tree position all follow from that order. The same
   content built in another order is a different, still valid, file.
2. **pyaaf2's objects are dynamic.** In `clip['StartTime'].value = 10`, the
   property is found by name at run time, in the dictionary the file carries,
   and the value is encoded against whatever type that property declares.
3. **pyaaf2 is nondeterministic.** A new file has a random `GenerationAUID`.
   Every new mob has a random `MobID` and the time it was made. The header
   records the time the file was saved.

## Decision

**Port the state machine, not just the format.** `AafWriter` keeps pyaaf2's
bookkeeping as it is:

- the ordered `modified` map of attached objects, keyed by path;
- the property entries of each object, ordered by pid;
- local keys and the next free key of each collection;
- the weak reference table;
- the countdown of dynamic pids.

Each attach, detach, and save does what pyaaf2's does, in the same order.
Anything pyaaf2 registers on every new file is generated from pyaaf2's own
model by `gen_write_tables.py`, in registration order: the Avid extensions,
the default definitions, and the pyaaf2 helper classes' constructor defaults.

**Name objects by handle.** A pyaaf2 object is a Python reference into a live
file. Here it is an `ObjRef`, an index into an arena that the writer owns,
the same shape [ADR 0001](0001-ownership-model.md) uses for timelines.
Operations are methods on the writer: `w.set(obj, "Name", value)`,
`w.append(obj, "Slots", slot)`, `w.create_timeline_slot(mob, 24, None)`.
Handles avoid shared ownership and interior mutability, and make "which file
does this object belong to" a question that cannot come up.

**Look up properties and types by name, as pyaaf2 does.** Property names are
the AAF model's names, looked up in the dictionary the writer builds. Values
are `WriteValue`s. They are encoded against the declared type with pyaaf2's
own coercions: enums by name or by index, extensible enums by AUID or by
case-insensitive name, `Boolean` by truthiness, and indirect values inferred
from the Rust value. A typed builder per class would catch more at compile
time, but it would have to be generated for every class and extension, and it
would drift from pyaaf2's run-time behaviour, which is what the bytes follow.

**Inject time and identity.** `WriteOptions` carries a `Clock` and an
`IdSource`. Every place pyaaf2 reads the clock or calls `uuid4` calls them
instead, at the same point.

- The defaults, `SystemClock` and `RandomIds`, behave like pyaaf2.
- `SteppingClock` and `SequentialIds` make every build of a file identical,
  for callers that want reproducible output.
- The tests replay the exact values pyaaf2 used, which `gen_written.py`
  records beside each fixture, and fail if the writer asks for a value
  pyaaf2 did not or at a different point.

`RandomIds` uses the standard library's randomly keyed hasher and the time,
because the crate takes no dependencies. AAF needs the identifiers to be
unique, not secret.

## Consequences

- The six written fixtures are identical to pyaaf2's output, byte for byte,
  and a regression shows up as the first differing byte and the stream or
  directory entry that holds it.
- Essence is part of the state machine too. pyaaf2 writes the stream of an
  object that is not in the file yet under `/tmp`, at a path named by a
  `uuid4`, moves it beside the object when the object is attached, and
  removes `/tmp` when the file is closed. The directory slots those moves
  and removals free are reused, so `AafWriter` parks, moves and removes
  streams exactly where pyaaf2 does, and draws the `uuid4` from the
  `IdSource` at the same point. Importing DNxHD and WAV essence
  (`import_dnxhd_essence`, `import_audio_essence`) reads the media in the
  same pieces pyaaf2 reads it, a frame or a second at a time, because each
  piece is one write to the stream. Copying objects in from another file
  (`copy_from`, pyaaf2's `copy(root=f)`) follows the source file's property
  order and copies streams in chunks of the source's sector size, as
  pyaaf2 does.
- The OpenTimelineIO adapter's writer in `otio-aaf`, a port of upstream's
  `aaf_writer.py` built on `AafWriter`, is held to the same standard with the
  same replay: its output is identical to upstream's adapter on twelve
  vendored fixtures, three of which embed essence, and on all 33 samples in
  upstream's own test data that upstream can write. The clock the adapter reads to date a new marker is the writer's
  clock (`AafWriter::now`), so those readings replay in their place among
  pyaaf2's own.
- Following pyaaf2 means inheriting its quirks, deliberately:
  - the colour of the root directory entry;
  - a `FixedArray` encoded with one element more than its count;
  - `create_timeline_slot` choosing a new slot's identifier by pyaaf2's search, not the lowest free one.
- Each quirk is commented where it is reproduced, so that a later "fix" is a
  conscious break from byte identity rather than an accident.
- Mistakes that a typed API would catch at compile time, such as a misspelt
  property name or a value of the wrong type, are run-time errors here. The
  error names the class and the property.
- Modifying an existing file (pyaaf2's `'r+'` and `'rw'`) is not covered. It
  needs the reader and writer joined through one object store.

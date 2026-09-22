# ADR 0005: Changing an existing AAF file: pyaaf2's `'r+'` mode, ported as it is

- **Status:** Proposed
- **Date:** 2026-09-22

## Context

[ADR 0004](0004-aaf-write-path.md) ported pyaaf2's writer for new files, and
held it to byte identity with what pyaaf2 writes. pyaaf2 can also open an
existing file and change it: `aaf2.open(path, 'r+')`, which it also spells
`'rw'`. Tools built on it use that mode to rename clips, add markers or
tagged values, drop mobs, or add a composition to a file an editing
application wrote, and save the file where it lies.

Changing a file is more exposed to layout than writing one. A new file is
laid out from nothing. A changed file keeps most of what it had: its sectors,
its directory, its free lists, stale bytes and all. What changes, and where
the new bytes land, follows from pyaaf2's bookkeeping in detail:

- **What is rewritten.** pyaaf2 writes back only the objects in its
  `modified` map. An object read from the file is not in it. The property
  setters, the collection operations and the attach and detach walks put
  objects in it, each at a particular point, and the map keeps the order in
  which objects were first put there. That order is the order streams are
  written in, so it decides which free sector each one gets.
- **Where it lands.** pyaaf2's container reads the file's FAT, mini FAT,
  DIFAT and directory, and reuses what is free. A freed sector goes to the
  front of the free list. Its directory free list starts empty, so new
  entries go into new directory sectors even when the directory has free
  slots. A stream rewritten in `'rw'` mode is written over and then cut to
  length, and one opened with `'w'` is freed first. On close the file is cut
  after the last sector in use.
- **What else changes.** Opening a file for writing merges the standard
  model into the file's meta dictionary: any class or type the file lacks is
  added, and a clashing dynamic property identifier is renumbered. Unless
  told not to, the Avid extensions are then registered, which rewrites the
  meta dictionary and replaces each extension type that is not an
  enumeration with a new object in the same storage. On a file another tool
  wrote, a save with no edits at all can write back hundreds of objects.
- **Streams of detached objects.** Taking an object that owns a stream out
  of the file moves the stream to `/tmp/<uuid4>/…`. Putting the object back
  moves it home. `/tmp` is removed when the file is closed.
- **What does not change.** pyaaf2 does not update the header's
  `LastModified` or add an identification when it saves an existing file.

## Decision

**Port the mode as pyaaf2 has it, into the same writer.** `AafWriter::open`
builds the same state `AafWriter::new` does, from an existing file, and the
same handle-based API then changes it. Nothing about the API differs between
a new file and an opened one.

- `CompoundFileWriter::open` reads the container into the state the writer
  keeps for a new file. Each directory entry keeps its 128 bytes as read,
  and saving writes back only the fields pyaaf2 would, into those bytes, so
  what pyaaf2 carries through unread (colour, timestamps, class identifiers)
  is carried through here too. Moving and removing entries is the
  container's `remove` module, which the essence work (#66) brought in for
  parking the streams of new objects; `rmtree` there now frees entries in
  the order pyaaf2's `walk(topdown=False)` lists them, since in a file
  changed afterwards the free lists are reused in that order.
- The object layer marks objects modified at the points pyaaf2's
  `@writeonly` decorator, `add2set` and `attach` do, and `finish` writes
  them in first-marked order.
- The meta dictionary is merged in pyaaf2's order: the standard model is
  built first and not attached; the file's own class and type definitions
  replace the standard ones of the same name and identifier; the standard
  ones the file lacks are appended in registration order, each type with
  its class; clashing dynamic identifiers are renumbered.
- A stream property keeps where its stream is parked, as pyaaf2's
  `StreamProperty.dir` does, and there is one way in and one way out.
  Detaching an object moves its streams under `/tmp`, drawing the `uuid4`
  from the `IdSource`, as does writing the stream of an object not yet in
  the file; writing it again while it is out writes the parked stream, and
  attaching moves it back. `finish` removes `/tmp` once, as pyaaf2's
  `remove_temp` does.
- The header is left alone on save, as pyaaf2 leaves it. `OpenOptions`
  carries the clock and identifier source, which new mobs and parked streams
  still read.

**Read every object when the file is opened.** pyaaf2 reads objects lazily,
as they are asked for. Reading has no effect on the file, so reading them
all up front gives the same result for every edit that succeeds, and keeps
the object store the same arena of handles a new file has. The cost is that
a file with an object that cannot be read fails to open, where pyaaf2 would
fail only on reaching it.

**Open from a path or from bytes.** pyaaf2 opens a file for changing only
by path, and changes it in place. `AafWriter::open` reads the file into
memory and `save` writes the result whole; `open_bytes` and `open_reader`
take the file from memory or any reader, and `finish` hands the bytes back.
There is no entry point over a `Read + Write + Seek`: pyaaf2's result
depends on cutting the file short, which `Seek` cannot do, and writing the
whole result is the same bytes.

**Test against pyaaf2's changes, kept as patches.** `gen_modified.py` opens
copies of fixtures with pyaaf2, makes scripted edits modelled on pyaaf2's own
tests, saves, and records the times and identifiers it used, as
`gen_written.py` does. A changed file is mostly the file it started from, so
what is kept is a patch: the 512-byte blocks of pyaaf2's result that differ
from its starting file. A scenario can start from another scenario's result.
The Rust tests replay the edits through the API and compare every byte.

## Consequences

- Twenty scenarios match pyaaf2 byte for byte: four on the container alone,
  in 4096- and 512-byte sectors, and sixteen on AAF files. Among them are a
  no-op save of a file PyAAF (the AAF SDK's Python bindings) wrote; changing,
  adding and deleting properties; adding a mob with slots and clips; removing
  a mob, a slot and a component; a new `MobID`; new definitions, a new class
  and a new property on an existing class; rewriting every object; a stream
  growing out of the mini stream and shrinking back; streams parked,
  restored, rewritten and dropped; and every mob taken out and put back in a
  512-byte-sector file.
- The mode inherits pyaaf2's choices along with its layout: a file saved
  with no edits can still grow, because the standard model and the
  extensions are added to it; an object taken out of the file leaves its
  storage behind; and new directory entries do not reuse free slots the file
  already had.
- `rmtree` visits children in the order pyaaf2's `listdir` returns them,
  which pyaaf2 caches for the 512 storages listed most recently. In the AAF
  layer `rmtree` runs only on `/tmp` at close, where the order cannot change
  the bytes; a caller removing several hundred storages directly through
  `CompoundFileWriter` could see a different order from pyaaf2.
- Opening a file and embedding essence share the container's removal code
  and the stream parking with the essence work in #66, so an object that
  holds essence can be taken out of an opened file and put back, or new
  essence written into one, through the same path a new file uses.

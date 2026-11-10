# ADR 0014: A multicam schema, as a prototype

- **Status:** Proposed. This is a prototype on a branch, kept off `main` while
  feedback comes in.
- **Date:** 2026-11-10
- **Deciders:** Jeff Hodges

## Context

OpenTimelineIO has no multicam ([upstream #427]). Trevor Aylward's
[otio-multicam-tests] holds the same multicam edit made in Avid, Premiere and
Resolve, with each one's OpenTimelineIO and AAF exports. None of the three
OpenTimelineIO exports records which angle a cut shows. Premiere and Resolve
copy the whole multicam, every angle, into every cut, which makes a 62-cut
edit 25 to 54 MB. Avid writes the flattened result.

Jeff's proposal ("OTIO Multicam Proposal") stores each multicam once, on the
timeline, and makes a cut name the angle it plays. It is prototyped twice:
in C++ and Python on Jeff's fork of OpenTimelineIO (branch
`claude/project-thread-otfkde`, draft jhodges10/OpenTimelineIO#2), and here.
The proposal is authoritative: where this ADR and it disagree, the proposal
wins and this ADR is wrong.

Briefly, the proposal says:

- `Timeline.multicams` is a dictionary of `Multicam` keyed by a name, as
  `Clip.media_references` is, and is left out of the file when empty.
- `Multicam.1` is a `Stack`. Its children are one `Stack` per angle, and an
  angle's name is its identity. It has an optional `global_start_time`.
- `MulticamClip.1` is an `Item`, a peer of `Clip`, with `multicam` (the key),
  `angle` (the name) and `angle_track` (an integer, default 0). Its
  `source_range` is in the multicam's time. `angle_track` counts only the
  angle's tracks of the kind of the track holding the cut.
- `validate` lists duplicate angle names, multicams that reach themselves
  through their angles, and cuts that do not resolve. `flatten` replaces each
  cut with the slice it plays and removes the definitions, reporting what it
  could not keep.

[upstream #427]: https://github.com/AcademySoftwareFoundation/OpenTimelineIO/issues/427
[otio-multicam-tests]: https://github.com/TrevorAyl/otio-multicam-tests

## Decisions

### The schemas are built-in subclasses, not new node kinds

`Multicam` and `MulticamClip` are registered at start-up as subclasses of
`Stack` and `Item`, by the mechanism of [ADR 0006](0006-subclassing-built-in-schemas.md).
A multicam is held as `Node::Stack` and a cut as `Node::Item`, with their own
fields in the extension fields. So every composition algorithm, the edit
operations and every adapter already accept them: an editor can trim, slip or
ripple a cut with the ordinary calls, and a multicam's angles are ordinary
stacks.

A new `Node` variant would have been the alternative. It would touch every
`match` in the core, the C ABI's kind enum and every SDK's class table, for
a schema that may still change. If the proposal is accepted upstream, giving
them variants of their own is a separate decision.

### `Timeline.multicams` is an extension field holding objects

The timeline's `multicams` is an extension field whose value is a dictionary
of `Any::Object` handles. The definitions live in the same arena as the
timeline and are not anyone's children, so they are not reached by
`find_clips` or the track walk; `validate` and `flatten` reach them by key.
Removing the last key removes the field, so a timeline without multicams
writes exactly what upstream writes.

### The C ABI is one group of free functions

The calls are `otio_multicam_*`, taking the document as the algorithms do:
creating a multicam and a cut, reading and switching a cut's angle, the
timeline's keyed definitions, `resolve`, `validate` and `flatten`. They are
generated into all seven SDKs with their prefix kept (`MulticamFlatten` in
Go), and into one `multicam` namespace in TypeScript (`multicam.flatten`),
where two hand-written constructors, `newMulticam` and `newMulticamClip`,
give each new object its own document as `new Clip()` does. Reports from
`validate` and `flatten` cross the ABI as one string, a problem per line,
empty when there is none.

### Field order differs from the C++ prototype

Extension fields are written before the built-in fields, as upstream writes a
Python subclass's fields (ADR 0006). The C++ prototype declares the fields in
the classes, so it writes them after. Both read either order, and the tests
compare the two writers' output as a multiset of lines rather than byte for
byte.

### AAF reads multicam on request and writes it whenever it is there

`otio-aaf` reads AAF at parity with upstream, and upstream turns each Avid
selector into the angle it chose. Reading a selector as a cut instead is the
`multicam` read option, off by default, so a default read stays byte for
byte upstream's. A selector whose choice is a source clip into a composition
becomes a `MulticamClip`, and that composition a `Multicam` keyed
`aaf:<MobID>`, named after the group composition that selects into it.
Its picture and sound slots are grouped into angles by slot name if they
have one, else by the master clip each starts with, which is what Media
Composer's files give. Each angle is simplified as the timeline is.

Writing needs no option, since upstream cannot write a timeline with
multicam cuts at all. Each multicam becomes a backing composition, its slots
named after their angles so they read back as the same angles, plus a
lower-level group composition named after the multicam; each cut becomes a
selector into the backing composition with the other angles' tracks at the
same place as alternates. A reader without multicam takes the choice, so the
file reads as the flattened edit there. Track names inside an angle are not
kept: the slot name carries the angle's.

## What is not here yet

- **A check in Media Composer.** The written form follows Media Composer's
  own files and reads back here, but no written file has been imported into
  Media Composer.
- **Python.** The Python bindings here do not have the schemas; the fork
  does.
- **Premiere and Resolve.** Neither export says which angle is live, so
  neither can be rebuilt from its OpenTimelineIO file alone. Pairing each
  with its AAF is the next step if the proposal needs them.

## Consequences

- The tests read the C++ prototype's files, round-trip them, and check that
  flattening here gives the same timeline as flattening there, for a small
  synthetic edit and for Trevor's Avid edit (13 angles, 62 cuts), whose
  flattened form matches Avid's own export frame for frame.
- Trevor's Avid AAF is vendored in `otio-aaf`. Read with `multicam`, it
  gives 1 multicam of 13 angles and 62 cuts, and flattening that gives the
  ordinary read. Written and read back, the angles and cuts are the same,
  and a plain read of the written file gives what writing and reading the
  ordinary read gives. No written file has been imported into Media
  Composer.
- Alchemist web can import, edit and export multicam edits through the
  TypeScript SDK without anything multicam-specific in the wasm layer.
- Nothing here merges to `main` until the proposal settles.

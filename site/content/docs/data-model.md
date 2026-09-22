---
title: The data model
summary: Timelines, stacks, tracks, clips, gaps, transitions — and the handles that name them.
section: The data model
order: 1
---

An OTIO document is a tree of objects that occupy time. There are not many
kinds, and the names are the ones upstream uses.

```text
Timeline
└── Stack  ("tracks")
    ├── Track  ("V1", kind "Video")
    │   ├── Clip   ("shot_01")
    │   ├── Gap
    │   └── Clip   ("shot_02")
    └── Track  ("A1", kind "Audio")
        └── Clip   ("dialogue")
```

A **Timeline** is the whole cut. It holds one **Stack**, conventionally
called `tracks`, and a stack lays its children over the same span of time —
which is what a track being "above" another means. A **Track** is a strand:
its children run one after another, and a **Gap** is the hole where nothing
plays. A **Clip** is a piece of media with a range taken out of it, and a
**Transition** is the overlap between two of them.

## Time, and what a range means

Every duration in OTIO is a `RationalTime`: a value and a rate, kept apart.
24 frames at 24fps is not the number 1 — it is `24/24`, and the rate travels
with it. This is the single most consequential decision in the format,
because it is what stops a 23.976 sequence from drifting into a 24 one by
accident.

<!-- ::sample id="time-math" -->

An item has up to three ranges, and the difference between them is the thing
people get wrong:

- **`available_range`** — everything the media has. A two-hour rush.
- **`source_range`** — the piece of it this item uses. Four seconds of that
  rush. An item without one uses all of its media.
- **`trimmed_range`** — the source range, or the available one when there is
  no source range. What the item actually contributes.

A clip's `duration` is its trimmed range's duration; a composition's is the
sum of its children's. That is why a track knows how long it is without
anyone saying so.

There are two more — `visible_range` and `range_in_parent` — and the thing
that actually trips people up is which clock each one answers in.
[Time ranges](/docs/time-ranges) is that, in full.

## Handles rather than pointers

Objects live in a `Document`, which owns them. You never hold an object
directly: you hold a handle, which is an index and a generation, and the
document resolves it. A handle to something that has gone away fails the
lookup rather than reading freed memory, and a handle from one document
cannot be used against another.

This is worth knowing because it shapes every binding. Every SDK but one hides
the document and lets a clip look like an object; Zig keeps it in the open,
because an arena you hand to the things that use it is how Zig already works.
Either way, what is underneath is the same arena.

<!-- ::sample id="build-a-timeline" -->

## Schemas, and what happens to ones we do not know

Every object carries a schema name and version — `Clip.2`, `Marker.3` — and
this port reads the versions upstream 0.19 writes, upgrading older ones as it
reads. An object whose schema is not recognised at all is kept verbatim, so a
third-party plugin's data survives a read and a rewrite untouched.

Writing works in the other direction too. A document can be written for an
older release by naming the schema versions it should carry, and each object
is downgraded on the way out, as upstream's serializer does. The upgrade and
downgrade steps live in one registry in the core, keyed by schema and
version, and from Python `register_upgrade_function` and
`register_downgrade_function` add to it.

A schema defined in Python with `register_type` is held in the core the way
upstream's C++ holds it: a generic object carrying its schema name, version
and fields. Python keeps the class for each schema name and hands that class
back when the object is read, so a type defined in Python round-trips through
any document without the core ever holding a Python object.

A Python subclass of a concrete schema, such as a `Clip` that also records
which take it is, stays a clip in the core: it carries the subclass's schema
name, version and extra fields beside its own, so every composition and
algorithm treats it as the clip it is, and it is written under the subclass's
name. A program that has not registered the subclass reads it as an unknown
schema, kept verbatim. Any built-in object read from a file keeps the fields
it does not know in the same place, and writes them back.

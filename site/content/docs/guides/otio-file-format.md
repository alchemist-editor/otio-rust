---
title: The .otio file format
summary: What is in an OTIO JSON file, what this implementation guarantees about it, and where it departs from strict JSON.
section: Guides
order: 2
---

`.otio` is the format the rest of OpenTimelineIO exists to read and write. It
is JSON, it is meant to be read by a person as well as a program, and it is
the contract between this implementation and every other one.

That contract matters more here than it does upstream. Upstream is the
reference implementation, so whatever it writes is by definition right; this
is a second implementation, and a file it writes has to open unchanged in
tools built on the first.

## The shape

An OTIO file is a tree of objects. Each object is a JSON dictionary whose
fields are either plain JSON values or more OTIO objects.

Every object carries an `OTIO_SCHEMA` key naming its type and version:

```json
{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "my timeline",
  "metadata": {},
  "tracks": {
    "OTIO_SCHEMA": "Stack.1",
    "name": "tracks",
    "children": [
      {
        "OTIO_SCHEMA": "Track.1",
        "name": "video track",
        "kind": "Video",
        "children": []
      }
    ]
  }
}
```

There is **no file-level version**. Each type is versioned on its own, so a
new `Clip.3` can land without renumbering everything else. That is also why
reading starts by looking at whatever the top-level object says it is rather
than at a header.

Children live under `children`, everywhere except a timeline, whose one child
is `tracks`.

## The top level can be anything

Most files hold a timeline, and most code assumes one. The format does not
require it: a bare `Clip`, a `Track`, even a `RationalTime` is a valid `.otio`
file, and this library writes one as readily.

```text
otio_core::to_string starts at whatever it is pointed at.
Hand it a clip and you get a clip.
```

Code that reads a file should say so when the top-level object is not what it
wanted, rather than assuming. A `SerializableCollection` is the right
container for several objects, because a bare JSON array cannot carry
metadata of its own.

## No instancing

There are no references to an object from two places. If the same clip or the
same media appears twice in a timeline, it appears as two identical copies.

This is worth knowing before you go looking for a way to share one: the
format does not have it, and neither do the objects in memory — see
[the data model](/docs/data-model) for how handles work here.

## Metadata

Nearly every object has a `metadata` dictionary, holding anything JSON can
hold, nested as deeply as you like, including other OTIO objects.

The core does nothing with it at all. It carries it, unchanged, so adapters
and tools can keep what the schema has no field for — which several of the
adapters here do.

**Namespace it.** Two workflows writing to the same file will collide unless
each keeps to its own key:

```json
"metadata": {
  "my_playback_tool": { "loop": false },
  "my_production_tracking_system": { "status": "IP", "owner": "rose" }
}
```

It is also where a new schema usually starts life: put the field in metadata,
see whether it earns its place, then move it.

## What this implementation guarantees

- **Field order is upstream's**, not alphabetical and not this library's
  convenience. Fields come out in the order upstream's own `write_to` methods
  emit them, so a diff between a file written here and one written there is
  empty rather than a reordering.
- **Indentation is four spaces** by default, as upstream's is. The format
  permits minifying and the project recommends against it: gzip the file if
  size matters, and keep it readable.
- **Round trips are byte-for-byte** on upstream's own sample documents, which
  is how that first point is measured rather than asserted.

## Where it is not quite JSON

Upstream configures RapidJSON with `kWriteNanAndInfFlag`, so a non-finite
number is written as a bare `NaN`, `Infinity` or `-Infinity` literal. None of
those is valid JSON, and a strict parser will refuse the file — **including
upstream's own**, unless it is the one doing the reading.

This library reproduces it exactly, in both directions: it writes those
literals for non-finite values and accepts them when reading, in the
spellings RapidJSON accepts (`Inf` as well as `Infinity`).

That is deliberate, and it is the project's rule in miniature. Writing a
tidier `null` instead would quietly change a document that somebody else's
tool produced, and agreeing with upstream is the only way to agree with every
other reader. The place to be surprised by this is here, in the
documentation, rather than in a file that will not load.

## Naming

Files should end in `.otio`. Not `.json` — the suffix is how every adapter in
the ecosystem, this one included, decides what it is holding.

---

This page is adapted from the
[File Format Specification](https://github.com/AcademySoftwareFoundation/OpenTimelineIO/blob/main/docs/tutorials/otio-file-format-specification.md)
in upstream OpenTimelineIO's documentation, which is Apache-2.0 like this
project. The guarantees section describes this implementation and has no
counterpart upstream.

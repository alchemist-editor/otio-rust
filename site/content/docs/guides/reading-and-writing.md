---
title: Reading and writing files
summary: The adapters, what each format can carry, and what a round trip preserves.
section: Guides
order: 1
---

An adapter turns a file of some interchange format into a document and back.
Upstream expresses this as a plugin with four loosely specified entry points
and a dictionary of keyword arguments; here it is one trait per format, with
each format's options named and typed — so a misspelled option is a compile
error rather than a silent no-op. The Python package puts upstream's own
`opentimelineio.adapters` back on top of those, with each adapter's names,
keyword arguments and exceptions, so code written against upstream runs
unchanged; there an unknown keyword is a `TypeError` rather than ignored.

| Format | Suffix | Reads | Writes |
| --- | --- | --- | --- |
| OpenTimelineIO JSON | `.otio` | Yes | Yes |
| CMX 3600 EDL | `.edl` | Yes | Yes, one video track |
| Avid Log Exchange | `.ale` | Yes | Yes |
| Final Cut Pro 7 XML | `.xml` | Yes | Yes |
| Final Cut Pro X XML | `.fcpxml` | Yes | Yes |
| AAF | `.aaf` | Yes | Not yet |

<!-- ::sample id="read-an-edl" -->

## An EDL does not know its own rate

This is the one thing to get right. A CMX 3600 file is timecode and nothing
else: it never states the rate that timecode is at, and nothing can infer it
from the contents. The rate you pass is believed. Pass the wrong one and the
file still reads — every event simply lands somewhere it should not.

There is a usual guess, 24, and it is a guess rather than a safe default.

## What a round trip keeps

Reading and writing the same file back should produce the same file. Where a
format carries something the data model has no place for, the adapter keeps
it on the object's metadata under the format's own key — `metadata["cmx_3600"]`
for an EDL — so that writing it out reproduces the line it came from rather
than dropping it.

The opposite direction has limits that are the format's rather than this
library's. An EDL describes a single strand of picture, so a timeline with
two video tracks has no EDL form at all; a wipe reads as a wipe and writes
back out as a dissolve, because CMX 3600's vocabulary for wipes is not one
anybody agrees about. These are upstream's limits too, and they are
documented on each adapter rather than discovered at runtime.

## Writing what you built

`.otio` is the format with no limits — it is the data model's own
serialization, so anything you can build, it can hold.

<!-- ::sample id="build-a-timeline" -->

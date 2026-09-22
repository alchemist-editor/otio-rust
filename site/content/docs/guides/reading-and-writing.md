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
| AAF | `.aaf` | Yes | Yes, from Rust and Python |

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

## Converting between formats

Converting is a read and a write with nothing in between. There is no
conversion step, and there is no per-pair code: an EDL and an FCP X XML are
two spellings of the same object model, so once a file is read the format it
came from stops mattering.

<!-- ::sample id="convert-a-format" -->

What survives the trip is the intersection of the two formats, and the table
above is how to predict it. Going to an EDL narrows a cut to one video
strand whatever it came from; going the other way, an EDL carries no media
references beyond reel names, so nothing downstream can relink without being
told where the media is.

## Writing an AAF

AAF is how a cut reaches Avid Media Composer, and writing one is a port of
upstream's AAF adapter rather than a design of its own. Each track becomes a
slot of a composition. Each clip becomes a chain of three objects: a master
clip, a file describing the media, and a tape carrying its timecode, which
is the chain Media Composer expects to relink through. Gaps, dissolves,
nested tracks, stacks and markers all have an AAF form; any other transition
is left out, as upstream leaves it out.

<!-- ::sample id="write-an-aaf" -->

A clip is tied to its media by a MobID. A cut read from an AAF already has
one on every clip, kept under `metadata["AAF"]`, and it is written back, so
the file relinks to the media it came from. A clip whose media is itself an
AAF holding one master clip takes that clip's MobID. Anything else, such as
a cut built from scratch, has none, and writing it needs
`use_empty_mob_ids`, which makes MobIDs up. A made-up MobID links to no
media Media Composer knows, which is why upstream refuses a clip without one
by default, and so does this.

Before writing anything, the writer checks the whole timeline the way
upstream does: every item at the timeline's rate, every clip saying how much
media it has, and every transition carrying what an AAF dissolve is built
from. It reports everything it finds wrong at once rather than stopping at
the first.

The file is the same, byte for byte, as the one upstream's adapter writes
from the same timeline, given the same clock and the same random
identifiers. The tests check that on nine files, and it holds on all 33
samples in upstream's own test data that upstream can write.
That parity is the evidence the file suits Media Composer: none of the files
has been imported into Media Composer as part of testing. Two things
upstream does are not ported: embedding the media in the file, which needs
decoding it, and running Python hooks.

AAF writing is available from Rust and from Python, where
`otio.adapters.write_to_file(timeline, "cut.aaf")` takes upstream's keyword
arguments; `embed_essence=True` raises `NotImplementedError` there. The C ABI
and the SDKs built on it have no AAF at all yet.

## Writing what you built

`.otio` is the format with no limits — it is the data model's own
serialization, so anything you can build, it can hold.

<!-- ::sample id="build-a-timeline" -->

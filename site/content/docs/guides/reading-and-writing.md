---
title: Reading and writing files
summary: The adapters, which formats each language can read and write, and what a round trip preserves.
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

| Format | Suffix | Reads | Writes | Guide |
| --- | --- | --- | --- | --- |
| OpenTimelineIO JSON | `.otio` | Yes | Yes | [The .otio file format](/docs/concepts/otio-file-format) |
| CMX 3600 EDL | `.edl` | Yes | Yes, one video track | [EDL](/docs/formats/edl) |
| Avid Log Exchange | `.ale` | Yes | Yes | [ALE](/docs/formats/ale) |
| Final Cut Pro 7 XML | `.xml` | Yes | Yes | [Final Cut Pro XML](/docs/formats/final-cut-pro) |
| Final Cut Pro X XML | `.fcpxml` | Yes | Yes | [Final Cut Pro XML](/docs/formats/final-cut-pro) |
| AAF | `.aaf` | Yes | Yes | [AAF](/docs/formats/aaf) |
| OTIO zip bundle | `.otioz` | Yes, except from TypeScript | Yes, except from TypeScript | [Bundles](/docs/formats/bundles) |
| OTIO directory bundle | `.otiod` | Yes, except from TypeScript | Yes, except from TypeScript | [Bundles](/docs/formats/bundles) |

<!-- ::sample id="read-an-edl" -->

Reading hands back whatever the file was about, usually a timeline, and the
same call shape works for every format: name the file, and pass the format's
options when it has any. Each format has its own page for what it carries and
what it cannot, linked from the table above. Two of them have a trap worth
knowing before you start: an [EDL](/docs/formats/edl) does not know its own
rate, and an [AAF](/docs/formats/aaf) clip cannot be written without a MobID.

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

Upstream's own adapters fail outright on several of these conversions, and
here they work. An EDL whose tracks start anywhere but `00:00:00:00`, or an
audio-only EDL, could not be written as FCP X XML; neither could an FCP 7
timeline with a 15 fps clip. And FCP X XML written as FCP 7 XML produced a
file that could not be read back. Each is a documented departure on the
adapter it concerns. So is one fix to plain reading: an FCP 7 nested
sequence reads with its tracks, where upstream reads every one as an empty
stack.

## Writing what you built

`.otio` is the format with no limits — it is the data model's own
serialization, so anything you can build, it can hold.

<!-- ::sample id="build-a-timeline" -->

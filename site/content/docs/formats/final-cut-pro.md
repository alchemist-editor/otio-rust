---
title: Final Cut Pro XML
summary: FCP 7 interchange XML and FCP X XML: what each reads as, and what is kept for the round trip.
section: Formats
order: 3
---

Two different formats share the name. FCP 7's `.xml` is the older one, and
still how a great many tools hand an edit to one another; FCP X's `.fcpxml`
is Final Cut's current format, and it does not think in tracks. Both are ports
of upstream's adapters, and both keep what OTIO has no field for in metadata
so a file read and written again keeps it.

<!-- ::sample id="convert-a-format" -->

## Final Cut Pro 7 XML

FCP 7 itself is long gone, but its XML is not: Premiere Pro, Resolve, Hiero
and Media Composer all read or write some dialect of it.

The format carries far more per-element detail than OTIO has fields for.
Everything the adapter does not turn into a real OTIO field is kept under the
`fcp_xml` key in the relevant object's metadata and written back out on the
way past, so a file read and written again keeps its colour settings, its
effect parameters and its host application's bookkeeping.

One fix to plain reading departs from upstream: a nested sequence reads with
its tracks, where upstream reads every one as an empty stack.

## Final Cut Pro X XML

Final Cut does not think in tracks. A sequence holds one `spine`, the main
storyline, and everything layered over or under it hangs off whichever
storyline item it overlaps, carrying a `lane` number saying how far above or
below it sits. Reading works out where each element really starts and groups
the results by lane, one track per lane. Writing puts lane zero back into the
spine and reattaches the rest.

What comes back from a read depends on what the file holds:

| The file holds | It reads as |
| --- | --- |
| a library or an event | a `SerializableCollection` of timelines |
| a bare project | a `Timeline` |
| loose clips | a collection of clips and compound clips |

What Final Cut knows about a piece of media that OTIO has no field for (its
note, its keywords and its Spotlight metadata) is kept under the `fcpx` key in
the media reference's metadata and written back out on the way past.

## Conversions upstream cannot do

Several conversions that fail outright with upstream's adapters work here,
each a documented departure on the adapter it concerns:

- An EDL whose tracks start anywhere but `00:00:00:00`, or an audio-only EDL,
  written as FCP X XML.
- An FCP 7 timeline with a 15 fps clip, written as FCP X XML.
- FCP X XML written as FCP 7 XML, which upstream wrote as a file that could not
  be read back.

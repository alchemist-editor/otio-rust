# otio-fcpx

Final Cut Pro X XML, read and written as OpenTimelineIO. This is a port of
upstream OpenTimelineIO's `otio-fcpx-xml-adapter`.

Final Cut does not think in tracks. A sequence holds one `spine`, the main
storyline, and everything layered over or under it hangs off whichever
storyline item it overlaps, carrying a `lane` number saying how far above or
below it sits. Reading means working out where each element really starts and
grouping the results by lane, one track per lane. Writing puts lane zero back
into the spine and reattaches the rest.

What comes back from a read depends on what the file holds: a library or an
event becomes a `SerializableCollection` of timelines, a bare project becomes
a `Timeline`, and a file of loose clips becomes a collection of clips and
compound clips.

What Final Cut knows about a piece of media that OTIO has no field for — its
note, its keywords and its Spotlight metadata — is kept under the `fcpx` key in
the media reference's metadata and written back out on the way past.

Behaviour matches upstream, quirks included; the deliberate differences are
listed in the crate documentation.

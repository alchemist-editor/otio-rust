# Sample data

These `.edl` files and `enabled.otio` are copied verbatim from upstream's
`otio-cmx3600-adapter` `tests/sample_data/`. They are Apache-2.0 licensed, the
same licence as this repository.

They are vendored rather than fetched so the conformance tests run offline and
against a pinned version. Between them they cover the shapes real files come
in: the three dialects, dissolves at the head of a clip, in the middle of one
and across a whole one, a wipe, a transition stated over three events and one
stated on the wrong event, frame numbers where timecode belongs, record
timecode that does not add up, holes in the record timecode, speed ramps and
freeze frames, colour decisions written with and without commas, an event
targeting two audio tracks, and a file with no column padding at all.

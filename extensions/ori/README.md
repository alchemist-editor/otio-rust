# ORI Annotations

**An otio-rust extension, not part of OpenTimelineIO.** The review
annotations of the ASWF's ORI project, from
[ORI Annotations](https://github.com/richardssam/ORIAnnotations): paint
strokes, text and shapes on a frame, stored on an OpenTimelineIO timeline.
ORI owns the schemas; this is one implementation of them, in Rust, built on
[timeline sync](../sync). [ADR 0012](../../docs/adr/0012-ori-annotations.md)
has the design and epic #126 the phases.

| Folder | What it is |
| --- | --- |
| [`otio-ext-ori-annotations`](otio-ext-ori-annotations) | The extension itself, in Rust: ORI's schemas, the annotation track API and ORI's coordinates |

It is enabled as `ori-annotations`, which enables `sync` too. While it is
off, ORI's schemas read as `UnknownSchema` and are written back unchanged.
With it on, ORI's files still round-trip byte for byte, the annotation track
is written as ORI's own Python writes it, and every command reads as a typed
value. The C interface, the SDK modules and Python come next (#128), then
live review over Furnace (#129) and the ORI wire (#130).

See [NOTICE](NOTICE) for what is ported from ORI Annotations.

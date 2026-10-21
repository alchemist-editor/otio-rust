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
| [`otio-ext-ori-annotations`](otio-ext-ori-annotations) | The extension itself, in Rust: ORI's schemas, the annotation track API, ORI's coordinates, the merge rules, and live review (`review`) |
| [`capi`](capi) | Its C interface, `otio_ext_ori_*` in `otio_ext_ori.h`, compiled into `libotio` behind otio-capi's `ext-ori` feature; every SDK's `ori` module is generated from it |

It is enabled as `ori-annotations`, which enables `sync` too. While it is
off, ORI's schemas read as `UnknownSchema` and are written back unchanged.
With it on, ORI's files still round-trip byte for byte, the annotation track
is written as ORI's own Python writes it, and every command reads as a typed
value. It is in every SDK and Python (#128). Live review (#129, #130)
commits review comments with their strokes on a sync session, folds the ones
the server relays into the host's timeline, and sends strokes in progress;
over ORI's wire an annotation edit goes out as `REPLACE_ANNOTATION_COMMANDS`.
Furnace does not carry strokes in progress yet.

See [NOTICE](NOTICE) for what is ported from ORI Annotations.

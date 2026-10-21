---
title: ORI Annotations
summary: Reading, writing and editing the ORI project's review annotations on an OpenTimelineIO timeline.
section: Extensions
order: 3
---

ORI Annotations are drawings and notes made on a frame during review: paint
strokes, text and shapes. The schemas belong to the ASWF's ORI project, whose
[ORI Annotations](https://github.com/richardssam/ORIAnnotations) repository
defines them as an OpenTimelineIO plugin in Python and exchanges them live
between OpenRV and xStudio. This extension reads and writes the same objects,
under the same names, so a file written here opens in ORI's tools and the
other way round.

It is built on [timeline sync](/docs/extensions/timeline-sync): enabling it
enables sync, and every object it creates carries a sync GUID.

## Where it stands

The extension is being built in phases, tracked in
[epic #126](https://github.com/alchemist-editor/otio-rust/issues/126):

1. **Schemas, the annotation track and coordinates**, in Rust.
2. **Every language**: the C interface, a module in every SDK, and Python.
   Phases 1 and 2 are what is here now.
3. **Live review over Furnace**: strokes in progress and review commits as
   sync channels.
4. **ORI's wire**, for live review with RV and xStudio.

## Turning it on

| Language | Import | Enable |
| --- | --- | --- |
| Rust | `otio-ext-ori-annotations` | `otio_ext_ori_annotations::install(); otio_ext::enable(otio_ext_ori_annotations::ID)?;` |
| Python | `from opentimelineio.ext import ori` | `ori.enable()` |
| C | `#include "otio_ext_ori.h"` | `otio_ext_ori_enable(&error);` |
| Go | `github.com/alchemist-editor/otio-rust/sdk/go/ext/ori` | `ori.Enable()` |
| Swift | `import OTIOExtOri` | `try Ori.enable()` |
| TypeScript | `import * as ori from "@alchemist-edit/otio/ext/ori"` | `await init(); ori.enable();` |
| C++ | `opentimelineio/ext/ori.hpp` | `otio::ext::ori::enable();` |
| C# | `using OpenTimelineIO.Ext.Ori;` | `OriExtension.Enable();` |
| Objective-C | `#import <OpenTimelineIO/OTIOExtOri.h>` | `OTIOExtOriEnable(&error);` |
| Zig | `otio.ext.ori` | `try otio.ext.ori.enable();` |

The id is `ori-annotations`. While it is off, `PaintStart.1` and the rest
read as unknown schemas and are written back unchanged, exactly as upstream
OpenTimelineIO does when ORI's plugin is not on its path; the annotation calls
refuse to run. With it on, ORI's files still come out byte for byte as they
went in.

## Commands

An annotation is a list of commands. A stroke is three or more commands that
share one `uuid`: a `PaintStart.1` with the colour, brush and who drew it, one
or more `PaintPoint.1` batches of points, and a `PaintEnd.1`. Text and shapes
are one command each.

| Schema | Holds |
| --- | --- |
| `PaintStart.1` | Opens a stroke: `uuid`, `rgba`, `brush`, `type`, `participant_hash`, `friendly_name`, and ORI's ghosting and hold fields |
| `PaintPoint.1` | A batch of points: a `PaintVertices.1` with parallel `x`, `y`, `size` and `alpha` arrays |
| `PaintEnd.1` | Closes a stroke |
| `TextAnnotation.1` | Text at a `position`, with `font`, `font_size`, `scale`, `rotation` and `spacing` |
| `EllipseAnnotation.1`, `RectangleAnnotation.1` | A shape in the box from `min` to `max`, with outline and fill colours |
| `ArrowAnnotation.1` | An arrow from `start` to `end` |

The batch of points is written `PaintPoint.1`, singular, as ORI's code and
files write it; ORI's prose sometimes says `PaintPoints.1`. ORI's playback
events (`Play.1`, `SetCurrentFrame.1`, `SyncPlayback.1` and the others) are
registered too, so ORI's recordings read as typed objects.

In Rust, `Command::read` gives a typed copy of a command and keeps every field
it does not name; a new command built with a constructor such as
`PaintStart::new` has the same defaults ORI's Python class gives it.

Every other language passes commands as JSON text: a list of ORI objects,
exactly as an annotation clip's `annotation_commands` holds them in a file.
A field a command leaves out takes the default ORI's class gives it, as
reading it with ORI's plugin would, so `{"OTIO_SCHEMA": "PaintStart.1"}` has
ORI's `"brush": "circle"`. Python's calls also take a list of dicts.

## Where annotations live

A frame's annotations sit on a one-frame clip in a track named `Annotations`,
beside the media:

```text
Stack
├─ Track "V1"            Clip "shot"            metadata.sync.guid = "shot"
└─ Track "Annotations"   Gap   Clip "Annotation_10"
                               source_range: frame 10, one frame long
                               metadata.clip_guid = "shot"
                               metadata.annotation_commands = [PaintStart, PaintPoint, PaintEnd, …]
```

The annotation clip's start time is the annotated frame of the media clip,
and `clip_guid` names that clip by its sync GUID. This is the form ORI's sync
protocol and Alchemist's review design both chose. It follows ORI exactly,
including one surprise: an annotation clip for a frame later than the end of
the track is placed after a gap at its own frame, but one for an earlier frame
is simply added at the end, so annotations are found by `clip_guid` and frame,
never by where they sit on the track.

Reading also understands two older forms ORI has written: its review export,
where the track is named after the review and each clip names the media in
`annotated_clip_name`, and `AnnotationEffect.1`, an effect on the media clip
that holds its commands. Neither is written.

<!-- ::sample id="annotate-a-frame" -->

The calls, named as in Rust's `annotations` module and in every language's
own case:

| Call | Does |
| --- | --- |
| `find_track`, `ensure_track` | Finds the timeline's annotation track, or adds one |
| `find_clip`, `ensure_clip` | Finds the annotation clip for a media clip GUID and frame, or adds one |
| `add_commands` | Adds commands at the end of an annotation clip |
| `replace_commands` | Replaces every command with a uuid, such as one whole stroke, in place |
| `remove_commands` | Removes every command with any of a list of uuids |
| `set_commands` | Replaces all of a clip's commands, as ORI's replace does; an empty list clears it |
| `list`, `list_for_clip`, `list_at_frame` | The object holding every annotation on the timeline, or those for a clip or a frame (`for_clip` and `at_frame` in Rust, which also has `by_participant`) |
| `describe`, `commands` | What an annotation says about itself, as JSON, and its commands |

Edits go by uuid, so one person's edit never disturbs another person's
strokes.

## Coordinates

ORI places every point in height-normalised coordinates with Y up and the
origin at the centre of the picture: `y` runs from -0.5 at the bottom to 0.5
at the top, and `x` from minus to plus half the aspect ratio, ±8/9 on a 16:9
frame.

```text
x = (px - width / 2) / height
y = -(py - height / 2) / height
```

`coords::ImageSize` converts to and from pixels, a port of ORI's `coords.py`
that gives ORI's own test chart its numbers bit for bit; every language has
it as `pixels_to_ori` and `ori_to_pixels`. Where a clip's media
has `available_image_bounds`, `ori_to_bounds` and `bounds_to_ori` map
ORI's space onto OpenTimelineIO's: the bounds' centre is ORI's origin and
their height ORI's unit. Without bounds ORI assumes 16:9.

Drawing the strokes is a renderer's job, not this extension's: it stores and
moves the commands.

## Checked against ORI

- ORI's own `annotationreview.otio` and test chart round-trip byte for byte,
  with the extension on and off.
- A review built through these calls is byte for byte the file ORI's
  `SyncManager` and annotation builder write for the same strokes, generated
  with ORI's plugin on upstream OpenTimelineIO 0.18.1.
- Every command in ORI's test chart reads as a typed command and writes back
  the same.
- ORI's own test suite, run on otio-rust's Python bindings with ORI's plugin,
  gives exactly what it gives on upstream OpenTimelineIO 0.18.1.
- ORI's plugin and this extension work in the same Python program: ORI's
  classes still read ORI's commands, this extension finds them, and the file
  writes back byte for byte.
- The same review is built and edited by each SDK's own tests.

See [ADR 0012](https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0012-ori-annotations.md)
for the design.

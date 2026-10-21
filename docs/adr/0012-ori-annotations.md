# ADR 0012: ORI Annotations as an extension

- **Status:** Accepted
- **Date:** 2026-10-21
- **Deciders:** Jeff Hodges

## Context

The ASWF's ORI project defines review annotations for OpenTimelineIO in its
[ORI Annotations](https://github.com/richardssam/ORIAnnotations) repository:
paint strokes, text and shapes drawn on a frame, as an OpenTimelineIO
schemadef plugin in Python, with OpenRV and xStudio plugins that exchange them
live over ORI Sync. ORI's own next step, from its SIGGRAPH 2026 session, is to
formalise that schema and its coordinates for OpenTimelineIO.

Alchemist means to read and write the same annotations from Swift,
TypeScript, Go and Zig, and was about to write four codecs by hand.
otio-rust already reaches all four through its C ABI and SDK generator. Jeff
asked for ORI Annotations to be built here as an extension, clearly not
OpenTimelineIO ([ADR 0010](0010-extensions.md)), on top of timeline sync
([ADR 0011](0011-timeline-sync.md)).

What ORI has today, which this has to match:

- **Schemas.** `PaintStart.1`, `PaintPoint.1` with `PaintVertices.1`,
  `PaintEnd.1`, `TextAnnotation.1`, `EllipseAnnotation.1`,
  `RectangleAnnotation.1` and `ArrowAnnotation.1`, all subclasses of a
  `SyncEvent` class with no OpenTimelineIO base beyond `SerializableObject`;
  the playback events `Play.1`, `SetCurrentFrame.1`, `GetSession.1`,
  `RequestSyncPlayback.1`, `SyncPlayback.1` and `MediaChange.1`; and an older
  `AnnotationEffect.1`, a subclass of `Effect`.
- **Storage.** ORI's sync protocol and Alchemist's review design
  (`app-apple/docs/MULTI_DEVICE_REVIEW.md`) both put a frame's commands on a
  one-frame clip in a track named `Annotations`, in
  `metadata.annotation_commands`, with `metadata.clip_guid` naming the media
  clip by its sync GUID. ORI's review export uses a review-named track and
  `annotated_clip_name`; some files carry `AnnotationEffect.1`.
- **Coordinates.** Height-normalised, Y up, origin at the image centre
  (`otio_sync_core/coords.py`).
- **Live editing.** A finished annotation travels as an ordinary
  `insert_child` patch; an edit replaces a clip's whole command list
  (`REPLACE_ANNOTATION_COMMANDS`); strokes in progress travel as a transient
  `PARTIAL` message that is never stored.

## Decisions

### Layout and switch

The extension lives in `extensions/ori/`, its crates are `otio-ext-ori-*`,
starting with `otio-ext-ori-annotations`, and its id is `ori-annotations`. It
depends on `sync`, so enabling it enables sync. Ported material keeps its
attribution in `extensions/ori/NOTICE`; both projects are Apache-2.0.

### Schema names stay ORI's

The schemas register under ORI's exact names. A namespace prefix would break
every ORI file, RV and xStudio. The boundary is drawn by where the code lives
and by enabling it, not by renaming. The point batch is `PaintPoint.1`, as
ORI's code, its example files and Alchemist write it, although ORI's spec and
docstrings say `PaintPoints.1`.

### No change to the object model

Every `SyncEvent` schema registers as a dynamic schema with a
`SerializableObject` base, and `AnnotationEffect.1` as a subclass of `Effect`
([ADR 0006](0006-subclassing-built-in-schemas.md)), so no `Node` variant is
added. A dynamic object keeps exactly the fields its file had, so ORI's files
round-trip byte for byte with the extension on as well as off. Typed Rust
values (`command::PaintStart` and the rest) read and build commands on top;
each keeps the fields it does not name, and its constructor gives every field
the default ORI's Python class gives it, so a new command is written exactly
as ORI writes one.

Registering a schema that is already registered the same way, as by a
program that loaded ORI's own classes first, succeeds; one registered some
other way fails the enable.

### Annotations are written in the track form

The crate writes only the `Annotations` track form, following ORI's
`SyncManager` exactly: the track is found by a name containing "annotation";
an annotation clip is found by `clip_guid` and the whole frames of its
`source_range.start_time`; a new one is named `Annotation_<frame>`, lasts one
frame, has a `MissingReference`, and goes at the end of the track, after a gap
when its frame is later than the track's end. A frame earlier than the end is
appended anyway, so the track's layout is not a timeline of the frames, as in
ORI. New tracks, clips and gaps get sync GUIDs. Reading also accepts ORI's
review export and `AnnotationEffect.1`; neither is written.

Commands are added, replaced and removed by `uuid`, so one person's edit
never disturbs another's strokes. A stroke is every command sharing a uuid.
Setting a clip's whole command list, as ORI's replace does, with an empty
list clearing it, is there too. The kind-by-kind merge Alchemist specifies
(a replacement missing one kind of command leaves that kind alone) is a sync
rule, and comes with live review in phase 3; see "As built" below.

### Coordinates

The pixel conversion is a direct port of ORI's `coords.py`, checked against
ORI's own test numbers and, bit for bit, against its test chart. Where an
item's media has `available_image_bounds`, ORI's space maps onto them: the
bounds' centre is ORI's origin and their height ORI's unit. Without bounds,
ORI's 16:9 default applies.

### Rendering is out of scope

Drawing strokes is the job of the ASWF's annotation geometry work. This
stores and moves commands; renderers draw them.

### On sync

Finished annotations are ordinary timeline patches, so sync carries them with
no special handling. Live review (phase 3) adds only what a timeline cannot
express, as two channels registered through the sync session's
`register_channel`: strokes in progress (transient, latest-wins per stroke
uuid) and review commits (an idempotent ledger: Furnace's `review_commit`,
`review_committed`, `review_rejected`). The ORI wire (phase 4) reads
`REPLACE_ANNOTATION_COMMANDS` as a patch and writes it back for ORI peers.
None of this edits sync's transport crate.

### As built: live review and the ORI wire (phases 3 and 4)

Added 2026-10-21. Three things came out differently from the plan above,
each for a reason found while building it.

- **Review commits use the session's own ledger, not a new channel.**
  Sync's session already had Furnace's review ledger (`commit_review`,
  `review_relay`, resent until acknowledged), and `register_channel` refuses
  a ledger channel on purpose. So `review::commit` builds a Furnace review
  commit from an anchor, a note and the commands, after checking ownership,
  and hands it to that ledger; `review::fold` applies a relayed one to the
  host's timeline. A comment is recorded on its annotation clip in
  `metadata.alchemist.comments` (`commentId`, `authorUserId`,
  `bodyMarkdown`, `commandUuids`, `state`, `revision`), which is what makes
  folding idempotent (by comment id and revision) and what ownership is read
  from: a stroke belongs to the comment listing its uuid. `note` holds the
  comments' notes and `status` is `open`, as Alchemist's
  MULTI_DEVICE_REVIEW.md specifies.
- **Strokes in progress go on a named, latest-wins channel**, `ori.partial`,
  with kind `Annotation.1/PARTIAL` and ORI's PARTIAL payload
  (`clip_guid`, `frame`, `fps`, `events`). The ORI profile carries it as
  ORI's own message. Furnace closes the connection on a message type it
  does not know, and passes through only a fixed list, so over Furnace the
  session refuses to send one (unsupported) rather than lose the
  connection. Carrying them needs a Furnace change, which is that service's
  owner's call; until then only finished comments travel over Furnace.
- **The ORI wire needed one line in sync's transport crate.** Sync's phase
  5 already read `REPLACE_ANNOTATION_COMMANDS` and merged annotation clips
  in `INSERT_CHILD`. What it lacked was the way out: a `SetProperty` of
  `/metadata/annotation_commands` to an array now goes out as
  `REPLACE_ANNOTATION_COMMANDS` for that clip rather than as a generic
  property set ORI's peers ignore.

The merge rules are plain functions in `annotations`, usable without a
session: `merge_commands` (by uuid, in place or appended),
`replace_by_kind` (strokes and text replaced kind by kind; an empty list
clears), and `merge_duplicates` (one clip per clip GUID and source frame).

Known gap: sync lets only a host or driver publish timeline changes, so an
ORI peer joined as a reviewer can send strokes in progress but not publish
finished ones. Lifting that is a sync decision, tracked separately.

## Consequences

- ORI's files read, edit and write in Rust, and in every SDK and Python
  through the C interface (`extensions/ori/capi`), with a file ORI's Python
  wrote as the test of what is written.
- Commands cross the C interface as JSON text, as they sit in a file, and a
  field a command leaves out takes ORI's class default. Points and times
  cross as plain numbers, since the SDK generator writes the core's structs
  into the core's module and not an extension's. Generated modules name the
  extension `ori`; the id stays `ori-annotations`.
- ORI's own Python plugin runs on otio-rust's Python bindings unchanged, and
  beside this extension: a schema it registered first is registered the same
  way, so enabling the extension keeps ORI's classes.
- The API refuses to work while the extension is off, so nothing written by a
  program that did not ask for ORI looks like ORI.
- otio-core reads every number exactly. Upstream OpenTimelineIO's reader
  rounds some 17-digit numbers to a neighbouring double, so ORI's
  `annotationreview.otio`, read and written by upstream 0.18, changes a few
  coordinates in the last digit; read and written here, it does not. That is
  a core difference, tracked on its own, not ORI's.
- Commands held by a removed annotation clip's metadata are dropped from the
  arena by this crate's own calls; otio-core's `remove_recursive` does not
  follow metadata, so a clip removed by other means leaves its commands in
  the arena, unreachable and never written.

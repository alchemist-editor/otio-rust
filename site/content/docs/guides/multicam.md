---
title: Multicam (prototype)
summary: A prototype of a proposed OpenTimelineIO multicam schema. A timeline holds its multicam definitions once, and each cut names the angle it plays, so switching angles is a one-field edit and nothing is copied per cut.
section: Guides
order: 5
---

> **Prototype.** This page describes a proposal, not OpenTimelineIO. The
> schemas it adds (`Multicam.1`, `MulticamClip.1`) and the timeline's
> `multicams` field are not in any upstream release, and they can still
> change. Upstream readers refuse a file that uses them until it is flattened
> (see [Older readers](#older-readers)). The decision record is ADR 0014 in
> the repository, and the proposal is tracked upstream as
> [OpenTimelineIO#427](https://github.com/AcademySoftwareFoundation/OpenTimelineIO/issues/427).

Upstream OpenTimelineIO has no multicam. Editors that export one today copy
the whole multicam, every angle, into every cut, and none of them records
which angle the cut was showing. An edit of 62 cuts from 13 cameras turns
into tens of megabytes, and the editor's choice of camera is lost.

The proposal stores each multicam once, on the timeline, and makes a cut a
small object that says which angle it plays.

<!-- ::sample id="switch-a-multicam-angle" -->

## The schema

| Object | What it is |
| --- | --- |
| `Timeline.multicams` | The multicam definitions, keyed by a name the cuts use, as a clip keys its media references. Left out of the file when empty. |
| `Multicam.1` | A `Stack` whose children are one `Stack` per angle. An angle's name is its identity and must be unique. An optional `global_start_time` says where the multicam's own time starts. |
| `MulticamClip.1` | An `Item`, a peer of `Clip`, with `multicam` (the key), `angle` (the angle's name) and `angle_track`. Its `source_range` is in the multicam's time. |

An angle holds as many tracks as its camera recorded: usually one video track
and one audio track per channel. A cut on a video track plays the angle's
video track number `angle_track`, counting only video tracks; a cut on an
audio track counts only audio tracks. So a cut that plays camera B's third
audio channel is `angle: "B", angle_track: 2` on an audio track.

In otio-rust a `Multicam` reads back as a stack and a `MulticamClip` as an
item, through the same mechanism as any other subclass of a built-in schema.

## Working with it

Every call is in the `multicam` group: `multicam.set` and `multicam.get` put a
definition on a timeline and fetch it, `multicam.clipSetAngle` switches a cut,
and `multicam.resolve` answers with the angle track a cut plays.
`multicam.validate` lists what is wrong: duplicate angle names, a multicam that
reaches itself through its own angles, and cuts that do not resolve.

## Older readers

`multicam.flatten` replaces every cut with the slice of the angle track it
plays, padded with a gap where the angle runs out, then removes the
definitions. The result is a plain timeline that every OpenTimelineIO reader
accepts. Each flattened item records where it came from in
`metadata["otio.multicam"]["flattened_from"]`, and the call answers with what
it could not keep, such as an effect on a cut that became several items.

The prototype was checked against Trevor Aylward's Avid test edit: read from
its AAF into these schemas and flattened, it gives the same timeline as
Avid's own OpenTimelineIO export, clip for clip and frame for frame, and
otio-rust's flattening matches the C++ prototype's on the same file.

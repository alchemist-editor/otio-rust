---
title: Timeline structure
summary: How stacks, tracks, clips, gaps and transitions nest, and what a flattened timeline looks like.
section: The data model
order: 2
---

A timeline holds tracks, and tracks hold clips — and then somebody nests a
stack inside a track and it stops being obvious. This page is the shapes:
what contains what, and what the result means when something plays it.

Upstream's figures are pictures; these are the same structures as text,
which is the same information and survives being copied into a terminal.

## A simple cut list

The smallest useful timeline is a single track of clips laid end to end.

```text
Timeline
└── Stack "tracks"
    └── Track "Track-001"
        ├── Clip "Clip-001"  source_range 0+10   →  Media-001  available 0+20
        ├── Clip "Clip-002"  source_range 5+10   →  Media-002  available 0+15
        ├── Clip "Clip-003"  source_range 2+8    →  Media-003  available 100+30
        └── Clip "Clip-004"  source_range none   →  Media-004  available 0+12
```

A timeline always has that top-level stack, even with one track in it,
because a timeline can hold several and something has to lay them over each
other.

At the bottom, each media reference has a `target_url` saying where the media
is and an `available_range` saying how much of it there is. Ranges are written
here as `start+duration` in frames; `Media-003` starting at 100 means its
first frame is timecode 00:00:04:04 at 24fps, not that it is offset in the
cut.

Above them, each clip's `source_range` picks the piece it uses.
`Clip-004` has none, so it uses all of `Media-004` — and asking any of them
for `trimmed_range` answers without the caller having to know which case
it is in.

A `source_range` may point outside the `available_range`. That happens for
real — only the first half of a shot has been rendered — and nothing here
snaps it back or complains. What a player does about it is the player's
business.

The track's own length is the sum of its children's trimmed ranges. Nobody
sets it.

## Transitions

A transition blends two neighbours on the same track: a dissolve, a wipe. It
can sit between any two composable items, not only clips.

```text
Track "Track-001"
├── Clip "Clip-002"     ends at frame 10
├── Transition          in_offset 2, out_offset 3
└── Clip "Clip-003"     starts at frame 0
```

`out_offset` reaches back into the clip before it, `in_offset` forward into
the clip after. So `Clip-002` is on screen for three frames longer than its
own range says, and `Clip-003` for two frames earlier — which is exactly what
`visible_range` reports for each while `trimmed_range` does not. See
[Time ranges](/docs/time-ranges) for the difference.

**A transition changes no length.** The track is as long with it as without,
so a tool that cannot render one may ignore it and still agree about where
everything is.

The format expects that a transition's offsets stay inside its neighbours,
that a clip with transitions at both ends has them not overlapping, and that
two transitions are never adjacent. Nothing in this library enforces any of
that — upstream does not either — so a file can carry all three and will be
read back as written.

A fade to or from black is usually a transition against a `Gap`, which may
have zero duration. A gap is *transparent* rather than black, so with tracks
underneath it the thing below shows through; for real black, use a clip with
a `GeneratorReference`.

## Several tracks

Tracks in a stack play at the same time, lower entries first.

```text
Stack "tracks"
├── Track-001   [ Clip-001 ][  Gap 4  ][ Clip-002 ]
├── Track-002   [ Gap ][    Clip-003     ][ Clip-005 ]
└── Track-003   [ Clip-006 ]        (trimmed by its own source_range)

flattened       [ Clip-001 ][Clip-003][ Clip-002 ]
```

Where `Track-001` has a gap, the track below shows through — those four
frames of `Clip-003` are what the flattened result carries. `Clip-005` is
covered by `Clip-003` the whole time, so it does not appear at all.

The gap at the head of `Track-002` is doing nothing but offsetting what
follows it. That is the usual way to shift a track's contents; setting the
track's own `source_range` is the other, and `Track-003` does it that way.

Tracks in a stack need not be the same length. Making a short one match means
appending a gap, or extending its `source_range` — either way `trimmed_range`
ends up the same, which is the only thing downstream cares about.

Image tracks composite in painter order, bottom to top, over a background
that is zero in colour and full in alpha. Audio tracks sum. Neither is
something OTIO does — it is what OTIO *means*, and the application does it.
What an effect on a clip does is not specified at all.

## Nesting

A track's children can be any composable item, and that includes other tracks
and stacks.

```text
Track "Track-001"
├── Clip "Clip-001"
├── Stack "Nested Stack"   source_range 2+6
│   ├── Track-A   [ Clip-003 ]
│   └── Track-B   [ Clip-005 ]      (trimmed by its own source_range)
├── Gap
└── Clip "Clip-004"
```

A nested composition behaves exactly like a clip that happens to have
complicated contents instead of a media reference. Give it a `source_range`
and only that slice of it is included — frames 2 to 7 here. Give it none and
its whole `available_range` is computed and used.

The consequence worth remembering: **nesting is opaque**. The gap in
`Track-001` cannot see `Clip-003`, because `Clip-003` is not its peer — it is
inside `Nested Stack`, and contents do not spill out into the neighbouring
gaps of the outer track.

---

This page is adapted from
[Timeline Structure](https://github.com/AcademySoftwareFoundation/OpenTimelineIO/blob/main/docs/tutorials/otio-timeline-structure.md)
in upstream OpenTimelineIO's documentation, which is Apache-2.0 like this
project. Its figures have been redrawn as text, and the claims about what is
and is not enforced were checked against this implementation.

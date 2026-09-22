---
title: Time ranges
summary: The five ranges an item has, which clock each one answers in, and why they differ.
section: The data model
order: 3
---

A timeline is a one-dimensional coordinate system, and laying clips
end-to-end in a track is the easy case. Trimming, nesting, dissolves and
speed changes are where it stops being obvious, and the confusion is almost
always the same one: two different ranges were read as though they were
measured against the same clock.

They are not. Every range below belongs either to the **item's own clock** —
the media's frame numbering — or to its **parent's clock**, the track it sits
in. Getting this right is most of getting OTIO right.

<!-- ::sample id="ranges-of-a-clip" -->

That sample prints four different answers about one clip, and the rest of
this page is why.

## The ranges in the item's own clock

### `available_range`

Everything behind the item, before anything trims it.

For a clip that is the active media reference's own `available_range`: if the
clip points at `wedding.mov`, this is how long that file is and what timecode
it starts at. For a track it is the sum of its children; for a stack, the
longest of them.

It can be unknown. Media goes missing, and a URL can be expensive enough to
query that nobody has. Upstream answers `None` in that case; here asking a
clip whose media says nothing about its length is an error rather than an
answer — `NoAvailableRange` in Rust, which reaches C as
`OTIO_STATUS_CORE_ERROR` and each SDK as whatever that SDK makes of a
failure. Note the difference from `source_range` below, where absence *is*
one of the answers and the status is `OTIO_STATUS_NO_VALUE`.

### `source_range`

The piece of the media this item uses — the trim, and the only one of these
ranges you set rather than ask for.

It may be absent, which means the item uses all of its media. An item with
neither a `source_range` nor an `available_range` is invalid: something has
to say how long it is.

It is usually shorter than what is available, but it does not have to be.
Asking for a range that runs past the end of the media is a real thing to
do — "this shot needs to be four seconds and only two have been rendered" —
and neither upstream nor this port will stop you or snap it back. What a
player does about it is the player's business.

### `trimmed_range`

The `source_range` if there is one, and the `available_range` if there is
not. This is how long the item is as far as its parent is concerned, and it
is what `duration` reports.

### `visible_range`

The `trimmed_range`, widened by any transition that reaches into the item.

A clip trimmed to end at frame 10, followed by a dissolve with an
`out_offset` of 5, is on screen until frame 15 — so its `visible_range` ends
at 15 while its `trimmed_range` ends at 10. With no transition touching it,
the two are equal, which is why the sample above prints the same numbers for
both.

## The ranges in the parent's clock

### `range_in_parent`

Where the item sits in the composition holding it.

In a **track** this is what you would expect, and one invariant follows from
it: each child begins exactly where the previous one ended, so for adjacent
clips A and B in a track,

```text
A.range_in_parent().end_time_exclusive() == B.range_in_parent().start_time()
```

In a **stack** it is duller, because a stack lays its children over the same
span: the start is always zero and the duration is the child's own. That is
what "these tracks are on top of each other" means. To offset something, put
it in a track and put a gap in front of it — or trim the track itself.

### `trimmed_range_in_parent`

The same, clipped to the parent's own `source_range`.

Most parents have no `source_range`, so most of the time this is
`range_in_parent` again. When the parent *is* trimmed, this is the honest
answer about where the child is relative to what survives the trim, and when
the parent's trim excludes the child entirely there is no answer at all:
`None` in Rust, `OTIO_STATUS_NO_VALUE` through the C ABI.

One inherited quirk: a *direct* child trimmed out of its composition raises
`InvalidTimeRange` rather than answering "nothing". That is upstream's
behaviour and it is deliberate here, because the alternative — handing back
a zero-length range — would quietly place the item at the head of the track.

## Compositions

A track or a stack is an item too, so it has all of the above. Two more are
worth naming:

- **`range_of_child_at_index`** — where the nth child sits, transitions
  included.
- **`trimmed_range_of_child_at_index`** — the same, relative to the
  composition's own trim.

Asking a track or a stack for its `duration` sums its children, which is why
nobody has to tell a track how long it is.

## Markers

A marker can hang off any item, and its `marked_range` is in **that item's**
clock — the same clock as the item's `source_range`. A marker on a clip is
positioned in the media's numbering; a marker on a track is positioned along
the track.

Its duration may be zero, which means an instant rather than a span. Both
are ordinary.

## Transitions

A transition has `in_offset` and `out_offset` rather than a range of its own:
how far it reaches back into the item before it and forward into the item
after. `range_in_parent` and `trimmed_range_in_parent` work on one, and both
defer to the parent.

A transition does not make its track any longer or shorter. A tool that
cannot render one can ignore it and still get the timeline's length right.

---

This page is adapted from
[Time Ranges](https://github.com/AcademySoftwareFoundation/OpenTimelineIO/blob/main/docs/tutorials/time-ranges.md)
in upstream OpenTimelineIO's documentation, which is Apache-2.0 like this
project. The behaviour described has been checked against this
implementation rather than carried over on trust; where the two differ — the
shape of "there is no answer", chiefly — it says so above.

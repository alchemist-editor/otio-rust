---
title: Editing a timeline
summary: The ten edit operations, what each one does to its neighbours, and which of them change a track's length.
section: Guides
order: 3
---

Appending clips to a track builds a cut. Changing one is a different problem,
because every change has a question attached: what happens to everything
after it?

There are ten operations, and they are the ones a non-linear editor has.
They are functions rather than methods, because each is about two objects and
belongs to neither — which is how upstream arranges them too.

<!-- ::sample id="edit-operations" -->

## The ten

| Operation | What it does |
| --- | --- |
| `insert` | Puts an item at an instant, pushing what follows later. |
| `overwrite` | Lays an item over a span, replacing what was there. |
| `trim` | Moves an item's in and out points without moving its neighbours. |
| `ripple` | Moves an item's in and out points, sliding everything after it. |
| `roll` | Moves the cut between an item and its neighbour. |
| `slip` | Moves the media inside an item without moving the item. |
| `slide` | Moves an item along its track, taking the time from its neighbours. |
| `slice` | Cuts whatever sits at an instant into two. |
| `fill` | Drops an item into a gap, fitted as the reference point says. |
| `remove` | Takes whatever sits at an instant out. |

## The question each one answers

The useful way to hold these is by what they do to the track's length and to
the neighbours.

**Length changes.** `insert` makes the track longer by the length of what
went in. `remove` without a fill makes it shorter, because what follows moves
up; `remove` with a fill leaves a gap and the length alone. `ripple` changes
it by however much the in or out point moved.

**Length does not change.** `overwrite` replaces rather than displaces.
`trim`, `roll`, `slip` and `slide` all move boundaries around within the
track without altering how much track there is. `slice` puts a cut in and
changes nothing else — two items where there was one, occupying the same
span.

**Neighbours move.** `insert`, `ripple` and `remove` without a fill. Those
are the three that shift material you did not name.

**Neighbours do not move.** `overwrite`, `trim`, `slip`, `slice` and `fill`.
`roll` and `slide` are the interesting pair: both change a neighbour, but
only by moving the boundary they share, so nothing further along the track
notices.

The sample above is `insert` and `overwrite` side by side, because that is
the distinction people reach for first and the one that decides whether a cut
gets longer.

## Filling what an edit opens

Several of these can open a hole — `overwrite` past the end of a track,
`remove` with a fill, `insert` at a point past the end.

Each takes a **fill template**: the item to put in that hole. Pass nothing
and it is an ordinary `Gap`, which is transparent, so on a multi-track
timeline whatever is underneath shows through. When you want something else —
solid black, a slate, colour bars — pass a clip with a `GeneratorReference`
as the template and that is what fills it instead.

## Slip and slide

These two are named after the editing desk and the names are worth learning
the way round.

**`slip`** keeps the item where it is on the track and moves the media inside
it: same four seconds of the cut, a different four seconds of the shot. The
track does not change at all, and neither does any neighbour.

**`slide`** keeps the media and moves the item along the track: the same four
seconds of the shot, arriving earlier or later. The time has to come from
somewhere, so the neighbours on either side absorb it — the one before grows
or shrinks, and the one after does the opposite.

## Where they are, and where they are not

All ten are in the core, in the C ABI, in every SDK generated from it, and in
Python as `opentimelineio.algorithms.overwrite`, `insert` and the rest, with a
`ReferencePoint` enum for `fill`'s four-point edits.

Upstream's own Python package does not bind them; only its C++ has them. The
Python functions here follow that C++ `editAlgorithm.h` in their names,
parameters and defaults, and raise what its error handler raises. An object an
edit takes out of a track stays usable while Python holds it, with no parent,
as any removed child does.

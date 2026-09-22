---
title: What this is
summary: A pure-Rust OpenTimelineIO, and a binding for every language it is generated into.
section: Start here
order: 1
---

[OpenTimelineIO](https://opentimeline.io) is how editorial decisions move
between programs. A cut assembled in one tool, a conform in another, a review
in a third: each holds the same idea — these shots, in this order, trimmed
here, at this rate — and OTIO is the written form of that idea, so the three
can agree about it without agreeing about anything else.

This library is a port of it to Rust. Not a reimplementation with its own
ideas about how timelines should work: the arithmetic, the rounding and the
timecode behaviour follow upstream exactly, measured against upstream's own
test suites, because a file written here has to open unchanged in every tool
that already reads OTIO.

## What you get

The whole data model, the time math beneath it, and the interchange formats
post actually uses:

| Piece | What it does |
| --- | --- |
| `opentime` | Rational time, time ranges, SMPTE timecode, drop-frame and all |
| `otio-core` | The schemas, the compositions, the algorithms, and `.otio` JSON |
| `otio-ale` | Avid Log Exchange |
| `otio-cmx3600` | CMX 3600 Edit Decision Lists |
| `otio-fcp7` | Final Cut Pro 7 interchange XML |
| `otio-fcpx` | Final Cut Pro X XML |
| `otio-aaf` | AAF, read and written as upstream's adapter writes it |

Everything above sits on Rust and nothing else. There is no C++ library
underneath and no Python interpreter: a binding links one static library.

## Why the bindings all agree

Each language SDK is generated from the C interface rather than written by
hand, and that interface describes itself — the generator reads the Rust
source of the ABI, not a header and not a schema somebody maintains alongside
it. So a call added to the core is a call every SDK has, and a call whose
meaning changed cannot keep an old signature in one language and a new one in
another. Drift fails the build.

That is also why this site can promise a switcher rather than a paragraph
apologising for the languages it did not get to.

<!-- ::sample id="read-an-edl" -->

Reading is the same shape everywhere: reading a file hands back the object it
is about, and you ask that object what is under it. Only Rust, C and Zig show
you the document that owns the objects on the way; every other SDK keeps it
out of sight, as upstream's own bindings do. What differs is what each
language calls a failure and what it calls an absence — an `error` in Go, a
`throws` in Swift, an `NSError` in Objective-C, an optional in Zig — and the
SDK for each follows that language rather than the C interface it came from.

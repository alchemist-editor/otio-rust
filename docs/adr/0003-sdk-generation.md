# ADR 0003: How the language SDKs are made

- **Status:** Accepted
- **Date:** 2026-09-22
- **Deciders:** Jeff Hodges

## Context

Everything above the Python bindings sits on `otio-capi`. Jeff wants SDKs for
Zig, WASM with TypeScript, Swift, Go and C++, each idiomatic to its language,
each carrying real documentation and types, and all of them easy to keep up to
date as the core grows.

Six hand-written bindings is six surfaces that drift from the core the moment
anyone adds a function to the C ABI — and drift a user discovers before CI
does. The C ABI has 260 entry points today and is not finished: AAF is not in
its format list yet, and the schemas the Python thread is still working
through will add more.

So the SDKs are generated. The question this decides is what they are
generated *from*, and how drift is made to fail a build rather than surprise
someone.

## Decision

**The Rust source of `crates/otio-capi` is the source of truth.** A crate,
`otio-sdk-model`, reads it and writes `sdk/api.json`. A second crate,
`otio-sdk-gen`, has one backend per target language and reads that
description. Both the description and the generated SDKs are committed.

### Why the Rust source

Three candidates were considered.

**The Rust source.** It *is* the library: a function exists because it is
written there, and its doc comment is the one a Rust user already reads. It
cannot be out of date with itself.

**The C header.** It is a second statement of the same interface, which is
exactly why `otio-capi/tests/header.rs` exists — a second statement can
disagree with the first. Generating from it would mean the SDKs inherit
whatever the header gets wrong in the window before that test is run.

**An interface file written alongside both.** A third statement, needing its
own check to stay honest, and a second place to remember when adding a
function.

The Rust source wins on the thing that matters: adding a function to the C ABI
reaches every SDK by being written, with nowhere else to remember.

The one thing the Rust source does not state is how C spells a constant —
`OtioValueKind::Bool` is `OTIO_VALUE_BOOL`, not `OTIO_VALUE_KIND_BOOL`, and no
rule derived from the Rust name gets every case right. That comes from the
header, and the two are checked against each other on the way past: same
number of variants, same value for each.

### What the description carries

A flat list of C functions is not enough to produce anything idiomatic. The C
ABI is written to conventions, and those conventions carry meaning that the
description reads back out:

- what a call is a method *on* — the document, an object of a schema, or a
  value like `RationalTime`
- which calls are constructors, getters, setters, or plumbing a binding calls
  but never shows
- which parameters a caller supplies, and which exist only to receive a result
- where three parameters are really a list, and where two are a borrowed run
  of bytes
- where `OTIO_STATUS_NO_VALUE` is an answer rather than a failure
- where an argument may be absent, read from what the function body does with
  it rather than from what its prose claims
- the OTIO schema ladder, which a flat C ABI cannot express and which is
  therefore declared once and checked against `OtioNodeKind`
- who owns what crosses back: a buffer the SDK must free after copying, a
  document the caller now owns, a document the call has consumed
- where every field of a value struct sits, and how big the struct is
- what an editing call does with each object handed to it: puts it somewhere,
  or only names one that has to be there already

That is enough for a backend to emit a method on a `Clip` returning a `[]Clip`
and an `error`, rather than a free function taking six pointers.

The layouts are there for a target with no C compiler behind it. A backend
that includes the header lets the compiler place the fields; one that reaches
the library through a WebAssembly linear memory has to write a `RationalTime`
into it byte by byte, and needs to know that `rate` begins at offset 8. They
are computed by C's own rules, twice, because a pointer is four bytes on
`wasm32` and eight elsewhere and three of these structs hold one. Computing a
layout is guessing until something checks it, so the sizes are compared
against the ones `otio-capi` asserts in a `const` block the compiler
evaluates, and a disagreement stops the build.

### Placing an object, or only naming one

The last item on that list is the one that cannot be read off a signature at
all, and it is in the description because getting it wrong is silent.

Every object crossing the C ABI is an `OtioNode`, and every handle means
something only inside the document that issued it. A binding that hides the
document therefore has to decide, for each object argument, whether to move
that object into the receiver's document first or to insist it is already
there. Both defaults are wrong somewhere and neither fails loudly. Moving an
object that was only going to be named swallows the timeline it came from:
`v1.detachChild(clipFromAnotherTimeline)` absorbs that whole timeline and
then deletes the clip out of it, reporting success. Refusing an object that
was going to be placed breaks appending, which is the one thing every binding
has to be able to do.

It is per parameter, not per call, and there is no convention in the naming
to read it from. `child` is placed by `otio_composition_append_child` and
only named by `otio_composition_detach_child`; `item` is placed by
`otio_edit_insert` and only named by `otio_edit_trim`. One call wants both:
`otio_edit_insert` places `item` and `fill_template` into a `composition`
that has to be there already.

Two answers do fall out of the description and need no table. A call holding
the document as `*const` cannot put anything into it. And the object a call
is *about* — its receiver — is never the object being placed, since
`otio_item_append_effect` puts the effect in the item rather than the item in
anything. Everything else is declared in
`otio-sdk-model/src/placement.rs`, and an editing call with an object
argument missing from it stops the build rather than reaching five SDKs with
a guess in it.

### Where a backend writes a call by hand

Almost every call emits mechanically. `otio_document_absorb` does not: it
answers with a translation table as two parallel lists of handles, and the
first list's handles belong to a document the same call has just freed.
Emitted mechanically in Go that is a pair of `[]Node` half of which name
nothing, so the Go backend writes it itself, as a `map[Node]Node` from the
nodes the caller already holds to their new ones.

A call written by hand stays in the description and stays in the name-collision
check, so the hand-written version cannot quietly diverge from the call it
stands for. The escape hatch is deliberately narrow: it is a named list in the
backend, and a call that needs it and is not on the list fails the build.

### How drift fails the build

`cargo test` regenerates everything under `sdk/` and compares it with what is
committed. Seven things stop the build rather than reaching a user:

1. A C ABI function that fits none of the conventions, named in the error.
2. A schema added to the core that nobody has placed in the OTIO ladder.
3. Two calls that would collide on one type in a generated SDK. This found
   four real collisions the first time it ran, including a track's `kind`
   against the schema `kind` every object has.
4. A list call that edits the document as it answers, which cannot be called
   twice and so needs somewhere to say how big its answer will be.
5. A call that consumes a document, which no backend can emit mechanically
   because it has the caller's own handle to close as well.
6. A struct whose computed layout disagrees with the size `otio-capi` asserts
   for it.
7. Anything regenerated that differs from what is committed — including a
   reworded doc comment, so the SDKs never document an older library than they
   wrap.

Numbers 1, 4 and 5 are not hypothetical: merging `otio_document_absorb` into
this branch tripped all three in one go, and the generator stopped with the
name of the parameter it had never seen rather than leaving the call out of
every SDK.

### The naming table

Almost every call falls out of the conventions reading the way someone would
have written it by hand. A few do not:
`otio_rational_time_duration_from_start_end_time` takes two times and belongs
to neither, so making the first the receiver gives
`start.durationFromStartEndTime(end)`, which is nonsense in any language.

`otio-sdk-model/src/overrides.rs` fixes those, and only those. An entry may
change what a call is *named* and whether it hangs off a receiver. It may not
change what it takes, what it returns, or what it does. Every entry must name
a symbol that exists, so a correction cannot outlive the function it was
written for.

### The first target

Go, alongside the WASM and TypeScript target another thread is taking.

cgo is the archetypal C consumer, and Go is about as far from C as a language
with a C FFI gets: methods, multiple returns, garbage collection, slices, and
errors as values, none of which look anything like an out-parameter. Swift and
Zig are closer to C in every one of those respects, so a description rich
enough for Go is rich enough for them. Go also needs nothing installed to run
its tests in CI.

### What a new target costs

One module under `otio-sdk-gen/src/`, registered in `TARGETS`, and a CI job
that builds and tests what it writes. Nothing in the shared model or the
other backends changes to add one.

What a target may not do is prove itself only against itself. Each SDK
currently tests its own surface in its own language, which catches a broken
binding and not a binding that quietly disagrees with the others about what
the library does. The intent is a set of conformance scenarios — build this
timeline, run these edits, produce this JSON — written once and run by every
target's CI job, so a new language is compared against the existing ones
rather than only against its own expectations. Those scenarios do not exist
yet; a target added before they do carries the obligation to run them once
they land.

## Following upstream

The generated SDKs are modelled on OpenTimelineIO's own bindings, not invented.
From upstream's Python, its Swift bindings, its C++ headers and the JavaScript
bindings:

- The **type names** are upstream's, unchanged: `Clip`, `Track`, `Stack`,
  `Timeline`, `Composable`, `SerializableObjectWithMetadata` and the rest.
- The **member names** are upstream's, transliterated only for case:
  `source_range`, `available_range`, `target_url`, `global_start_time`,
  `active_media_reference_key`.
- The **accessor shape** is upstream's C++: a bare noun to read, `set_`
  prefixed to write, and no `get_` anywhere. In Go that is `SourceRange()` and
  `SetSourceRange()`.
- The **property-or-method split** — a stored field is a property, anything
  computed or fallible is a method — is the rule Python, Swift and the
  JavaScript bindings all follow, and it falls out of whether the C++
  signature takes an `ErrorStatus*`. Go has no properties, so everything is a
  method, but the naming follows.
- **Compositions are not collections.** Upstream's Swift bindings deliberately
  do not conform `Composition` to a Swift `Collection`, exposing `children`
  plus explicit throwing `append`/`insert`/`remove` instead, because
  re-parenting can fail and has side effects. Our C ABI has the same shape and
  the Go SDK keeps it.
- **Real enums**, as Swift and the C bindings have, rather than Python's bare
  string constants.

Where the generated Go differs from upstream, it is on purpose:

- **There is a `Document`, for now.** Upstream's objects own themselves; ours
  live in an arena, for the reasons in ADR 0001. So an object is a handle and
  the document it can be resolved against, and objects are built with
  `document.NewClip` rather than `Clip(...)`.

  This one is going away. `otio_document_absorb` landed while this PR was in
  review, and it is what lets a binding offer upstream's shape: every new
  object gets a document of its own and moves into the parent's when it is
  appended. Jeff decided on 2026-09-22 that the SDKs should hide the document,
  on the criterion that they be easy to use while staying idiomatic, so this
  is a departure with an expiry date rather than a settled one.

  It is deliberately not being done here. The Go and TypeScript generators are
  converging into one, and this wants writing once in that shared place rather
  than twice. It is also a rewrite of the surface rather than a call swap: the
  C ABI leaves no forwarding note, so a handle into an absorbed document is
  dead rather than redirected, and each SDK has to keep the translation chain
  itself — `crates/otio-python/src/arena.rs` is what that costs, and it gets
  off lightly by sitting on `otio_core::Document` directly.

  Where hiding the document would make some language *less* idiomatic rather
  than more, that language keeps it, and says why here.
- **Errors are Go errors**, and `OTIO_STATUS_NO_VALUE` is the sentinel
  `ErrNoValue`. Upstream Python maps onto builtin exceptions where one fits
  and Swift throws one struct carrying a status; every binding maps the same
  taxonomy onto its own mechanism, and this is Go's.
- **An absent string is the empty string**, not a `*string`. Go has no
  optional `string`, and making every optional name a pointer would be worse
  for every caller who has one.

## Consequences

- Adding a function to the C ABI costs one command — `cargo run -p
  otio-sdk-gen` — and every SDK carries it, documented.
- Adding one that breaks the ABI's conventions costs a conversation, because
  the generator stops and says so. That is the point: the conventions are what
  make an idiomatic SDK possible, and a function that ignores them would come
  out as a transliteration of C.
- The generated code is committed, so a change to the API surface is visible in
  review rather than only in the Rust that implements it.
- The C ABI's own gaps become visible. `find_clips` takes no `search_range` or
  `shallow_search` where upstream's takes both, and `OtioStatus` is a smaller
  set than upstream's `ErrorStatus::Outcome`. Both are worth closing, and both
  are the C ABI's to close rather than the generator's.

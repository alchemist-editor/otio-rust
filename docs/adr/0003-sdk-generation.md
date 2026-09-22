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

That is enough for a backend to emit a method on a `Clip` returning a `[]Clip`
and an `error`, rather than a free function taking six pointers.

### How drift fails the build

`cargo test` regenerates everything under `sdk/` and compares it with what is
committed. Five things stop the build rather than reaching a user:

1. A C ABI function that fits none of the conventions, named in the error.
2. A schema added to the core that nobody has placed in the OTIO ladder.
3. Two calls that would collide on one type in a generated SDK. This found
   four real collisions the first time it ran, including a track's `kind`
   against the schema `kind` every object has.
4. A list call that edits the document as it answers, which cannot be called
   twice and so needs somewhere to say how big its answer will be.
5. Anything regenerated that differs from what is committed — including a
   reworded doc comment, so the SDKs never document an older library than they
   wrap.

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

- **There is a `Document`.** Upstream's objects own themselves; ours live in an
  arena, for the reasons in ADR 0001. So an object is a handle and the document
  it can be resolved against, and objects are built with `document.NewClip`
  rather than `Clip(...)`. The WASM thread is adding `otio_document_absorb` so
  that a binding can offer upstream's shape — build an object on its own, put
  it somewhere later — and the generator will pick that up when it lands.
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

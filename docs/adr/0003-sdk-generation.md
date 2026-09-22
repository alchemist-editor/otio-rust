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

### Swift

The Swift SDK is modelled on
[OpenTimelineIO's own Swift bindings](https://github.com/OpenTimelineIO/OpenTimelineIO-Swift-Bindings),
and copies their shape: a class per schema deriving as the schemas derive,
values as structs that are `Equatable` and `Hashable`, real enums, `throws`
with one error type carrying a status, and compositions that are deliberately
*not* Swift collections — `children()` plus throwing `append`/`insert`/
`remove`, because re-parenting can fail and has side effects. Upstream's
`sourceRange: TimeRange?` and our optional for `OTIO_STATUS_NO_VALUE` are the
same idea arrived at twice.

It carries the visible `Document` described above, and will lose it with
every other SDK when the shared generator hides it.

Where it departs, and why:

- **A fallible getter is a method, not a property.** The rule above — a
  stored field is a property, anything computed or fallible is a method —
  lands differently under an arena, because an object is a handle and
  *every* read of one can fail on a handle that no longer resolves. Swift
  has no throwing property, so on objects every getter is a `throws`
  method: `try clip.sourceRange()`, where upstream writes
  `clip.sourceRange`. On the value structs and the enums, where the C ABI
  says the call cannot fail, a getter stays a property, so `time.toSeconds`
  and `format.name` read as upstream's do. The rule is the same one; it is
  the C ABI that answers it differently.
- **A call answers with the base class, and the object's real class carries
  the schema.** Upstream's C++ types `media_reference()` as a
  `MediaReference*`; the C ABI hands back an untyped handle, so the
  generated signature says `SerializableObject`. Every handle that comes
  back is built as the class its schema names, so `as? ExternalReference`
  tells the truth and `for case let clip as Clip in try track.children()`
  reads the way Swift reads. A constructor is typed, because the
  constructor knows what it built: `document.newClip` answers with a `Clip`.
- **Equality is `==`, not `===`.** Upstream keeps one wrapper per object in
  a cache, so `===` is object identity. Here a handle is a value and
  several wrappers for one object are ordinary rather than a bug, so
  `SerializableObject` is `Hashable` on the document it belongs to and the
  handle itself, and `==` is the question worth asking. It is also what
  makes the dictionary `absorb` answers with usable.
- **Argument labels are mechanical**: the first argument carries no label,
  every later one is labelled with its name from the C ABI. Upstream picks
  labels by hand — `transformed(time:toItem:)` — and a generator cannot,
  short of an overrides table with an entry per call that nobody would keep
  up to date. The rule gives `range.overlaps(other, epsilonS: 0.5)` and
  `clip.setMediaReference("main", reference: media)`, which is close enough
  to read as Swift.
- **A call with two results answers with a labelled tuple**, so
  `composition.neighborsOf` gives `(before:after:)` and `item.color()` gives
  `(color:name:)` rather than out-parameters.
- **The free functions hang off an `OTIO` namespace**, because Swift has no
  package scope and a top-level `version()` would land in every file that
  imports the module.
- **A value struct's text field is empty rather than absent**, as in Go: a
  `String?` for every optional name in a struct would be worse for every
  caller who has one.
- **The package finds `libotio` on the command line.** SwiftPM's
  `.unsafeFlags` would embed a library search path in the manifest and make
  the package unusable as anyone's dependency, so the static library is
  built by cargo, copied into `sdk/swift/lib`, and pointed at with
  `swift build -Xlinker -L"$PWD/lib"`. The header is not copied: the module
  map reads the one in `crates/otio-capi/include` where it lives, so the
  package cannot describe an older interface than the library.

### C++

The C++ SDK is modelled on
[OpenTimelineIO's own C++ library](https://github.com/AcademySoftwareFoundation/OpenTimelineIO/tree/main/src/opentimelineio),
which is the reference implementation rather than a binding, and copies its
shape: a type per schema deriving as the schemas derive, `snake_case`
members, `RationalTime`/`TimeRange`/`TimeTransform`/`V2d`/`Box2d` as value
types with the same members and the same arithmetic, `std::optional` where
upstream uses it, and compositions that are deliberately not standard
containers — `children()` plus `append_child`/`insert_child`/`remove_child`,
because re-parenting can fail and has side effects.

It is header-only. Everything the SDK adds is a thin call into `libotio`, so
there is nothing to compile separately, and `#include
<opentimelineio/otio.hpp>` plus linking the static library is the whole
integration. It carries the visible `Document` described above, and will lose
it with every other SDK when the shared generator hides it.

Where it departs, and why:

- **A fallible call throws; there is no `ErrorStatus *`.** Upstream threads
  an `ErrorStatus *` through every call that can fail and leaves checking it
  to the caller, which is a C++98 habit the library has kept for ABI reasons
  it has and we do not. Ours throws `otio::Error`, which derives from
  `std::runtime_error` and carries the `Status`. It is the same decision that
  gave Go an `error` and Swift `throws`: the C ABI returns a status from
  every call, and a binding that made ignoring it the easy path would be
  worse than the C.
- **`OTIO_STATUS_NO_VALUE` is `std::nullopt`, and on a call with nothing to
  return it throws.** `item.source_range()` answers
  `std::optional<TimeRange>` because there is a value to be absent. A call
  that returns `void` has no `nullopt` to answer with, so its no-value
  arrives as an `otio::Error` whose `status()` is `Status::NO_VALUE` — an
  answer rather than a failure, as Go's `ErrNoValue` and Swift's
  `.noValue` are.
- **Objects are values, and `is<T>()`/`as<T>()` replace `dynamic_cast`.**
  Upstream's objects are reference-counted `SerializableObject *`, and its
  callers write `dynamic_cast<Clip *>(child)`. Here an object is a handle
  into an arena, so the natural C++ for it is a small value type holding the
  document and the handle — copyable, comparable, nothing to delete. That
  leaves no polymorphic class for `dynamic_cast` to work on, so the schema
  question is asked directly: `child.is<Clip>()` reads
  `otio_node_kind`, and `child.as<Clip>()` answers a
  `std::optional<Clip>`. `is_a(SchemaKind)` is the same question against the
  derivation table, matching upstream's `SerializableObject::is_equivalent_to`
  neighbourhood.
- **Equality is on the document and the handle.** Upstream compares
  pointers. A handle is a value here and two copies of one naming the same
  object are ordinary rather than a bug, so `operator==` compares the
  document pointer and the handle, as Swift's `==` does. There is no
  `std::hash` specialisation: hashing would have to promise stability across
  a generation bump, which a stale handle deliberately does not have.
- **The plumbing is public.** `document()` and `handle()` are public members
  and `detail::` is a public namespace, because C++ has no module-internal
  access and the alternative is a `friend` declaration per generated type.
  They are documented as plumbing and named so that nobody reaches for them
  by accident.
- **A call with two results answers with a small named struct**, declared
  inside the type that returns it — `Item::ColorResult`, with `color` and
  `name` — rather than `std::pair` or out-parameters, so the fields have
  their names at the call site.
- **An enum constant keeps as much of its C name as it needs to be its
  own.** The C++ name is the C one without the prefix its type already says,
  so `OTIO_STATUS_NO_VALUE` is `Status::NO_VALUE`. Where that would land on
  a name a standard macro has taken, a segment of the prefix goes back on,
  because a macro is replaced before the compiler sees the declaration:
  `OTIO_VALUE_NULL` is `ValueKind::VALUE_NULL`, not `NULL`.
- **Declarations and definitions are split.** Every class body is emitted
  first, in `values.hpp` and `objects.hpp`, and every `inline` definition
  after it in `calls.hpp`, because a header-only SDK of mutually recursive
  types cannot be written in one pass: `Track::children()` answers objects
  whose own calls answer `Track`s.

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

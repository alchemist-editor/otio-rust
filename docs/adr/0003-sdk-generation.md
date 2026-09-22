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
- where a failing call writes its message: the `out_error` buffer every
  status-returning call takes last, and no other call takes
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

The message arrives from the call that failed, in that `out_error` buffer,
and not from a second call asking for the last failure. The first version of
the C ABI kept it per thread and had a binding read it back afterwards, which
is invisible from C and bites every runtime above it that can move work
between OS threads: Go had to pin every goroutine to its thread across the
pair, and Swift's async tasks, .NET's thread pool and a Node worker would all
have needed the same care. Handing the message back with the status fixed it
once for every target
([#63](https://github.com/alchemist-editor/otio-rust/issues/63)), and the
description refuses a status-returning call without `out_error` last, so a
new entry point cannot bring the problem back.

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

### Which document a call is made in

Hiding the document does not make the C ABI stop wanting one. Every call
still takes an `OtioDocument *`, and with the caller no longer supplying it
the only place left to get one is the objects the call was handed. Exactly
one of them can be that object, and which it is is not free choice: a
parameter the call `Require`s cannot be moved, so the call has to happen
where that one already is.

The rule is the receiver where there is one; otherwise the first object
parameter the call requires; otherwise the first object parameter at all. A
list counts even when the description marks it optional, because that mark
means the pointer may be null and the call is still about the objects it is
given.

Getting it wrong is not silent, but it is late and confusing: the call is
made in the wrong document and then refuses one of its own arguments for
being somewhere else. It shipped that way in TypeScript — `edit.insert`,
`edit.overwrite` and `edit.fill` anchored on the item they adopt, so building
a clip and inserting it into an existing track threw, which is the one thing
hiding the document is for. So this is `Param::anchor`, beside `placement`,
worked out once in `otio-sdk-model` and read by every backend.

A call that writes a whole timeline out is the one thing left with no object
to find a document by. Those take a root object and start writing there, so
writing a track rather than the timeline around it is a thing you can ask
for.

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

That turned out to be true of both. Swift and Zig each landed as one more
module reading the same `Api`, with nothing added to the description: the
struct layouts, the `optional` flag, the `sized_by` link and the ownership
notes were already there for Zig, which is the one of the two with no upstream
binding to copy.

### What a new target costs

One module under `otio-sdk-gen/src/`, registered in `TARGETS`, and a CI job
that builds and tests what it writes. Nothing in the shared model or the
other backends changes to add one.

What a target may not do is prove itself only against itself. Each SDK
currently tests its own surface in its own language, which catches a broken
binding and not a binding that quietly disagrees with the others about what
the library does. The intent is a set of conformance scenarios — build this
timeline, run these edits, produce this JSON — written once and rendered by
every target, so a new language is compared against the existing ones rather
than only against its own expectations.

Not every scenario applies to every target, and the split is in the scenario
data rather than each backend's judgement. Some scenarios are about the C
ABI's semantics and apply to everyone: a stale handle after a remove, a list
drawn from two documents. Others are about what hiding the document obliges
a binding to do, and are meaningless against a target that keeps it visible,
where a foreign object is simply an error and the placement question never
arises. The same fork decides whether a target consumes the placement table
at all, so it is one property of the target — hides the document, or does
not — rather than two switches that can disagree.

Those scenarios do not exist yet; a target added before they do carries the
obligation to run the ones that apply to it once they land.

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

- **There is no `Document`.** Upstream's objects own themselves; ours live in
  an arena, for the reasons in ADR 0001. The arena is not in the surface: a
  new object is built in one of its own, appending it moves that arena into
  the parent's, and what a caller holds is objects. So `otio.NewClip("A")`
  and `track.AppendChild(clip)`, not `document.NewClip`.

  Jeff decided this on 2026-09-22, on the criterion that the SDKs be easy to
  use while staying idiomatic. It rests on `otio_document_absorb`, and
  because the C ABI leaves no forwarding note, on each binding keeping the
  translation chain itself: an arena that has been absorbed remembers what it
  moved into and what each of its handles became, so an object held from
  before a move is followed rather than left dead.

  Where hiding the document would make some language *less* idiomatic rather
  than more, that language keeps it and says why here. Zig is the one that
  does; see below.
- **An object from another timeline is refused, and refused early.** A call
  that only names an object gets `ErrOtherTimeline`, a sentinel rather than a
  bare string, so a caller can tell it from a failure the library reported.
  The refusal is made before the library is asked: absorbing first and
  failing afterwards would already have merged the two timelines, which is
  the damage the refusal exists to prevent.
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

There is no `Document`, for the reasons above: a schema is built with an
initializer of its own — `try Clip(name: "shot_01")` — and joins a timeline
when it is appended. Swift will not take an initializer in an extension of
the class it builds, so those are the one thing the generator writes into the
class bodies rather than beside them. A subclass giving an initializer the
same shape as one it inherits is a redeclaration and not an override, which
is what lets `Clip(name:)` and `Item(name:)` both exist.

Go, TypeScript and C++ lost theirs first, and C# and Objective-C followed.
Zig is the one target that keeps it, for the reason set out below.

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
  reads the way Swift reads. An initializer is typed, because it knows
  what it built: `Clip(name:)` is a `Clip`.
- **Equality is `==`, not `===`.** Upstream keeps one wrapper per object in
  a cache, so `===` is object identity. Here a handle is a value and
  several wrappers for one object are ordinary rather than a bug, so
  `SerializableObject` is `Hashable` on the arena it belongs to and the
  handle itself — both resolved through the move chain first, so two
  wrappers issued either side of an absorb still compare equal.
- **Argument labels are mechanical**: the first argument carries no label,
  every later one is labelled with its name from the C ABI. Upstream picks
  labels by hand — `transformed(time:toItem:)` — and a generator cannot,
  short of an overrides table with an entry per call that nobody would keep
  up to date. The rule gives `range.overlaps(other, epsilonS: 0.5)` and
  `clip.setMediaReference("main", reference: media)`, which is close enough
  to read as Swift. An initializer is the exception and labels every
  argument, because it has no name of its own to say what the first one is:
  `Clip(name:)`, not `Clip(_:)`.
- **A call with two results answers with a labelled tuple**, so
  `composition.neighborsOf` gives `(before:after:)` and `item.color()` gives
  `(color:name:)` rather than out-parameters.
- **The free functions hang off an `OTIO` namespace**, because Swift has no
  package scope and a top-level `version()` would land in every file that
  imports the module. What the C ABI hung off the document and is not about
  one of the objects it is handed lands there too: `OTIO.open`, `OTIO.save`,
  `OTIO.readFromFile`, the algorithms and the ten edit operations.
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
integration. There is no `Document`, for the reasons above: a schema is built
with a static `create` of its own — `otio::Clip::create("shot_01")` — and
joins a timeline when it is appended. A static rather than a constructor
because C++ hides an inherited static behind one of the same name, so
`Clip::create` and `Item::create` do not collide the way two ordinary members
on one line of descent would. It was the first of the compiled SDKs to make
that move.

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
- **Objects own the arena between them.** An object must never copy the raw
  `OtioDocument *`: a copy outliving the release is a use-after-free, because
  the C interface can refuse a null document but cannot tell a freed one from
  a live one. It reads the pointer out of a wrapper at the call instead, as
  Go and Swift do. With no `Document` left to hold the arena, the objects
  hold it: `std::shared_ptr<detail::Arena>`, and the arena goes when the last
  object naming it does. That is strong where the earlier design was weak,
  and it is what hiding the document means — there is nobody else left to own
  it. `close()` still ends a timeline early, and it is safe for the same
  reason as before: it nulls the pointer, and the C interface refuses null.
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

### C#

There is no upstream OpenTimelineIO binding for C#, so nothing here is copied
from one. What things are *called* still follows upstream — the schema names,
the member names, the bare-noun getter and the `Set` prefix, spelled in .NET's
PascalCase — and the nearest precedent for the *shape* is upstream's Java
bindings, which are the other managed language over the same C++ library: a
class per schema deriving as the schemas derive, exceptions rather than status
codes, and compositions that are not collections.

Where it departs from upstream's shape, and why:

- **Failure is an exception, not a status.** `OtioException` carries the
  `Status`, as upstream's Java `OpenTimelineIOException` carries its error.
  This is the same decision Go, Swift and C++ made: the C ABI returns a status
  from every call, and a binding that made ignoring it the easy path would be
  worse than the C.
- **`OTIO_STATUS_NO_VALUE` is `null`, not an exception.** `item.SourceRange()`
  answers `TimeRange?` and a clip with no active media reference answers
  `null`, because C# has nullable value types and "there is nothing here" is
  an answer rather than a failure. A call with nothing to answer *with* throws
  it instead, as it does in Go and C++.
- **Nothing is `unsafe`.** Every `DllImport` takes blittable arguments,
  strings cross as `IntPtr` allocated by `Marshal.StringToCoTaskMemUTF8` and
  freed as the call unwinds, and lists cross as `[In, Out] T[]`. A C# SDK
  that needed `/unsafe` would be one a good many callers could not build.
- **A call with two results answers a tuple.** `Item.Color()` answers
  `(Color color, string name)?` rather than taking out-parameters, so the
  fields keep their names at the call site without a type per call.
- **A member may shadow a type, so generated code names types in full.**
  C# resolves a simple name among members before types, so `Item.Color()` and
  the `Color` struct collide on sight. The backend tracks which type names
  some member also answers to and writes those as
  `global::OpenTimelineIO.Color`, which is a spelling nobody has to think
  about and a rule that cannot be forgotten.
- **`Equals` and `==` compare the timeline and the handle.** A handle is a
  value here, so two wrappers naming one object are ordinary rather than a
  bug. The generated `Equals(SerializableObject)` from `otio_node_equal` asks
  the same question through the library; the runtime's `Equals(object?)`
  answers it without a call, so it still works once the timeline has gone.
  Both resolve the handle first, so a wrapper held from before a move still
  compares as the object it names.

There is no `Document`, for the reason given above. What that costs C#, and
what it buys:

- **A constructor is a real constructor.** `new Clip("shot_01")` rather than
  a static factory, which is what a C# caller reaches for and what the
  TypeScript SDK already spells as `new Clip({name})`. C# does not inherit
  constructors, so `Clip(string?)` and `Item(string?)` cannot collide the way
  two ordinary members on one line of descent would — the problem Swift had to
  reason about does not arise here at all.
- **A base constructor runs before the body**, so the work of building the
  object cannot happen inside the constructor that needs its answer. Each
  schema gets a `private static Site MakeClip(...)` beside its constructor,
  which makes the arena, calls the library and hands back both; the
  constructor is the one line that passes that to its base. The schema classes
  are `partial` already, so this needs no new file.
- **The objects own the arena between them.** An object holds a reference to
  it and .NET releases it when the last one goes, with a finalizer behind that
  so forgetting is a delay rather than a leak. `Close()` stays, for releasing
  a large timeline at a moment the caller chose: it zeroes the pointer, and
  the C interface refuses a null document, so every object that lived there
  fails with `Status.NullPointer` rather than reading freed memory. Nothing is
  `IDisposable` any more, because there is nothing a caller has to remember.
- **Reading and writing are statics on `Otio` over a root object.**
  `Otio.Open(path)` answers the object the file is about, and
  `Otio.Save(root, path)` writes from the object it is given, so handing it a
  track writes that track. The four calls the C ABI hung off the document that
  are really about an object — `IsLive`, `DeepClone`, `RemoveFromTimeline`,
  `RemoveFromTimelineRecursive` — are members of `SerializableObject`.

### Objective-C

Objective-C is the other target with no upstream binding to copy. What things
are *called* still follows upstream; what the binding *is* follows Cocoa,
because the language has one house style and a library that ignores it is a
library nobody can read.

- **A class per schema**, deriving as the schemas derive, prefixed `OTIO`
  because the language has no namespaces and two-letter prefixes are Apple's.
  Every handle the library hands back is built as the class its schema names,
  so `isKindOfClass:` asks what an object really is and gets a true answer.
- **Values are C structs**, as `NSRange` and `CGRect` are, with
  `OTIORationalTimeMake` to build one and C functions to compute with one. A
  value type with no object identity is a struct in this language, and what
  you do to it is a function, not a message.
- **Failure is an `NSError` out-parameter** in `OTIOErrorDomain`, whose code
  is the `OTIOStatus`. A call that can fail answers `NO` or `nil` and fills it
  in, which is what every Cocoa call that can fail does.

Where it departs, and why:

- **`OTIO_STATUS_NO_VALUE` is an error you can tell apart, not an optional.**
  Every other SDK makes it an absent value. Here a method answering a struct
  has no `nil` to answer with, and boxing every `OTIOTimeRange` in an
  `NSValue` to gain one would cost every caller for the sake of a few calls.
  So "there is nothing here" travels as an error whose code is
  `OTIOStatusNoValue`, and `OTIOIsNoValue` tells it apart from a real failure.
  It says it the same way in both cases, which is worth more than saying it
  two ways for the sake of the half that could be optional.
- **A call answering anything but a single object uses out-parameters, and
  says `get`.** `-[OTIOClip name:]` answers the string, because an object
  method can say "nothing" with `nil`. `-[OTIOItem getDuration:error:]`
  cannot, so the answer is an out-parameter and the name says so, as
  `-[NSURL getResourceValue:forKey:error:]` does.
- **ARC, and also manual retain and release.** Apple's runtime has ARC and
  GNUstep's legacy runtime does not, so everything the SDK owns is confined to
  the runtime and says so through three macros. CI builds both ways, on both
  runtimes, because a memory model that is only ever compiled one way is a
  memory model nobody has checked.
- **Storage is declared on the interface, not the implementation.** GNUstep's
  runtime has the fragile ABI, where an `@implementation` may not declare
  ivars of its own. So `OTIOSerializableObject` holds its handle as two
  `uint32_t`s and `OTIOArena` holds a `void *`: a caller can see them, but
  the public headers name none of the C interface's own types, and everything
  that does is in `src/OTIOPrivate.h`.
- **`NSArray` carries lists, boxing what is not an object.** A list of objects
  is an `NSArray` of them; a list of value structs is an `NSArray` of
  `NSValue`, unboxed with `OTIOTimeRangeUnboxed` and its siblings. There is no
  way to put a struct in a Cocoa collection without a box, and inventing an
  object per value type to avoid one would be a worse trade.
- **An object holds its timeline strongly.** The arena has to outlive the
  handles into it, so the objects own it between them and the last one to go
  releases it. This is Swift's choice rather than C++'s weak one, because
  Objective-C has no `weak` on the legacy runtime.

There is no `Document`, for the reason given above. What that costs
Objective-C, and what it buys:

- **A constructor is a Cocoa class factory**, not an initializer:
  `[OTIOClip clipWithName:@"shot_01" error:&error]`. Two things push it that
  way. `new` is a reserved method family, so an `-initWith…` pair would have
  the runtime reasoning about ownership of an object built by the library;
  and an initializer that fails has to `[self release]` and answer `nil`,
  which is a dance to get right once per schema under manual retain and
  release. A class factory has neither problem, and `+[NSArray
  arrayWithObjects:]` is the shape a Cocoa caller already reads. Each one
  answers the concrete class — `OTIOClip *`, not `instancetype` — so a
  subclass that inherits `+clipWithName:error:` cannot claim to have built
  itself.
- **`OTIOArena` is a class the headers name but never hand out.** The language
  has no `internal`, and the fragile ABI above forces ivars onto the
  `@interface`, so the type has to be visible for an object to hold one. It
  declares no methods and no properties; every call that touches one is in
  `src/OTIOPrivate.h`, and nothing in the public surface takes or answers one.
- **`-close` stays, and empties the arena rather than the object.** It is for
  releasing a large timeline at a moment the caller chose: it zeroes the
  pointer, and the C interface refuses a null document, so every object that
  lived there fails with `OTIOStatusNullPointer` rather than reading freed
  memory.
- **Reading and writing are C functions over a root object.** `OTIOOpen(path,
  &error)` answers the object the file is about, and `OTIOSave(root, path,
  &error)` writes from the object it is given, so handing it a track writes
  that track. They are functions because the language has no namespace to hang
  a static on and a class existing only to own two class methods would be
  worse. The four calls the C ABI hung off the document that are really about
  an object — `-isLive`, `-deepClone:`, `-removeFromTimeline:`,
  `-removeFromTimelineRecursive:` — are methods on `OTIOSerializableObject`.

## Zig

Zig is the first target with no upstream OpenTimelineIO binding to copy; C#
and Objective-C above are the others. What
things are *called* still follows upstream's Python and Swift — the schema
names, the member names, the bare-noun getter and the `set` prefix, spelled in
Zig's own case — and what the binding *is* had to be decided here. Jeff's
criterion was easy to use while still being idiomatic, and where the two pull
apart, this is where it landed and why.

### The document stays visible

This is the one place Zig deliberately parts company with the other SDKs.

Everywhere else the document is being hidden: a clip is built on its own and
adopts a document when it is appended, which is upstream's shape. That rests
on `otio_document_absorb`, and because the C ABI leaves no forwarding note, on
each binding keeping the translation chain itself — in Go and TypeScript a
finalizer and a cache of live handles, in Swift a class `deinit`, in the
Python crate `arena.rs`.

Zig has neither a finalizer nor a garbage collector, and hiding the document
would mean paying for both by hand:

- Every free-floating object would own a document, so building a clip would
  allocate, and every clip would need a `deinit` that a caller has to
  remember — and that has to become a no-op once the clip has been appended.
- A handle would have to change when somebody *else* absorbs its document, so
  an object could no longer be a value. It would be a pointer to a mutable
  cell, and Zig values are copyable with nothing to hook, so copying one would
  silently make a second owner of the same cell.
- Both of those are hidden control flow and hidden allocation, which is the
  one thing Zig is most consistently against.

And the alternative is not a compromise. A document is an arena, and a Zig
programmer already holds arenas and hands them to the things that allocate
from them. `Clip.init(document, "A")` reads exactly like
`std.heap.ArenaAllocator` does, `defer document.deinit()` frees a whole
timeline at a moment the caller chose, and nothing is hidden. Hiding it would
make this target *less* idiomatic, not more, which is the condition the
decision of 2026-09-22 set for a target keeping it. Zig is so far the only
target that meets it: Go, Swift, C++, TypeScript, C# and Objective-C all
hide the document.

`absorb` is therefore an ordinary call rather than the backbone, and it is
still written by hand: it takes `*?*Document` so that a `defer` that frees the
source does the right thing after the source has been consumed, and it answers
with a slice of old-handle/new-handle pairs. Every handle into an absorbed
document is dead afterwards, and the `from` side of that slice is for matching
against handles the caller holds, never for calling.

The classification the other backends read — `placement`, for adopting an
argument versus requiring it to be local, and `anchor`, for which object's
document a call is made in — does not reach this target: with the document in
the open, the caller says which one, and an object from elsewhere is
`error.ForeignObject` whichever the call meant to do with it.

### Nothing is marshalled

The value structs *are* the C structs. Zig's `extern struct` is laid out by
C's rules, so `RationalTime` crosses the boundary as itself and there is no
conversion layer at all — no `toC`, no `fromC`, no allocation. The layouts the
description computes are written out as `comptime` assertions beside each
struct, so Zig's idea of the layout and the description's are checked against
each other and a disagreement stops the build.

That check is only as wide as the description, which carries one layout for
32-bit pointers and one for 64-bit, both computed for an ABI that aligns a
64-bit scalar to eight bytes. Pointer width is not the whole ABI: `i386`
aligns a `double` to four, so the layouts would be wrong there rather than
merely unmet. The Zig package says which targets it describes, in a
`@compileError` ahead of the assertions, instead of asserting offsets it has
no reason to believe. Widening it means a third layout in the description,
which every SDK would then share.

For the same reason there is no `@cImport` and no header is read at build
time. The `extern fn` declarations are written out directly, which means the
package builds wherever Zig builds and cross-compiles with nothing but the
static library.

### Memory is the caller's, and so is the allocator

Anything the library hands back that has to be freed — a name, a JSON
document, a list of children — is copied into an allocator the caller passes
and freed with `allocator.free`. A call that needs one takes it as its first
argument after the receiver.

The alternative considered was a wrapper type holding the library's own buffer
with a `deinit` on it, which would save a copy. It was rejected: it puts a
type between the caller and a `[]u8` for a saving nobody will measure, and
"if it allocates, it takes an allocator" is the one rule Zig's standard
library holds to everywhere. A call that answers with several things at once
hands back a small record with a `deinit(allocator)` of its own.

Text going the other way is `[:0]const u8`, so a string literal passes
straight through with no allocation and no NUL-terminating copy.

### "No value" is an optional

`OTIO_STATUS_NO_VALUE` is not a failure, and Zig is the first target with a
type that says so exactly. An item's source range is `Error!?TimeRange`: the
error union is for failure, the optional is for "there is nothing here". Go
needed a sentinel error for this and Python `None`; here it is the language's
own answer. A call whose only answer *is* whether there was anything —
`metadata.remove` — hands back a `bool` rather than `?void`.

`Error` is a Zig error set, one member per `OtioStatus`. A Zig error carries
no payload, so the sentence the library left is read separately with
`lastErrorMessage()`, the way `errno` is; the generated documentation says so
on the package.

### The schema ladder is written out

Zig has no inheritance, and since 0.15 no `usingnamespace` to stand in for
one. So a method of `Item` is a method of `Clip` because the generator writes
it there: each type carries a `Node` and declares, in full, every method of
every schema it derives from, forwarding one hop to the type that declares it.

That is about seven hundred forwarding declarations, and it is the place a
generator earns its keep: the duplication is free to write, free to keep
correct, and it is what makes `clip.duration()` work and show up in the
documentation of `Clip` rather than in a base class a reader has to go and
find. Zig analyses only what is reached, so the test suite walks every
declaration in the package to make sure all of it compiles.

The collision check is the same one the Go backend makes for its embedding,
because it is the same condition: two calls of one name on schemas where
either derives from the other.

### Two things Zig would not let this SDK say

Both are the language's constraints rather than opinions about the interface,
so both are tables in the Zig backend rather than in `overrides.rs`, where
they would change every other SDK:

- `otio_transition_type` would be declared as `type`, which is one of Zig's
  primitives. It is `transitionType`, which is what upstream's Python calls
  it anyway, and its setter follows so the pair still reads as a pair.
- Zig counts a parameter that shares a name with a declaration on the same
  type as shadowing it. Almost every case is a setter whose argument is named
  after the property it writes, so those take `new_` in front —
  `setName(new_name)` — which is what someone would have written by hand. The
  one exception, a `fill` argument that collides with the `fill` edit
  operation, is named for what the C interface's own prose says it does.

A call whose name Zig will not accept and that is not in the table stops the
build with its symbol, the same way an unclassifiable function does.

## Consequences

- Adding a function to the C ABI costs one command — `cargo run -p
  otio-sdk-gen` — and every SDK carries it, documented.
- The description was rich enough for Swift and for Zig without being changed,
  which is the claim the Go backend was chosen to test.
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

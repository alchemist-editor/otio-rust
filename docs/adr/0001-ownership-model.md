# ADR 0001: Ownership model for the OTIO object graph

- **Status:** Accepted
- **Date:** 2026-09-21
- **Deciders:** Jeff Hodges

## Context

Upstream OpenTimelineIO's C++ core owns its object graph with two mechanisms
working together:

- **Intrusive reference counting.** `SerializableObject::Retainer<T>` holds a
  refcount inside the object itself. A `Composition` holds
  `std::vector<Retainer<Composable>>` for its children.
- **Raw parent back-pointers.** `Composable::_parent` is a bare
  `Composition*`, set and cleared by the parent as children are added and
  removed.

So a `Track` owns its clips, and every clip points back at the track. That is a
reference cycle. Translating it directly into `Rc<RefCell<T>>` produces a graph
that never drops: safe, but it leaks every timeline the process ever opens.
Safe Rust prevents use-after-free and data races; it does not prevent leaks from
cycles. We have to pick an ownership model that avoids creating them.

Two candidates were considered.

### Option A — arena with typed indices

Every object lives in a single owning arena (a generational slot map). A
reference to an object is a small `Copy` handle — an index plus a generation
counter — rather than a pointer.

- Parent back-pointers become ordinary data. No cycle exists, because a handle
  is not an owning edge.
- Dropping the arena drops the whole graph, once, deterministically.
- A stale handle is detectable: the generation counter no longer matches, so a
  lookup returns `None` instead of resurrecting a recycled slot. This is the
  failure mode that would be a use-after-free in the C++.
- The graph is a contiguous allocation, so traversal is cache-friendly and the
  whole structure is trivially `Send`.
- Handles are integers, which cross an FFI boundary as-is.

The cost is ergonomic: you need the arena in hand to resolve a handle, so
almost every accessor takes an `&Arena` or `&mut Arena`. That shapes the public
API of the core and of every binding built on it.

### Option B — `Arc<RwLock<T>>` with `Weak` parents

Children are held as `Arc`, parents as `Weak`, which breaks the cycle.

- Closer to the C++ structure and to what Python users expect, so a drop-in
  Python binding is easier to reach.
- Costs a lock acquisition per access, and an allocation per node.
- The `Weak` parent link must be upgraded on every use, so parent traversal is
  fallible in a way that is noisy at every call site.
- Deep mutation of a nested tree behind `RwLock` is genuinely unpleasant to
  write, and deadlock becomes possible once two subtrees are locked at once —
  a bug class the arena does not have.
- `Arc<RwLock<T>>` graphs do not cross an FFI boundary cleanly, which matters
  because language bindings are the point of this project.

## Decision

**Use the arena model (Option A).**

Jeff's instruction was to take the most memory-safe approach available. Both
options are memory-safe in the strict Rust sense. The arena is the one that
also eliminates leaks-by-cycle structurally rather than by convention, turns
dangling references into a checked `None` rather than a silent wrong answer,
and removes the deadlock class that comes with per-node locks.

That it is also the better substrate for FFI bindings — the stated end goal of
the project — settles it.

## Consequences

- The core crate exposes an arena type that owns a timeline's object graph.
  Accessors take the arena; handles alone are inert.
- Handles are `Copy`, comparable and hashable, which makes algorithms that need
  object identity (the edit operations, `flatten_stack`) straightforward.
- The Python binding will need a wrapper layer that pairs a handle with an
  `Arc` of its arena, so that Python objects behave the way Python users
  expect. This is a known, contained cost, and is cheaper than retrofitting an
  arena later.
- The C ABI can hand out opaque `u64` handles directly.
- This ADR governs `otio-core`. It does not affect `opentime`, whose types are
  plain `Copy` values with no graph at all.

## Notes

`opentime` is being built first and is unaffected by this decision, so work can
proceed while the arena details are settled in code review.

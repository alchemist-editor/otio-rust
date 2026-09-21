# otio-python

Python bindings for the Rust core, built with [PyO3](https://pyo3.rs).

The package they build is called `opentimelineio`, and the name is the point:
the aim is a drop-in replacement for upstream OpenTimelineIO's Python package,
so that code written against upstream runs against this one without changes.
That is a strong claim, so it is measured the strongest way available —
upstream's own test files are vendored in [`tests/upstream`](tests/upstream)
and run unmodified.

| Upstream test file | Result |
| --- | --- |
| `test_opentime.py` | 83 of 83 passing |
| `test_composable.py` | 4 of 4 passing |
| `test_effect.py` | 8 of 8 passing |

Alongside them, [`tests/bindings`](tests/bindings) covers what these bindings
have to do that upstream's C++ does not: moving an object from one document
into another when it is appended to something.

## What is bound so far

`opentimelineio.opentime`, in full: `RationalTime`, `TimeRange` and
`TimeTransform`, with the module-level helpers (`from_frames`, `to_timecode`
and the rest) carried over from upstream's `opentime.py` as-is.

`opentimelineio.core`'s `SerializableObject`,
`SerializableObjectWithMetadata`, `Composable` and `Item`, and
`opentimelineio.schema`'s `Gap`, `Marker`, `Effect`, `LinearTimeWarp` and
`FreezeFrame`, with names, write-through metadata, write-through `effects` and
`markers` lists, `is_equivalent_to`, `str()` and `repr()`; and `Color`, `V2d`
and `Box2d`, the value types metadata can hold.
`opentimelineio.adapters.otio_json` reads and writes any of them, and — as
upstream's does — any list or plain value as well.

Not yet: `Clip`, `Track`, `Stack`, `Timeline` and the media references, plus
`copy`/`deepcopy` and `Color`'s named constants. They are all built on what is
here — a wrapper is a node in a document and nothing else — so each is a
constructor, some properties and a line in the dispatch that turns a parsed
node back into the right Python class. Upstream's `test_item.py` is the next
measure, and it needs a `Track` and a `Timeline` before it can run.

## Building

```sh
pip install .
python tests/run_upstream_tests.py
```

`pip install .` runs [maturin](https://www.maturin.rs), which builds the Rust
extension module and packages it with the Python sources under `python/`.

## Imitating upstream on purpose

A binding that is *nearly* the same is worse than one that is obviously
different, because the difference surfaces as a bug in somebody else's code
rather than as an import error. So the odd corners of upstream's pybind11
bindings are reproduced here rather than tidied:

- **Operators raise `TypeError` instead of returning `NotImplemented`.**
  `time < -1` is an error, not `False`. Upstream type-checks every operand by
  hand and its tests pin the behaviour.
- **`+=` does not mutate.** A time is a value; mutating one in place would
  reach every other name bound to it. Upstream builds a new object and
  rebinds. There is deliberately no `__iadd__` here, so Python falls back to
  `__add__` and rebinds, which is the same thing by a shorter route.
- **`str()` and `repr()` are formatted with C's `%g`.** `opentime::cfmt` is
  public for this reason: a binding that prints `100000000000000000000` where
  upstream prints `1e+20` is not a drop-in replacement.
- **`TimeRange` takes either two times or a start, a duration and a rate.**
  Upstream offers both and its tests use both.
- **Error messages are upstream's, character for character.** They reach
  Python as the text of a `ValueError`, and one upstream test compares one of
  them exactly. That made them part of the observable behaviour, so
  `opentime`'s own error type now renders upstream's wording — including the
  two spaces in `"Frame rate mismatch.  Timecode ..."`.

## What makes the rest harder

Binding `opentime` was mechanical, because a `RationalTime` is a value with no
identity. The object model is not. Three of the four problems it raised are
now settled, and the reasoning is in [`src/arena.rs`](src/arena.rs) where the
code that acts on it lives.

**A free-floating object has no document.** In upstream, `Clip("a")` exists on
its own and is appended to a track later. *Settled:* each object built from
Python gets a `Document` of its own, holding just it and whatever hangs off
it. The alternative, one document per interpreter, was rejected because it
never gets smaller — every object anyone builds would stay alive until the
process exits — and because it lets two unrelated timelines share a pool.

Appending therefore moves the object, which `Document::absorb` does, and the
document it came from is left as a forwarding note: where its contents went,
and the map from its old handles to the new ones. Every wrapper already handed
out keeps working, because each resolves its handle through that chain before
touching it. Updating each wrapper's handle in place instead would work for
the object being appended and quietly fail for everything under it.

**Object identity has to be maintained by hand.** Upstream's `track[0] is
track[0]` is `True`. *Settled:* each document caches a weak reference to the
Python wrapper it handed out for each node, and hands the same one back. The
reference is weak on purpose: a strong one makes a cycle running from the
wrapper through the cache back to the wrapper, and Python's collector cannot
see through Rust to break it.

**Mutation needs a short borrow.** *Settled:* every method takes the borrow,
does its work and drops it before returning to Python, and `metadata` is a
proxy object that reads and writes through to the document rather than a copy
of it — which is what makes upstream's `obj.metadata["k"] = v` change the
object. Holding a borrow across a call back into Python would deadlock the
moment that code touched the same document.

**Schemas registered from Python have no home.** *Still open.* Upstream lets a
user define a schema in Python (`schemadef`) and have it participate as a
first-class object. `Node` here is a closed enum, so such an object can only
arrive as `UnknownSchema`: it round-trips through a file intact but answers
none of the questions a real node answers. Supporting it properly means a
variant that holds a Python object, which is a design decision with a cost,
not an oversight to fix later.

Two smaller things worth knowing before the rest is written:

- **`otio-core`'s error messages are not upstream's yet.** The same problem
  this crate fixed in `opentime` applies to every `Error` variant in
  `otio-core`, and every one of them now reaches Python as the text of a
  `ValueError`. It is cheaper to fix before upstream tests start comparing
  them.
- **Upstream's exception types are missing.** `opentimelineio.exceptions` has
  a dozen of them and its tests catch them by name; everything here raises
  `ValueError`, which is what upstream's binding layer falls back to but not
  what it raises first.

## License

Apache-2.0, matching upstream OpenTimelineIO.

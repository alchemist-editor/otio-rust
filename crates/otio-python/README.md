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

## What is bound so far

`opentimelineio.opentime`, in full: `RationalTime`, `TimeRange` and
`TimeTransform`, with the module-level helpers (`from_frames`, `to_timecode`
and the rest) carried over from upstream's `opentime.py` as-is.

Nothing of `opentimelineio.core` or `opentimelineio.schema` yet. Those are the
object model, and they need a design decision that `opentime` did not; see
below.

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
identity. The object model is not, and these are the problems it raises. They
are written down here because they should be settled before the `_otio`
bindings are written, not during.

**A free-floating object has no document.** In upstream, `Clip("a")` exists on
its own and is appended to a track later. Here a node lives in a `Document`
and is named by a `NodeId`, so there is nowhere to put a clip that has no
timeline yet. Either each such object carries its own scratch document and
`append_child` moves the node between documents, or the Python layer keeps one
document per interpreter. The first is more honest and more work.

**Object identity has to be maintained by hand.** Upstream's `track[0] is
track[0]` is `True`, because a C++ object has exactly one Python wrapper, kept
alive by a keepalive monitor. Two wrappers built from the same `NodeId` would
be different Python objects, so `is` would be `False` and any code using a
node as a dictionary key would break. This needs a per-document cache from
`NodeId` to a weak reference to its wrapper.

**Mutation needs a short borrow.** Changing a node needs `&mut Document`, and
the wrapper cannot hold that across a call back into Python. Every method has
to take the borrow, do its work and drop it; anything that hands out a
reference into the arena — a metadata dictionary that writes through, which is
exactly what upstream's `AnyDictionary` does — has to be written as a proxy
rather than a view.

**Schemas registered from Python have no home.** Upstream lets a user define a
schema in Python (`schemadef`) and have it participate as a first-class
object. `Node` here is a closed enum, so such an object can only arrive as
`UnknownSchema`: it would round-trip through a file intact but would not
answer any of the questions a real node answers. Supporting it properly means
a variant that holds a Python object, which is a design decision with a cost,
not an oversight to fix later.

**`otio-core`'s error messages are not upstream's yet.** The same problem this
crate just fixed in `opentime` applies to every `Error` variant in
`otio-core`, and it is cheaper to fix before anything depends on the wording.

## License

Apache-2.0, matching upstream OpenTimelineIO.

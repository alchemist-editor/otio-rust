# Upstream tests

These files are copied from OpenTimelineIO 0.19.0's `tests/` directory
unmodified, and they are the point of this crate: if the bindings are a real
drop-in replacement, upstream's own test suite runs against them with nothing
changed.

Do not edit them. A test here that fails is a statement about the port, not
about the test, and the fix belongs in the Rust crates or in the bindings. If
a test genuinely cannot pass — because it exercises something this port has
deliberately not reproduced — say so in the crate README and skip it from the
runner, rather than editing the file and losing the record of what upstream
expects.

| File | From | Status |
| --- | --- | --- |
| `test_opentime.py` | `tests/test_opentime.py` | 83 of 83 passing |

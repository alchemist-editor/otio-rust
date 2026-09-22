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
| `test_composable.py` | `tests/test_composable.py` | 4 of 4 passing |
| `test_effect.py` | `tests/test_effect.py` | 8 of 8 passing |
| `test_media_reference.py` | `tests/test_media_reference.py` | 5 of 5 passing |
| `test_generator_reference.py` | `tests/test_generator_reference.py` | 4 of 4 passing |
| `test_image_sequence_reference.py` | `tests/test_image_sequence_reference.py` | 24 of 24 passing |
| `test_clip.py` | `tests/test_clip.py` | 8 of 8 passing |
| `test_item.py` | `tests/test_item.py` | 18 of 18 passing |
| `test_track.py` | `tests/test_track.py` | 5 of 5 passing |
| `test_transition.py` | `tests/test_transition.py` | 5 of 5 passing |
| `test_timeline.py` | `tests/test_timeline.py` | 16 of 16 passing |
| `test_serializable_collection.py` | `tests/test_serializable_collection.py` | 8 of 8 passing |

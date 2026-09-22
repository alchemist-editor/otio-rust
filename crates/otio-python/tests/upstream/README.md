# Upstream tests

These files are copied from OpenTimelineIO 0.19.0's `tests/` directory
unmodified, and they are the point of this crate: if the bindings are a real
drop-in replacement, upstream's own test suite runs against them with nothing
changed.

Do not edit them. A test here that fails is a statement about the port, not
about the test, and the fix belongs in the Rust crates or in the bindings. If
a test genuinely cannot pass — because it exercises something this port has
deliberately not reproduced — list it with its reason in
[`../excluded`](../excluded) so the runner deselects it, rather than editing the file and losing the record of what upstream
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
| `test_serializable_object.py` | `tests/test_serializable_object.py` | 15 of 16 passing; 1 skipped by upstream itself |
| `test_marker.py` | `tests/test_marker.py` | 9 of 9 passing |
| `test_unknown_schema.py` | `tests/test_unknown_schema.py` | 3 of 3 passing |
| `test_json_backend.py` | `tests/test_json_backend.py` | 16 of 16 passing |
| `test_core.py` | `tests/test_core.py` | 2 of 3 passing; 1 is Windows-only and skipped elsewhere |
| `test_cxx_sdk_bindings.py` | `tests/test_cxx_sdk_bindings.py` | 1 of 1 passing |
| `test_adapter_plugin.py` | `tests/test_adapter_plugin.py` | 13 of 13 passing |
| `test_hooks_plugins.py` | `tests/test_hooks_plugins.py` | 11 of 11 passing |
| `test_media_linker.py` | `tests/test_media_linker.py` | 7 of 7 passing |
| `test_plugin_detection.py` | `tests/test_plugin_detection.py` | 6 of 6 passing |
| `test_builtin_adapters.py` | `tests/test_builtin_adapters.py` | 6 of 6 passing |
| `test_otiod.py` | `tests/test_otiod.py` | 1 of 1 passing |
| `test_otioz.py` | `tests/test_otioz.py` | 1 of 1 passing |
| `test_schemadef_plugin.py` | `tests/test_schemadef_plugin.py` | 3 of 3 passing |
| `test_version_manifest.py` | `tests/test_version_manifest.py` | 6 of 6 passing |
| `test_console.py` | `tests/test_console.py` | 52 of 72 passing; 20 wait on `opentimelineio.algorithms` |
| `test_serialized_schema.py` | `tests/test_serialized_schema.py` | 2 of 3 passing; 1 compares docstrings |
| `test_url_conversions.py` | `tests/test_url_conversions.py` | 3 of 3 passing |

# Upstream tests left out

One file per vendored upstream test module, named after it
(`test_marker.txt` for `upstream/test_marker.py`), listing the tests the
runner deselects. Each line is a pytest node id relative to the module,
then `#` and the reason it cannot pass:

```
MarkerTest::test_downgrade_to_2  # writes an older schema version
```

A file per module keeps two changes to two modules from touching the same
lines. The upstream files themselves are never edited; see
[`../upstream/README.md`](../upstream/README.md).

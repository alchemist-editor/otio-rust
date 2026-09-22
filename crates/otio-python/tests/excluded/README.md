# Upstream tests left out

One file per vendored upstream test module, named after it
(`test_clip.txt` for `upstream/test_clip.py`), listing the tests the
runner deselects. Each line is a pytest node id relative to the module,
then `#` and the reason it cannot pass:

```
ClipTests::test_example  # why it cannot pass
```

A file per module keeps two changes to two modules from touching the same
lines. The upstream files themselves are never edited; see
[`../upstream/README.md`](../upstream/README.md).

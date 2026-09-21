# Manifest generators

The scripts that produce everything in the directory above, and
`src/builtin/tables.rs`, by reading upstream [`pyaaf2`][pyaaf2].

```
python3 gen_values.py ~/src/pyaaf2
```

Each one takes a pyaaf2 checkout as its only argument and writes its output
into the repository. Re-run them only when the pinned pyaaf2 revision changes
or a new fixture is added; running one should otherwise leave every file
byte-for-byte as it was.

| Script | Produces |
|---|---|
| `gen_manifest.py` | `*.manifest.tsv` — every directory entry and stream in the container |
| `gen_objects.py` | `*.objects.tsv` — every AAF object and the properties it holds |
| `gen_metadict.py` | `*.metadict.tsv` — the definitions each file stores about itself |
| `gen_merged.py` | `*.merged.tsv` — the dictionary a file is read with, built-ins included |
| `gen_values.py` | `*.values.tsv` — every data property decoded against its type |
| `gen_builtin.py` | `../../../src/builtin/tables.rs` — the definitions AAF takes as given |

## This is not a dependency

The `aaf` crate has no dependencies, runs no Python, and shells out to nothing.
These scripts are not part of the build or the test run: they are how a person
regenerates checked-in data when upstream moves, in the same way one might
paste in a fixture by hand. Nothing in `cargo build` or `cargo test` looks at
them, and the crate builds and its tests pass on a machine with no Python on it.

They live here rather than in someone's shell history because the data they
produce is only trustworthy if it can be reproduced. A manifest generated from
this crate's own reader would agree with any bug that reader has, which is the
one thing these tests exist to catch — so the way each one was made needs to be
readable, and re-runnable, by anyone reviewing them.

`gen_builtin.py` is the odd one out: its output is crate source rather than test
data. It is generated for the same reason. AAF's built-in definitions run to
116 classes and 164 types, and transcribing several thousand identifiers by hand
would introduce exactly the sort of error nothing downstream could catch.

[pyaaf2]: https://github.com/markreidvfx/pyaaf2

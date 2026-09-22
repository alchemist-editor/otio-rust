# Manifest generators

The scripts that produce everything in the directory above, and
`src/builtin/tables.rs` and `src/builtin/write_tables.rs`, from upstream
[`pyaaf2`][pyaaf2]. Most of them read files with pyaaf2. `gen_written.py`
writes files with it.

```
python3 gen_values.py ~/src/pyaaf2
```

Each one takes a pyaaf2 checkout as its only argument and writes its output
into the repository. Re-run them only when the pinned pyaaf2 revision changes
or a new fixture is added; running one should otherwise leave every file
byte-for-byte as it was. `gen_builtin.py` and `gen_write_tables.py` write crate source, so they run
`rustfmt` over what it emits and needs that on the path: the generated table is
formatted like anything else in the crate, and `cargo fmt --check` passes
straight after a regeneration.

| Script | Produces |
|---|---|
| `gen_manifest.py` | `*.manifest.tsv` — every directory entry and stream in the container |
| `gen_objects.py` | `*.objects.tsv` — every AAF object and the properties it holds |
| `gen_metadict.py` | `*.metadict.tsv` — the definitions each file stores about itself |
| `gen_merged.py` | `*.merged.tsv` — the dictionary a file is read with, built-ins included |
| `gen_values.py` | `*.values.tsv` — every data property decoded against its type |
| `gen_content.py` | `*.content.tsv` — the content tree, read by property name |
| `gen_builtin.py` | `../../../src/builtin/tables.rs` — the definitions AAF takes as given |
| `gen_write_tables.py` | `../../../src/builtin/write_tables.rs` — the extension classes and types, and the default data, container and codec definitions, pyaaf2 registers in a new file, in its order |
| `gen_written.py` | `written_*.aaf` and `written_*.calls.tsv` — files pyaaf2 *wrote*, with the times and UUIDs it used |

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

`gen_builtin.py` and `gen_write_tables.py` are the odd ones out: their output is
crate source rather than test data. It is generated for the same reason. AAF's built-in definitions run to
116 classes and 164 types, and transcribing several thousand identifiers by hand
would introduce exactly the sort of error nothing downstream could catch.

`gen_written.py` pins `PYTHONHASHSEED`, seeds `random`, and swaps pyaaf2's
`uuid.uuid4` and `datetime.now` for deterministic sequences before it writes
anything. It records the values it handed out beside each file, and that
record is what lets the Rust writer reproduce the file exactly.

[pyaaf2]: https://github.com/markreidvfx/pyaaf2

# aaf

A Rust implementation of AAF, the Advanced Authoring Format.

AAF is what professional editing applications use to move sequences, and often
the media itself, between each other. It is the format behind an Avid bin
export, and the one OpenTimelineIO's AAF adapter reads.

This is a port of [`pyaaf2`](https://github.com/markreidvfx/pyaaf2), the
library that adapter is built on. It is a full reimplementation: there is no
Python involved at any stage, and nothing here shells out or binds to it.

## Status

Early. What is here:

| Module | What it is | State |
|---|---|---|
| `cfb` | The Microsoft Compound File Binary container an AAF file is stored in | Reading, checked against pyaaf2 |
| `Auid` | AAF's 16-byte identifier | Done |

Still to come: the AAF object and type model, the write path, and above all of
that the adapter that maps AAF to OpenTimelineIO objects.

## Reading a file

```rust
use std::fs::File;
use aaf::cfb::CompoundFile;

let mut file = CompoundFile::open(File::open("example.aaf").unwrap()).unwrap();

for (path, id) in file.walk().unwrap() {
    let entry = file.entry(id).unwrap();
    if entry.is_stream() {
        println!("{path} ({} bytes)", entry.len());
    }
}
```

## Compatibility

Real AAF files, written by many applications over thirty years, disagree with
their own headers in small ways. Where a count in the header conflicts with
what the sector chains hold, this reader trusts the chains and records a
`cfb::Warning` rather than refusing the file — the same files open in editing
applications today, and pyaaf2 reads them too. Anything that would make a read
ambiguous or unbounded is still an error.

The test suite reads both fixture AAF files end to end — every directory entry
and every stream, 2152 entries in all — and compares paths, entry kinds, class
AUIDs, stream lengths and stream content hashes against manifests pyaaf2
produced from the same two files. See [`tests/data`](tests/data).

## License

Apache-2.0, matching the rest of this workspace. The AAF files under
[`tests/data`](tests/data) come from pyaaf2 and are MIT licensed; see the
README there.

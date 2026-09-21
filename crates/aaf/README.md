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
| `property` | The `properties` stream and the collection indexes | Reading, checked against pyaaf2 |
| `AafFile` | The file as a tree of objects, with references followed | Reading, checked against pyaaf2 |
| `MetaDictionary` | The class, property and type definitions a file carries | Reading, checked against pyaaf2 |
| `Value` | Property bytes decoded against the type they declare | Reading, checked against pyaaf2 |
| `MetaDictionary::builtin` | The definitions AAF takes as given and no file stores | Done, checked against pyaaf2 |
| `Aaf` | The file read by name: mobs, slots, segments, components | Reading, checked against pyaaf2 |
| `Auid`, `MobId` | AAF's 16- and 32-byte identifiers | Done |

Still to come: the write path, along with the extension definitions that go
with it, and above all of that the adapter that maps AAF to OpenTimelineIO
objects.

## Reading a file

```rust
use std::fs::File;
use aaf::Aaf;

let mut aaf = Aaf::open(File::open("example.aaf").unwrap()).unwrap();

for mob in aaf.top_level_mobs().unwrap() {
    println!("{:?}", aaf.name(&mob).unwrap());
    for slot in aaf.slots(&mob).unwrap() {
        let segment = aaf.child(&slot, "Segment").unwrap().unwrap();
        println!("    {:?} holds a {}",
            aaf.name(&slot).unwrap(),
            aaf.class_name(&segment).unwrap());
    }
}
```

Underneath that, `AafFile` reads the same file by identifier — every object,
every property, references followed — and `aaf::cfb` reads the container it is
all stored in, storages and streams directly.

## Compatibility

Real AAF files, written by many applications over thirty years, disagree with
their own headers in small ways. Where a count in the header conflicts with
what the sector chains hold, this reader trusts the chains and records a
`cfb::Warning` rather than refusing the file — the same files open in editing
applications today, and pyaaf2 reads them too. Anything that would make a read
ambiguous or unbounded is still an error.

The test suite reads both fixture AAF files end to end, twice over, and
compares the result against manifests pyaaf2 produced from those same files.
Once as a container: every directory entry and every stream, 2152 entries in
all, checked on path, kind, class AUID, stream length and content hash. Once as
objects: all 995 of them, reached by following the same references pyaaf2
follows, checked on path, class and the full list of properties each one holds.
Then every property those objects hold is decoded against the type declared for
it and compared with what pyaaf2 decoded — all 3,620 of them, down to the sign
of an `int16` and the members of a `TimeStamp`. The dictionary that decoding
runs on is checked too, against pyaaf2's own: 116 classes and 164 types, every
property and every type detail. Then the content tree is walked by name, the
way an application reads a file — mobs, slots, segments, components — and
compared again. See [`tests/data`](tests/data).

## License

Apache-2.0, matching the rest of this workspace. The AAF files under
[`tests/data`](tests/data) come from pyaaf2 and are MIT licensed; see the
README there.

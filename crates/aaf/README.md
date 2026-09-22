# aaf

A Rust implementation of AAF, the Advanced Authoring Format.

AAF is what professional editing applications use to move sequences, and often
the media itself, between each other. It is the format behind an Avid bin
export, and the one OpenTimelineIO's AAF adapter reads.

This is a port of [`pyaaf2`](https://github.com/markreidvfx/pyaaf2), the
library that adapter is built on. It is a full reimplementation: there is no
Python involved at any stage, and nothing here shells out or binds to it.

## Status

What is here:

| Module | What it is | State |
|---|---|---|
| `cfb` | The Microsoft Compound File Binary container an AAF file is stored in | Reading, checked against pyaaf2 |
| `cfb::CompoundFileWriter` | The same container, written with pyaaf2's layout | Writing, byte-identical to pyaaf2 |
| `property` | The `properties` stream and the collection indexes | Reading, checked against pyaaf2 |
| `AafFile` | The file as a tree of objects, with references followed | Reading, checked against pyaaf2 |
| `MetaDictionary` | The class, property and type definitions a file carries | Reading, checked against pyaaf2 |
| `Value` | Property bytes decoded against the type they declare | Reading, checked against pyaaf2 |
| `MetaDictionary::builtin` | The definitions AAF takes as given and no file stores | Done, checked against pyaaf2 |
| `Aaf` | The file read by name: mobs, slots, segments, components | Reading, checked against pyaaf2 |
| `write::AafWriter` | A new file, as `aaf2.open(path, 'w')` builds one, with pyaaf2's helpers | Writing, byte-identical to pyaaf2 |
| `AafWriter::open` | An existing file, changed as `aaf2.open(path, 'r+')` changes it | Changing, byte-identical to pyaaf2 |
| `write` extensions | The Avid extension definitions pyaaf2 registers in every new file | Done, byte-identical to pyaaf2 |
| `Auid`, `MobId` | AAF's 16- and 32-byte identifiers | Done |

`AafWriter` also embeds essence as pyaaf2 does: `import_dnxhd_essence` and
`import_audio_essence` read a raw DNxHD stream or a PCM WAV file frame by
frame into a new `EssenceData`, and `copy_from` copies an object, and all it
holds and refers to, out of a file `Aaf` has open, as pyaaf2's
`copy(root=f)` does. Both are byte-identical to pyaaf2.

The adapter that maps AAF to and from
OpenTimelineIO objects is the `otio-aaf` crate, which reads through `Aaf` and
writes through `AafWriter`.

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

## Writing a file

```rust
use aaf::write::AafWriter;

let mut w = AafWriter::new().unwrap();

let comp = w.create_mob("CompositionMob", Some("Edit")).unwrap();
w.set(comp, "UsageCode", "Usage_TopLevel").unwrap();
w.add_mob(comp).unwrap();

let slot = w.create_timeline_slot(comp, 24, None).unwrap();
let sequence = w.create_sequence("picture").unwrap();
w.set(slot, "Segment", sequence).unwrap();
let filler = w.create_filler("picture", 48).unwrap();
w.append(sequence, "Components", filler).unwrap();
w.set(sequence, "Length", 48).unwrap();

w.save("edit.aaf").unwrap();
```

This is pyaaf2's write path, ported: `f.create.Filler('picture', 48)` is
`w.create_filler("picture", 48)`, `obj['Name'].value = x` is
`w.set(obj, "Name", x)`, `obj['Slots'].append(s)` is
`w.append(obj, "Slots", s)`. Objects are named by `ObjRef` handles into the
writer rather than held as Python objects, and values convert from the Rust
types they correspond to and are encoded against the type the property
declares, as pyaaf2 encodes them. That includes what pyaaf2 accepts because
Python does: a float (`WriteValue::Float`) stored as a rational the way
`AAFRational(float)` makes one, a record given where an array is declared
taken as the list of its member names, as iterating a `dict` gives its keys,
and an empty list given for a collection of objects.

The promise is exact. The same operations in the same order produce the same
file pyaaf2 produces, down to the byte: the same directory layout and sector
allocation, the same red-black trees, the same property and index streams, and
the same extension definitions registered in the same order. The only inputs
that are not the operations themselves are the times pyaaf2 reads from the
clock and the UUIDs it draws at random. Here both come from a `Clock` and an
`IdSource` in `WriteOptions`. The defaults read the system clock and generate
random UUIDs, as pyaaf2 does. `SteppingClock` and `SequentialIds` make every
build of a file identical.

The tests replay the exact times and UUIDs pyaaf2 used for three fixture files
(an empty file, a composition, and the source chain the OpenTimelineIO adapter
writes). They then require the output to be identical to what pyaaf2 wrote.
All three are identical. See [`tests/write.rs`](tests/write.rs). The replaying
and the comparing live in [`tests/written/mod.rs`](tests/written/mod.rs), which
the `otio-aaf` crate's tests share: they hold the whole OpenTimelineIO writer,
built on this one, to the files upstream's adapter writes in the same way.

## Changing a file

```rust,no_run
use aaf::write::AafWriter;

let mut w = AafWriter::open("edit.aaf").unwrap();
let mobs = w.mobs().unwrap();
for mob in &mobs {
    if w.get_string(*mob, "Name").unwrap().as_deref() == Some("Old name") {
        w.set(*mob, "Name", "New name").unwrap();
    }
}
let comp = w.create_mob("CompositionMob", Some("Added")).unwrap();
w.add_mob(comp).unwrap();
w.remove_mob(mobs[0]).unwrap();
w.save("edit.aaf").unwrap();
```

This is pyaaf2's `aaf2.open(path, 'r+')`, which pyaaf2 also spells `'rw'`,
with the same API as a new file. The promise is the same too: the same edits
in the same order leave the same bytes pyaaf2 leaves. pyaaf2 writes back only
what changed, so that covers which objects it rewrites and when it marks
them, which freed sectors and directory entries it reuses and in what order,
the standard and Avid extension definitions it adds to the file's
dictionary on opening it, and the `/tmp` storage it parks the streams of
objects taken out of the file under. It leaves the header's `LastModified`
alone, and so does this. The design is in
[ADR 0005](../../docs/adr/0005-aaf-modify-path.md).

The tests hold that to twenty scenarios pyaaf2 ran on copies of the fixture
files: changing, adding and deleting properties, adding and removing mobs,
slots and components, new definitions and classes, streams growing out of the
mini stream and shrinking back into it, essence taken out, put back and
dropped, and a 512-byte-sector file. See [`tests/modify.rs`](tests/modify.rs).

One difference from pyaaf2 does not show in the bytes: pyaaf2 reads objects as
they are asked for, and this reads them all when the file is opened. A file
with an object that cannot be read fails to open here, where pyaaf2 would fail
only on reaching it.

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

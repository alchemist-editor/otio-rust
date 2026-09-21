# ADR 0002: Writing our own compound file reader rather than using an existing crate

- **Status:** Proposed
- **Date:** 2026-09-21
- **Deciders:** Jeff Hodges

## Context

An AAF file is not a format of its own at the byte level. It is a Microsoft
Compound File Binary (MS-CFB) file — a small filesystem inside a single file,
the same container Word 97 documents and MSI packages use — with AAF's object
model stored in it as named storages and streams.

So the AAF port begins with a compound file reader, and that is a format other
people have already implemented in Rust. The well-maintained [`cfb`][cfb] crate
reads and writes MS-CFB with a `std::io`-like API, is MIT licensed, and would
save something like 1500 lines of code here.

Against that, the AAF files this project has to open are thirty years of output
from a dozen editing applications, and they are not all well-formed. Upstream
`pyaaf2`, which is the library this port has to match, carries specific
tolerances for the ways they go wrong:

- the header's FAT sector count disagreeing with the DIFAT,
- the header's directory sector count disagreeing with the directory chain,
- the root entry's mini stream length disagreeing with the mini FAT,
- a version 4 file allocating the sector reserved for a byte-range lock,
- a file truncated partway through its last sector.

In each case pyaaf2 logs a warning, believes the structure over the header, and
carries on. Those files open in editing applications today. A reader that is
strict about the spec rejects work that people are actually trying to move.

The write path pulls the same way. AAF writers are conservative about the exact
layout they emit — the directory entries' red-black tree, where streams cross
the mini stream threshold, how sectors are allocated — because a file that a
strict reader rejects is a file that does not import. Matching pyaaf2's layout
byte for byte is a requirement, not a preference, and it is much easier to do
from a reader and writer we control.

## Decision

Write the compound file layer here, as the `cfb` module of the `aaf` crate,
porting `pyaaf2`'s behaviour including its tolerances.

It is a module rather than a separate crate because this is not a
general-purpose MS-CFB implementation and should not be mistaken for one: its
lenience is tuned to AAF. Nothing in it refers to AAF's object model, so
splitting it out later is a re-export away if that ever becomes useful.

Where this reader works around a malformed file, it records a `cfb::Warning`
rather than staying silent. A caller that wants strictness — a validator, or a
test — can see exactly what was tolerated. Anything that would make a read
ambiguous or unbounded (a cyclic sector chain, a sector outside the allocation
table, a directory tree that is not a tree) is still an error.

## Consequences

- About 1500 lines of format code to own and keep correct, against a dependency
  someone else maintains.
- Tolerance is ours to tune as real files turn up, rather than something to work
  around at arm's length or upstream into another project.
- The write path can match pyaaf2's layout exactly, which is what makes the
  files we produce importable.
- Correctness has to be demonstrated, not assumed. Every test reads real AAF
  files from pyaaf2's own suite and compares against manifests pyaaf2 generated
  from those same files: paths, entry kinds, class AUIDs, stream lengths and
  stream content hashes, for all 2152 entries across the two fixtures. A
  manifest is never regenerated from this crate's own reader, which would make
  it agree with any bug it has.

## Notes

This decision is about the container only. The layer above it — AAF's object
model, type definitions and class dictionary — has no Rust implementation to
reuse, so there is nothing to weigh there.

The choice not to depend on Python for AAF in any form was made separately and
is not revisited here.

[cfb]: https://crates.io/crates/cfb

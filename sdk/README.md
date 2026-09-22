# SDKs

Language SDKs for OpenTimelineIO, generated from the C ABI in
[`crates/otio-capi`](../crates/otio-capi).

| SDK | Directory | State |
|---|---|---|
| Go | [`go`](go) | Complete over the C ABI, tested and checked in CI |
| Swift | [`swift`](swift) | Complete over the C ABI, tested and checked in CI |

## How these are made

Nobody writes these by hand, and nobody writes an interface description by
hand either. The C ABI's own Rust source is the single source of truth:
[`otio-sdk-model`](../crates/otio-sdk-model) reads it and produces a
description of the interface — every function, its parameters, what it
returns, its documentation, and what role it plays — and
[`otio-sdk-gen`](../crates/otio-sdk-gen) turns that description into an SDK
per language.

The description is committed as [`api.json`](api.json) so that a change to the
C ABI shows up as a readable diff rather than as a surprise in generated code.

To regenerate everything:

```sh
cargo run -p otio-sdk-gen
```

To check that what is committed matches what the C ABI says it should be,
which is what CI does:

```sh
cargo test -p otio-sdk-gen
```

Seven kinds of drift fail that check rather than reaching a user: a C function
that fits no naming convention, a C function whose body does something the
description has no way to express, a schema in the data model that the
generator does not know about, two C functions that would land on the same
name in a target language, a call that consumes a document, a struct whose
computed layout disagrees with the size the C ABI asserts for it, and
generated files that differ from the ones checked in.
[ADR 0003](../docs/adr/0003-sdk-generation.md) explains each, and why the Rust
source rather than the header or a hand-written schema.

## What "idiomatic" means here

A generated API that reads like a transliteration of C is a failure even if
every function is present. Each backend owns the shape of its own language:
Go gets methods, embedded structs for the schema ladder, slices, strings,
`error` and a sentinel for "there is no value", not two-pass out-parameters
and status codes.

The shape of the API itself — what things are called, which members exist,
which are properties and which are methods — follows upstream
OpenTimelineIO's own bindings for that language rather than being invented
here. Where a generated SDK departs from upstream's shape, the departure is
recorded in the ADR with its reason.

## Adding a language

A backend is one Rust module in `otio-sdk-gen` that takes the `Api` and
returns a list of files. `go.rs` is the worked example. Adding one means
adding the module, a line to the `TARGETS` table, a CI job that builds the C
library and runs the language's own tests, and a note in the ADR saying what
the language copies from upstream and what it deliberately does differently.

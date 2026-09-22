---
title: Getting started
summary: Building the core, and reaching it from your language.
section: Start here
order: 2
---

Everything starts with the Rust core. The bindings other than Python link a
static library built from it, so that is the first build in every case.

```sh
git clone https://github.com/alchemist-editor/otio-rust
cd otio-rust
cargo build --workspace
cargo test --workspace
```

> [!NOTE]
> Build the workspace before running the tests that include the C ABI's own C
> program. It links against `libotio`, so a test run that is also the first
> build can reach for a library that is not there yet.

## Rust

Add the crates you need. `otio-core` is the data model; each adapter is its
own crate so that a program reading EDLs does not compile an AAF parser.

```toml
[dependencies]
otio-core = { git = "https://github.com/alchemist-editor/otio-rust" }
otio-cmx3600 = { git = "https://github.com/alchemist-editor/otio-rust" }
```

## Python

The package is called `opentimelineio`, and the name is the point: it aims to
be a drop-in replacement for upstream's own package, measured by running
upstream's test files unmodified.

```sh
cd crates/otio-python
pip install .
```

## The SDKs

Go, Swift, Zig and C++ each link `libotio`, which is not checked in. Build it
and put it where the package expects it:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/go/lib/     # or sdk/swift, sdk/zig, sdk/cpp
```

Then each is ordinary for its language — `go test ./...`, `swift test`,
`zig build test`, `cmake --build`. The README beside each SDK has the exact
invocation, including the linker flag Swift needs.

## TypeScript

The TypeScript package is different: it has no static library at all. The C
ABI is compiled to WebAssembly and the package ships the module, so it runs
in a browser and in Node with nothing native to install.

```sh
cd crates/otio-wasm/ts
npm install
npm run build
```

There is no filesystem behind a WebAssembly module, so the adapters there
take and return bytes and leave getting hold of them to the host — a `fetch`
in a browser, `fs` in Node. That is a line of code, and it is the host's line
rather than the library's.

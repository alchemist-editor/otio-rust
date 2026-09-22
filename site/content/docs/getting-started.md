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

Files are read and written the way upstream reads and writes them, through
`opentimelineio.adapters`: `read_from_file("cut.edl", rate=24)` picks the EDL
adapter from the suffix, and each adapter takes upstream's keyword arguments.
Every format on the [reading and writing](/docs/guides/reading-and-writing)
page is there, AAF included.

The rest of upstream's package is there as well. Its plugin system loads
adapters, media linkers, hooks, schemadefs and version manifests from
`OTIO_PLUGIN_MANIFEST_PATH` and from installed packages' entry points, the
same way upstream's does, and the built-in formats are declared as plugins
themselves. Installing the package also installs upstream's console tools:
`otiocat`, `otioconvert`, `otiostat`, `otiotool` and `otiopluginfo`.

## The SDKs

Go, Swift, Zig, C++, C# and Objective-C each link `libotio`, which is not
checked in. Build it and put it where the package expects it:

```sh
cargo build -p otio-capi --release
cp target/release/libotio.a sdk/go/lib/     # or sdk/swift, sdk/zig, sdk/cpp, sdk/objc
```

C# is the one that loads the library at run time rather than linking it, so
it wants the shared one instead — `libotio.so`, or `libotio.dylib` on macOS —
copied into `sdk/csharp/lib/`.

Then each is ordinary for its language — `go test ./...`, `swift test`,
`zig build test`, `cmake --build`, `dotnet run --project tests`, `make check`.
The README beside each SDK has the exact invocation, including the linker
flag Swift needs and the GNUstep packages Objective-C needs on Linux.

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

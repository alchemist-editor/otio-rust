# otio-wasm

OpenTimelineIO for the browser and for Node.

The crate is a `cdylib` that builds the C ABI for `wasm32-unknown-unknown`,
plus the two allocator entry points JavaScript needs to hand the module a
string. The TypeScript package in `ts/` ships with it, and is written by
`otio-sdk-gen`'s TypeScript backend rather than by hand.

## The package

Published to npm as
[`@alchemist-edit/otio`](https://www.npmjs.com/package/@alchemist-edit/otio).
The README in `ts/` is the one npm shows; this one is about how it is made.

```sh
npm install @alchemist-edit/otio
```

```ts
import { init, readTimelineFromString, Clip, RationalTime, TimeRange } from "@alchemist-edit/otio";

await init();

const timeline = readTimelineFromString("cmx3600", edl, { rate: 24 });
for (const clip of timeline.findClips()) {
  console.log(clip.name, clip.trimmedRange().duration.toTimecode());
}

const shot = new Clip({ name: "shot_01" });
shot.sourceRange = new TimeRange(
  RationalTime.fromTimecode("01:00:00:00", 24),
  RationalTime.fromFrames(48, 24),
);
```

The hierarchy and the names are upstream OpenTimelineIO's, because that is the
API the people who will use this already know. A `Clip` is built on its own and
put inside a `Track` afterwards, `sourceRange` is where Python says
`source_range`, and `clip.metadata.get("cmx_3600.reel")` reads the same tree
`clip.metadata["cmx_3600"]["reel"]` does there.

One package serves both environments. `package.json` points a browser at
`browser.js`, which fetches the module and compiles it as it streams, and Node
at `node.js`, which reads it off disk; everything above that is the same file.

## The adapters come too

EDL, ALE, FCP 7 XML, FCP X XML and AAF are Python-only plugins upstream, not
part of its C++ core, so a WebAssembly build of that core structurally cannot
read them. Ours are Rust and compile to `wasm32` unchanged, so a browser gets
the interchange formats an editorial tool actually receives rather than only
`.otio`. All five are here. AAF is the one that wanted something from the
host: a written AAF records when it was made and gives itself random
identifiers, and a module with no imports has neither a clock nor
randomness, so `writeToBytes` passes the time and a fresh seed on every
write unless the caller gives its own.

The two bundle formats, `.otioz` and `.otiod`, are the exception. A bundle is
a directory, or an archive of media copied in from files on disk, and a
module with no imports has no file system to find either on. The library
refuses one on `wasm32`, and rather than offer a format that can only fail,
the generator leaves the `otioz` and `otiod` formats, the bundle options and
the `BundleMediaPolicy` enum out of the TypeScript surface altogether (its
`NO_FILE_SYSTEM_*` tables). The option fields are still in the structs, which
are the library's layout, and are written as zero.

## Why there is no wasm-bindgen

The C ABI compiles to `wasm32-unknown-unknown` unchanged: 266 exports, no
imports, and the memory exported. So the build is `cargo build --target
wasm32-unknown-unknown` and nothing else — no bindgen step, no
post-processing, and no Rust dependency the workspace did not already have.

What that costs is the marshalling, which has to be written in TypeScript
rather than derived by a macro. It is written once, by the generator, from the
same source the C header comes from.

## What the generator reads, and what it insists on

`otio-sdk-model` parses `crates/otio-capi/src/*.rs` — the signatures, the doc
comments, and a few things the prose says that the signatures do not, such as
which arguments may be null and which calls answer "there is nothing there".
From that description, `otio-sdk-gen`'s TypeScript backend writes five files
under `ts/src/generated/` and one Rust file:

| File | What is in it |
| --- | --- |
| `exports.ts` | the module's exports, as a TypeScript interface |
| `types.ts` | the enums, the structs, and the code that reads and writes them |
| `raw.ts` | one function per C entry point, doing the marshalling and nothing else |
| `values.ts` | `RationalTime`, `TimeRange` and `TimeTransform`, as classes |
| `api.ts` | the object model, as classes |
| `src/layout.rs` | assertions that the layouts the marshalling assumes are the ones the compiler produced |

Two things keep it honest:

- **Every entry point is accounted for.** Each one is classified, or named in a
  short table with the reason it is not. An entry point no rule covers stops
  the generator rather than being quietly left out, so adding one to the C ABI
  and forgetting the SDK is not something that can happen.
- **The layout is checked by the compiler.** A 32-bit target lays a struct out
  differently from the machine the generator runs on, so the offsets are
  computed for `wasm32` and written back out as `const` assertions that the
  wasm build fails on if they are wrong.

`cargo run -p otio-sdk-gen -- --check ts` fails if what is committed is not
what the generator would write today, which is what CI runs.

## The arena, and what it means in JavaScript

The core keeps every object in a document's arena and names it with a handle:
an index and a generation, per ADR 0001. Upstream's API has no document in it.
Reconciling those two is `ts/src/objects.ts`, and it is the same answer the
Python bindings reached:

- each object built on its own gets a document of its own;
- putting one inside another moves it there, and the emptied document keeps a
  note saying where its contents went, so a wrapper handed out before the move
  goes on working;
- one that is still a child in another timeline is refused when it is
  appended or inserted, with the library's own `"coreError"` and message, and
  one whose handle has gone stale is refused wherever it is placed, with
  `"staleHandle"`, both before anything moves, so both timelines stay whole;
- reading the same object twice gives the same wrapper, so `===` means what a
  JavaScript programmer expects;
- a document is released by a `FinalizationRegistry`, which is late but
  correct, or by `dispose()` for code that would rather say when.

Node handles themselves are values with nothing to release. Only the document
owns memory, and there is exactly one reference to it in the whole package.

## Building and testing

```sh
cd crates/otio-wasm/ts
npm install
npm run build        # cargo build --target wasm32-unknown-unknown, then tsc
npm test             # the suite, in Node
npm run test:browser # the same suite, in Chromium
npm run check:pack   # packs the tarball, installs it somewhere empty, runs it
```

The cases live in `test/suite.ts` and are run by both, because a package that
ships to two environments has two of everything that could go wrong and one
API. On a machine that already has a Chromium, `OTIO_CHROMIUM` points the
browser run at it instead of downloading another.

## Releasing

Pushing a tag `npm-v<version>` publishes that version to npm, from
[`.github/workflows/release-npm.yml`](../../.github/workflows/release-npm.yml)
with trusted publishing and provenance. The steps, and what has to be set up
on npmjs.com first, are in [docs/releasing.md](../../docs/releasing.md).

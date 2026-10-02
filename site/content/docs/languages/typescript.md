---
title: TypeScript
summary: The core compiled to WebAssembly and published to npm, for the browser and for Node, with no document to hold.
section: Languages
order: 3
---

The TypeScript package, [`@alchemist-edit/otio`](https://www.npmjs.com/package/@alchemist-edit/otio),
is the C ABI compiled to `wasm32-unknown-unknown` with a TypeScript object
model on top. The module has no imports, so one package serves both the
browser and Node with nothing native to install. The TypeScript is written by
the same generator as the other SDKs, from the same description of the C ABI,
and the class hierarchy and names are upstream OpenTimelineIO's, spelled the
way TypeScript spells things: `sourceRange` where Python says `source_range`.

## Install

```sh
npm install @alchemist-edit/otio
```

The package declares Node 20 or newer. Call `init()` once before anything
else; calling it again does nothing. In Node it reads the module off disk. In
a browser it fetches it from beside the JavaScript with
`new URL("otio_wasm.wasm", import.meta.url)`, which Vite, webpack, Rollup and
esbuild all understand.

When the module is served from somewhere else, such as a CDN, pass its URL:
`init(url)`. When you already have the bytes or a compiled module, hand them
to `initAsync` or `initSync` instead. The module itself is exported as
`@alchemist-edit/otio/wasm` for bundlers that want to be told where it is.

To build it from this repository instead, which needs Rust and the
`wasm32-unknown-unknown` target:

```sh
cd crates/otio-wasm/ts
npm install
npm run build        # cargo build --target wasm32-unknown-unknown, then tsc
npm test             # the suite, in Node
npm run test:browser # the same suite, in Chromium
```

Versions are published from an `npm-v<version>` tag with npm's trusted
publishing, so each version on npm carries a provenance statement that
`npm audit signatures` checks. A pre-release such as `0.2.0-rc.1` goes to the
`next` dist-tag, so a plain `npm install` never picks one up.

## Your first program

<!-- ::sample id="read-an-edl" lang="typescript" -->

There is no file system behind a WebAssembly module, so the adapters take and
return strings or bytes, and getting hold of them is the host's line of code:
`fs` in Node, `fetch` in a browser. `readTimelineFromString` is
`readFromString` with a check that the file held a timeline, which most files
do and most callers want. The rate is the one option an EDL cannot do
without; see [CMX 3600 EDL](/docs/formats/edl).

## Building a timeline

<!-- ::sample id="build-a-timeline" lang="typescript" -->

A new `Timeline` arrives with an empty stack called `tracks`, as upstream's
does. Constructors take an object of named fields, and stored fields such as
`name` and `sourceRange` are properties.

## How objects and documents work here

The core keeps every object in a document's arena and names it with a handle.
Upstream's API has no document in it, and neither does this one:

- each object built on its own gets a document of its own;
- putting one inside another moves it there, and the emptied document keeps a
  note saying where its contents went, so a wrapper handed out before the move
  goes on working;
- reading the same object twice gives the same wrapper, so `===` means what a
  JavaScript programmer expects;
- a document is released by a `FinalizationRegistry`, which is late but
  correct, or by `dispose()` for code that would rather say when. Everything
  that lived in it throws afterwards rather than reading memory that has been
  handed back.

An object that is still a child in another timeline is refused when it is
appended or inserted, and one whose handle has gone stale is refused wherever
it is placed, both before anything moves, so both timelines stay whole.

## Errors and missing values

A call the library fails throws an `OtioError`, whose `status` says which kind
of failure it was: `"staleHandle"` for an object used after it was removed,
`"parseError"` for a file that was not what it claimed to be, `"coreError"`
for a refusal from the core, and the rest of the C ABI's statuses spelled the
same way.

A call that only names an object, such as `hasChild`, `detachChild` or
`flattenTracks`, refuses one from another timeline before the library is
asked, with an `OtherTimelineError`. It carries no status and is not an
`OtioError`, so `err instanceof OtherTimelineError` is how to recognise it.

"There is nothing here" is `undefined`: the `sourceRange` of an untrimmed
item, the `parent()` of an object that is in nothing.

One failure is particular to WebAssembly. `wasm32-unknown-unknown` has no
unwinding, so a panic in the Rust core traps instead of being caught at the
boundary. The module is then marked poisoned and every later call throws an
`OtioPanic`; load a fresh one with `init()`.

## Editing and algorithms

<!-- ::sample id="edit-operations" lang="typescript" -->

The ten edit operations are functions on the exported `edit` object, such as
`edit.insert`, `edit.overwrite` and `edit.fill`, and the algorithms are on
`algorithms`: `flattenStack`, `flattenTracks` and `trackTrimmedToRange`.
[Editing a timeline](/docs/guides/editing) says what each edit does.

## Formats

`readFromString`, `readFromBytes`, `writeToString` and `writeToBytes` take a
format by name: `"otioJson"`, `"ale"`, `"cmx3600"`, `"fcp7Xml"`, `"fcpxXml"`
or `"aaf"`. `serializeJsonToString` and `deserializeJsonFromString` are
upstream's names for `.otio` JSON, and start wherever they are pointed, so a
bare clip serialises as happily as a whole timeline.

EDL, ALE, both FCP XML flavours and AAF are Python-only plugins upstream, so a
WebAssembly build of upstream's C++ core cannot read them. Here they are Rust
and compile to `wasm32` unchanged. AAF is the one that wanted something from
the host: a written AAF records when it was made and gives itself random
identifiers, and the module has neither a clock nor randomness, so
`writeToBytes` passes the time and a fresh seed on every write.

<!-- ::sample id="write-an-aaf" lang="typescript" -->

The bundle formats, `.otioz` and `.otiod`, are not available here. A bundle is
a directory, or an archive of media copied in from files on disk, and the
module has no file system to find either on. Rather than offer a format that
can only fail, the package leaves them out of its surface: write the timeline
as `.otio` and bundle it where there is a file system. See
[Bundles](/docs/formats/bundles) and
[Reading and writing files](/docs/guides/reading-and-writing).

## Platforms

Browsers and Node. CI runs the same test suite in both, in Node and in
Chromium, and `npm run check:pack` installs the packed tarball into an empty
project and runs it there before a release.

## Reference

- [`crates/otio-wasm/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-wasm/README.md):
  how the module and the package are made, and why there is no wasm-bindgen.
- [`crates/otio-wasm/ts/README.md`](https://github.com/alchemist-editor/otio-rust/blob/main/crates/otio-wasm/ts/README.md):
  the README npm shows.
- The [C ABI reference](/reference) the package is generated from.

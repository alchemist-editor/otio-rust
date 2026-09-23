# @alchemist-edit/otio

[OpenTimelineIO](https://opentimeline.io) for the browser and for Node, on a
Rust core compiled to WebAssembly. It reads and writes `.otio`, CMX 3600 EDL,
ALE, Final Cut Pro 7 XML, Final Cut Pro X XML and AAF, with no native
dependency to install and nothing but the one `.wasm` module it ships.

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

The class hierarchy and the names are upstream OpenTimelineIO's, spelled the
way TypeScript spells things: `sourceRange` where Python says `source_range`.

## Where the `.wasm` comes from

`init()` finds the module for wherever it is running. In Node it reads it off
disk; in a browser it fetches it from beside the JavaScript with
`new URL("otio_wasm.wasm", import.meta.url)`, which Vite, webpack, Rollup and
esbuild all understand. Call it once; calling it again does nothing.

When the module is served from somewhere else, such as a CDN or a versioned
asset path, pass its URL: `init(url)`. When you already have the bytes or a
compiled module, from a service worker's cache or shared between workers, hand
them to `initAsync` or `initSync` instead. The module itself is exported as
`@alchemist-edit/otio/wasm` for bundlers that want to be told where it is.

## More

- Documentation, guides and the API reference:
  <https://otio-rust-docs.vercel.app>
- Source, issues and the other language SDKs:
  <https://github.com/alchemist-editor/otio-rust>

Apache-2.0.

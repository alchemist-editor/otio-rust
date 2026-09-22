import { readFile } from "node:fs/promises";
import { init, readTimelineFromString } from "@otio/otio";

await init();

// There is no filesystem behind a WebAssembly module, so getting hold of the
// bytes is the host's line of code: `fs` in Node, `fetch` in a browser.
const edl = await readFile("cut.edl", "utf8");

// An EDL never says what rate its timecode is at, so this has to be right: a
// file read at the wrong rate puts every event in the wrong place rather
// than failing.
const timeline = readTimelineFromString("cmx3600", edl, { rate: 24 });

for (const clip of timeline.findClips()) {
  console.log(clip.name);
}

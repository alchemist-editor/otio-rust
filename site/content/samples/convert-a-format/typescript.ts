import { readFile, writeFile } from "node:fs/promises";
import { init, readFromString, writeToString } from "@alchemist-edit/otio";

await init();

// There is no filesystem behind a WebAssembly module, so getting hold of the
// bytes is the host's line of code: `fs` in Node, `fetch` in a browser.
const edl = await readFile("cut.edl", "utf8");

// An EDL never says what rate its timecode is at, so this has to be right: a
// file read at the wrong rate puts every event in the wrong place rather
// than failing.
const timeline = readFromString("cmx3600", edl, { rate: 24 });

// Nothing happens in between. The timeline an EDL parses to is the same
// timeline FCP X writes out, so converting is a read and a write: the object
// model is the interchange, and the file formats are two ways of spelling it.
await writeFile("cut.fcpxml", writeToString("fcpxXml", timeline));

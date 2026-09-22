/**
 * The suite, run in Node.
 *
 * Imports the package the way an application in Node does — through the
 * `node` entry point, which reads the `.wasm` off disk — so what is tested is
 * the shape the package ships and not the source tree behind it.
 */

import { before, test } from "node:test";

import * as otio from "../src/node.js";
import { exports } from "../src/runtime.js";
import { cases } from "./suite.js";

before(async () => {
  await otio.init();
});

for (const each of cases) {
  test(each.name, () => {
    each.run(otio);
  });
}

/*
 * Not in `suite.ts`, because it looks past the package's surface at the
 * module's memory, which only a runner that loaded the same copy of the
 * runtime can reach.
 */
test("a failure's message is freed, and so is the one a missing value brings", () => {
  // Every call that can fail is handed a buffer for its message, and every
  // path out of the binding has to free it: `check` after a failure or a
  // success, `release` after "there is nothing". A path that forgot would
  // leak a few dozen bytes a call, which nothing else would notice. Tens of
  // thousands of calls turn that into megabytes, and the module's memory only
  // ever grows, so a leak shows up as memory that is bigger afterwards.
  const { Clip, OtioError, RationalTime, Track } = otio;
  const track = new Track({ name: "V1" });
  const clip = new Clip({ name: "shot_01" });
  const round = (count: number): void => {
    for (let index = 0; index < count; index += 1) {
      try {
        RationalTime.fromTimecode(`not a timecode ${index}`, 24);
        throw new Error("a bad timecode was read");
      } catch (thrown) {
        if (!(thrown instanceof OtioError) || thrown.message === "") {
          throw thrown;
        }
      }
      try {
        track.childAt(index);
        throw new Error("an empty track had a child");
      } catch (thrown) {
        if (!(thrown instanceof OtioError) || thrown.message === "") {
          throw thrown;
        }
      }
      if (clip.sourceRange !== undefined) {
        throw new Error("a new clip had a source range");
      }
      if (track.childCount() !== 0) {
        throw new Error("an empty track counted children");
      }
    }
  };

  // The first rounds settle the allocator and the scratch block, which grow
  // once on their own; only growth after that is the thing being looked for.
  round(2_000);
  const settled = exports().memory.buffer.byteLength;
  round(30_000);
  const after = exports().memory.buffer.byteLength;
  if (after !== settled) {
    throw new Error(
      `the module's memory grew from ${settled} to ${after} bytes ` +
        "over calls that should each have freed what they were handed",
    );
  }
});

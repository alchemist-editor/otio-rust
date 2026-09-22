/**
 * The suite, run in Node.
 *
 * Imports the package the way an application in Node does — through the
 * `node` entry point, which reads the `.wasm` off disk — so what is tested is
 * the shape the package ships and not the source tree behind it.
 */

import { before, test } from "node:test";

import * as otio from "../src/node.js";
import { cases } from "./suite.js";

before(async () => {
  await otio.init();
});

for (const each of cases) {
  test(each.name, () => {
    each.run(otio);
  });
}

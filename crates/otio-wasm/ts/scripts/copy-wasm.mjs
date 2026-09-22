/**
 * Puts the module beside the compiled JavaScript, which is where both entry
 * points look for it.
 */

import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
for (const into of ["dist", "dist-test/src"]) {
  const destination = join(here, "..", into, "otio_wasm.wasm");
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(join(here, "..", "vendor", "otio_wasm.wasm"), destination);
}

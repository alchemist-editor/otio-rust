/**
 * Runs before `npm pack` and `npm publish` put the package in a tarball.
 *
 * It builds nothing: building needs Rust and the wasm target, and a publish
 * should ship exactly what the tests just ran against rather than a second
 * build of it. What it does is refuse a tarball that would be broken, and
 * bring in the one file that lives outside this directory.
 */

import { copyFileSync, existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(dirname(fileURLToPath(import.meta.url)));
const workspace = join(here, "..", "..", "..");

// npm puts a LICENSE beside package.json in every tarball, but the licence is
// the repository's and lives at its root. Copied rather than committed twice,
// so there is one of it; the copy is ignored by git.
copyFileSync(join(workspace, "LICENSE"), join(here, "LICENSE"));

// Every file `exports` names has to be in `dist`, and a tarball without the
// module in it would install cleanly and fail on the first `init()`.
const manifest = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));
const missing = targets(manifest.exports).filter((path) => !existsSync(join(here, path)));
if (missing.length > 0) {
  console.error(
    `prepack: the package is not built; missing ${missing.join(", ")}.\n\n` +
      "    npm run build\n",
  );
  process.exit(1);
}

/** Every relative path an `exports` map can resolve to. */
function targets(exports) {
  if (typeof exports === "string") {
    return exports.startsWith("./") ? [exports] : [];
  }
  return Object.values(exports ?? {}).flatMap(targets);
}

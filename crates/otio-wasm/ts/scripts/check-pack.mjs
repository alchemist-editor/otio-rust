/**
 * Checks the package as somebody installing it would get it.
 *
 * The tests run against the working tree, which has files the tarball does
 * not: `src`, `vendor`, `node_modules`. A `files` list that forgot the module,
 * a map pointing at a source that was not shipped, or an `exports` entry that
 * names a file which is not there would all pass them and fail every user.
 * So this packs the tarball, installs it into an empty project, and runs it
 * there, with nothing of this directory in reach.
 */

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, posix } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(dirname(fileURLToPath(import.meta.url)));
const manifest = JSON.parse(readFileSync(join(here, "package.json"), "utf8"));
const scratch = mkdtempSync(join(tmpdir(), "otio-pack-"));
const npm = process.platform === "win32" ? "npm.cmd" : "npm";

let failures = 0;
function fail(message) {
  console.error(`check-pack: ${message}`);
  failures += 1;
}

try {
  const [packed] = JSON.parse(
    execFileSync(npm, ["pack", "--json", "--pack-destination", scratch], {
      cwd: here,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    }),
  );
  const shipped = new Set(packed.files.map((file) => file.path));
  console.log(
    `${packed.filename}: ${packed.files.length} files, ` +
      `${(packed.size / 1024).toFixed(0)} KiB packed, ` +
      `${(packed.unpackedSize / 1024).toFixed(0)} KiB unpacked`,
  );

  // What a reader of the npm page and a lawyer both look for.
  for (const path of ["README.md", "LICENSE", "package.json"]) {
    if (!shipped.has(path)) fail(`${path} is not in the tarball`);
  }

  // Everything `exports` can resolve to, under every condition.
  for (const path of targets(manifest.exports)) {
    if (!shipped.has(posix.normalize(path))) fail(`exports names ${path}, which is not in the tarball`);
  }

  // Every source map's sources, so a debugger stepping into the package lands
  // in the TypeScript rather than in a warning from the bundler.
  for (const map of [...shipped].filter((path) => path.endsWith(".js.map"))) {
    const { sources } = JSON.parse(readFileSync(join(here, map), "utf8"));
    for (const source of sources) {
      const path = posix.normalize(posix.join(posix.dirname(map), source));
      if (!shipped.has(path)) fail(`${map} points at ${path}, which is not in the tarball`);
    }
  }

  // Nothing that only the build or the tests need.
  for (const path of shipped) {
    if (/^(vendor|test|dist-test|scripts|node_modules)\//.test(path)) fail(`${path} should not be shipped`);
  }

  // Install it where nothing else is, and use it the way the README says to.
  const project = join(scratch, "project");
  mkdirSync(project);
  writeFileSync(join(project, "package.json"), JSON.stringify({ name: "check-pack", private: true, type: "module" }));
  execFileSync(npm, ["install", "--no-audit", "--no-fund", "--ignore-scripts", join(scratch, packed.filename)], {
    cwd: project,
    stdio: ["ignore", "ignore", "inherit"],
  });
  writeFileSync(
    join(project, "smoke.js"),
    `import { init, readTimelineFromString, RationalTime, Clip } from ${JSON.stringify(manifest.name)};

await init();

const end = RationalTime.fromTimecode("01:00:00:00", 24).add(RationalTime.fromFrames(48, 24));
if (end.toTimecode() !== "01:00:02:00") throw new Error("time math: " + end.toTimecode());

const edl = [
  "TITLE: Cut",
  "FCM: NON-DROP FRAME",
  "",
  "001  A001     V     C        01:00:00:00 01:00:01:00 00:00:00:00 00:00:01:00",
  "* FROM CLIP NAME:  A",
  "",
].join("\\n");
const names = readTimelineFromString("cmx3600", edl, { rate: 24 }).findClips().map((clip) => clip.name);
if (names.join() !== "A") throw new Error("EDL: " + names.join());

if (new Clip({ name: "shot" }).name !== "shot") throw new Error("Clip");
console.log("the installed package loads, reads an EDL and does time math");
`,
  );
  execFileSync(process.execPath, ["smoke.js"], { cwd: project, stdio: "inherit" });
} catch (error) {
  fail(error instanceof Error ? error.message : String(error));
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

if (failures > 0) {
  process.exit(1);
}
console.log("check-pack: the tarball is complete and works once installed");

/** Every relative path an `exports` map can resolve to. */
function targets(exports) {
  if (typeof exports === "string") {
    return exports.startsWith("./") ? [exports.slice(2)] : [];
  }
  return Object.values(exports ?? {}).flatMap(targets);
}

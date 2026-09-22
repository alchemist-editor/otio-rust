/**
 * Builds the WebAssembly module the package ships.
 *
 * The C ABI compiles to `wasm32-unknown-unknown` unchanged, so this is
 * `cargo build` with a target and nothing else: no bindgen step, no
 * post-processing, no toolchain beyond the one a Rust contributor already has.
 */

import { execFileSync } from "node:child_process";
import { mkdirSync, copyFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const workspace = join(here, "..", "..", "..", "..");
const profile = process.env.OTIO_WASM_PROFILE ?? "release";

execFileSync(
  "cargo",
  [
    "build",
    "--package",
    "otio-wasm",
    "--lib",
    "--target",
    "wasm32-unknown-unknown",
    ...(profile === "release" ? ["--release"] : []),
  ],
  { cwd: workspace, stdio: "inherit" },
);

const built = join(
  workspace,
  "target",
  "wasm32-unknown-unknown",
  profile,
  "otio_wasm.wasm",
);
const destination = join(here, "..", "vendor", "otio_wasm.wasm");
mkdirSync(dirname(destination), { recursive: true });
copyFileSync(built, destination);

const size = statSync(destination).size;
console.log(`otio_wasm.wasm: ${(size / 1024).toFixed(0)} KiB`);

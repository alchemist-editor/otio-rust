/**
 * The entry point Node resolves.
 *
 * Reads the `.wasm` from beside this file, which is where the package puts
 * it. Everything else comes from `index.ts` unchanged.
 */

import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { initAsync, ready } from "./runtime.js";

export * from "./index.js";

/**
 * Loads the WebAssembly module.
 *
 * Call it once, before anything else. Calling it again when it has already
 * loaded does nothing, so it is safe to put at the top of every entry point of
 * an application rather than threading a promise around.
 */
export async function init(): Promise<void> {
  if (ready()) {
    return;
  }
  const wasm = fileURLToPath(new URL("otio_wasm.wasm", import.meta.url));
  await initAsync(await readFile(wasm));
}

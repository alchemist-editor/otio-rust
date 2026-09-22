/**
 * The entry point a browser, and any bundler targeting one, resolves.
 *
 * Fetches the `.wasm` from beside this file and compiles it as it arrives.
 * `new URL("...", import.meta.url)` is the form every bundler understands, so
 * the file is found whether this is served as-is or built into an application.
 */

import { initAsync, ready } from "./runtime.js";

export * from "./index.js";

/**
 * Loads the WebAssembly module.
 *
 * Call it once, before anything else, and `await` it. Calling it again when it
 * has already loaded does nothing.
 *
 * Pass a `url` when the `.wasm` is somewhere the default would not find it: a
 * CDN, a versioned asset path, a service worker's cache.
 */
export async function init(url?: string | URL): Promise<void> {
  if (ready()) {
    return;
  }
  const source = url ?? new URL("otio_wasm.wasm", import.meta.url);
  await initAsync(fetch(source));
}

/**
 * Serves the compiled package to a browser, for the browser half of the tests.
 *
 * A browser will not `import` from a `file:` URL and will not instantiate
 * WebAssembly it fetched from one, so the test needs an origin. This is the
 * smallest thing that is one: no dependency, no configuration, and it serves
 * the working tree as it stands so what the browser runs is what `npm run
 * build` just produced.
 */

import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const port = Number(process.env.OTIO_TEST_PORT ?? 8901);

/** What to call each kind of file, since a browser insists on being told. */
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
};

const server = createServer((request, response) => {
  const path = normalize(new URL(request.url ?? "/", "http://localhost").pathname);
  const file = join(root, path);
  if (!file.startsWith(root)) {
    response.writeHead(403).end("outside the package");
    return;
  }
  try {
    if (!statSync(file).isFile()) {
      throw new Error("not a file");
    }
  } catch {
    response.writeHead(404).end("no such file");
    return;
  }
  response.writeHead(200, {
    "content-type": types[extname(file)] ?? "application/octet-stream",
    // The tests instantiate the module from a stream, which a browser only
    // allows for a response it was told is WebAssembly.
    "cache-control": "no-store",
  });
  createReadStream(file).pipe(response);
});

server.listen(port, () => {
  console.log(`serving ${root} on http://localhost:${port}`);
});

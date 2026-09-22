/**
 * How the browser half of the tests runs.
 *
 * Chromium only, and on purpose: what is being tested is that the package
 * works in a browser at all — that the module streams in, that the memory
 * grows under someone else's allocator, that nothing reaches for a Node
 * built-in. That is the same in every engine, and a matrix of three browsers
 * would cost three times as much to say it once.
 */

import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.OTIO_TEST_PORT ?? 8901);

export default defineConfig({
  testDir: "./test",
  testMatch: "**/*.spec.ts",
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  reporter: process.env.CI ? "list" : "line",
  use: {
    baseURL: `http://localhost:${port}`,
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        // Normally Playwright's own Chromium, installed by `playwright
        // install`. `OTIO_CHROMIUM` is for a machine that already has one and
        // would rather not download a second: point it at the binary.
        ...(process.env.OTIO_CHROMIUM === undefined
          ? {}
          : { launchOptions: { executablePath: process.env.OTIO_CHROMIUM } }),
      },
    },
  ],
  webServer: {
    command: "node scripts/serve.mjs",
    url: `http://localhost:${port}/test/browser.html`,
    reuseExistingServer: !process.env.CI,
    stdout: "pipe",
  },
});

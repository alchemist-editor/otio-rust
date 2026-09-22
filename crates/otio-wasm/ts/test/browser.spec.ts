/**
 * The suite, run in Chromium.
 *
 * The same cases as `node.test.ts`, loaded through the browser entry point
 * instead: the module arrives over `fetch` and is compiled as it streams, the
 * text goes through the browser's own `TextDecoder`, and the memory grows
 * under a browser's allocator. Those are the three things that could differ
 * between the two environments, and this is what would catch it.
 *
 * One page is loaded for the whole file, because compiling the module costs
 * more than every case put together.
 */

import { expect, test, type Page } from "@playwright/test";

import { cases } from "./suite.js";

let page: Page;

test.beforeAll(async ({ browser }) => {
  page = await browser.newPage();
  const problems: string[] = [];
  page.on("pageerror", (error) => problems.push(String(error)));
  await page.goto("/test/browser.html");
  await page.waitForFunction(() => "otioRun" in window, undefined, {
    timeout: 30_000,
  });
  expect(problems, "the page reported an error while loading").toEqual([]);
});

test.afterAll(async () => {
  await page.close();
});

for (const each of cases) {
  test(each.name, async () => {
    // The case runs in the page, so the failure that comes back is the one
    // `suite.ts` threw rather than a wrapper around it.
    await page.evaluate((name) => {
      (window as unknown as { otioRun(name: string): void }).otioRun(name);
    }, each.name);
  });
}

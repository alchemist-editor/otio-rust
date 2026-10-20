/**
 * The suite, run in Node.
 *
 * Imports the package the way an application in Node does — through the
 * `node` entry point, which reads the `.wasm` off disk — so what is tested is
 * the shape the package ships and not the source tree behind it.
 */

import { deepStrictEqual, strictEqual } from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { before, test } from "node:test";
import { fileURLToPath } from "node:url";

import * as sync from "../src/ext/sync.js";
import * as otio from "../src/node.js";
import { exports } from "../src/runtime.js";
import { cases } from "./suite.js";

before(async () => {
  await otio.init();
});

for (const each of cases) {
  test(each.name, () => {
    each.run(otio);
  });
}

/*
 * Not in `suite.ts`, because it looks past the package's surface at the
 * module's memory, which only a runner that loaded the same copy of the
 * runtime can reach.
 */
test("a failure's message is freed, and so is the one a missing value brings", () => {
  // Every call that can fail is handed a buffer for its message, and every
  // path out of the binding has to free it: `check` after a failure or a
  // success, `release` after "there is nothing". A path that forgot would
  // leak a few dozen bytes a call, which nothing else would notice. Tens of
  // thousands of calls turn that into megabytes, and the module's memory only
  // ever grows, so a leak shows up as memory that is bigger afterwards.
  const { Clip, OtioError, RationalTime, Track } = otio;
  const track = new Track({ name: "V1" });
  const clip = new Clip({ name: "shot_01" });
  const round = (count: number): void => {
    for (let index = 0; index < count; index += 1) {
      try {
        RationalTime.fromTimecode(`not a timecode ${index}`, 24);
        throw new Error("a bad timecode was read");
      } catch (thrown) {
        if (!(thrown instanceof OtioError) || thrown.message === "") {
          throw thrown;
        }
      }
      try {
        track.childAt(index);
        throw new Error("an empty track had a child");
      } catch (thrown) {
        if (!(thrown instanceof OtioError) || thrown.message === "") {
          throw thrown;
        }
      }
      if (clip.sourceRange !== undefined) {
        throw new Error("a new clip had a source range");
      }
      if (track.childCount() !== 0) {
        throw new Error("an empty track counted children");
      }
    }
  };

  // The first rounds settle the allocator and the scratch block, which grow
  // once on their own; only growth after that is the thing being looked for.
  round(2_000);
  const settled = exports().memory.buffer.byteLength;
  round(30_000);
  const after = exports().memory.buffer.byteLength;
  if (after !== settled) {
    throw new Error(
      `the module's memory grew from ${settled} to ${after} bytes ` +
        "over calls that should each have freed what they were handed",
    );
  }
});

/*
 * Before the test below turns sync on, which nothing in `suite.ts` does: Node
 * runs one file's tests in order, in a process of its own, so this is the
 * first sync call the instance sees.
 */
test("the sync extension refuses every call until it is enabled", () => {
  try {
    sync.newGuid();
    throw new Error("a GUID was made before sync was enabled");
  } catch (thrown) {
    if (
      !(thrown instanceof otio.OtioError) ||
      thrown.status !== "pluginError" ||
      !thrown.message.includes("otio_ext_sync_enable")
    ) {
      throw thrown;
    }
  }
});

/*
 * Not in `suite.ts` either, because the extension is a module of its own,
 * imported beside the package rather than through it. It has to run in the
 * instance `init()` loaded above, since nothing here loads another.
 */
test("the sync extension is a module of its own, on the core's instance", () => {
  sync.enable();
  if (!sync.isEnabled()) {
    throw new Error("sync is not on after enable()");
  }
  if (!otio.isExtensionEnabled("sync")) {
    throw new Error("the core does not see sync as on");
  }
  try {
    otio.enableExtension("no-such-extension");
    throw new Error("an extension that does not exist was enabled");
  } catch (thrown) {
    if (!(thrown instanceof otio.OtioError) || !thrown.message.includes("no-such-extension")) {
      throw thrown;
    }
  }
});

/** The repository, which the compiled test sits five directories under. */
const REPOSITORY = fileURLToPath(new URL("../../../../../", import.meta.url));

/**
 * The cut the `apply-patches` sample starts from: Timeline "cut", track V1,
 * clips A and B, none of them with a GUID yet.
 */
const CUT = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}`;

function openCut(): otio.Timeline {
  const timeline = otio.deserializeJsonFromString(CUT);
  if (!(timeline instanceof otio.Timeline)) {
    throw new Error("the cut is not a timeline");
  }
  return timeline;
}

/*
 * Furnace's golden corpus, the gate every SDK has to pass: the patches are its
 * own byte for byte, the fallbacks are where it falls back, and applying the
 * compact patches to the old timeline gives the new one.
 */
test("sync diffs and applies Furnace's golden corpus", () => {
  sync.enable();
  const fixtures = join(
    REPOSITORY,
    "extensions",
    "sync",
    "otio-ext-sync",
    "tests",
    "fixtures",
    "otiodiff",
  );
  const fellBack = new Set([
    "generation_regression",
    "guidless_transition",
    "metadata_sync_threshold_fallback",
    "sequence_changed",
  ]);
  let cases = 0;
  for (const name of readdirSync(fixtures).sort()) {
    const directory = join(fixtures, name);
    if (!statSync(directory).isDirectory()) {
      continue;
    }
    cases += 1;
    const oldText = readFileSync(join(directory, "old.json"), "utf8");
    const newText = readFileSync(join(directory, "new.json"), "utf8");

    const patches = sync.diff(oldText, newText, true);
    strictEqual(patches + "\n", readFileSync(join(directory, "patches.json"), "utf8"), name);

    const stats = sync.diffStats(oldText, newText);
    strictEqual(
      stats.includes(`"fellBack":${fellBack.has(name)}`),
      true,
      `${name}: ${stats}`,
    );

    const oldTimeline = JSON.stringify(JSON.parse(oldText).otioJson);
    const applied = sync.apply(oldTimeline, sync.diff(oldText, newText, false));
    deepStrictEqual(JSON.parse(applied), JSON.parse(newText).otioJson, `${name}: apply`);
  }
  strictEqual(cases, 11);
});

test("a sync transaction carries an edit to another copy of the timeline", () => {
  sync.enable();
  const editor = openCut();
  const before = sync.begin(editor, "main", 1);
  const snapshot = JSON.parse(before) as { sequenceId: string; generation: number };
  deepStrictEqual([snapshot.sequenceId, snapshot.generation], ["main", 1]);

  // The second copy is read from the stamped timeline, so both carry the
  // same GUIDs.
  const viewer = otio.deserializeJsonFromString(otio.serializeJsonToString(editor));
  if (!(viewer instanceof otio.Timeline)) {
    throw new Error("the copy is not a timeline");
  }

  editor.name = "fine cut";
  const patches = sync.commit(editor, before);
  strictEqual(patches.includes('"path":"/name"'), true, patches);
  sync.applyTo(viewer, patches);
  strictEqual(viewer.name, "fine cut", "the object held before applying is the one patched");

  const guid = sync.guid(editor);
  if (guid === undefined || guid === "") {
    throw new Error("the timeline has no GUID after begin()");
  }
  const found = sync.find(viewer, guid);
  strictEqual(found instanceof otio.Timeline, true);
  strictEqual(found?.name, "fine cut");
  strictEqual(sync.stamp(viewer), 0);
  strictEqual(sync.find(viewer, "nobody"), undefined);
  strictEqual(sync.guid(new otio.Clip({ name: "loose" })), undefined);
  strictEqual(sync.newGuid() === "", false);
});

test("sync rolls a transaction back, and journals edits", () => {
  sync.enable();
  const timeline = openCut();
  const before = sync.begin(timeline, "main", 1);
  timeline.name = "scrapped";
  sync.rollback(timeline, before);
  strictEqual(timeline.name, "cut");

  // A GUID set by hand is the one the object answers with.
  strictEqual(sync.find(timeline, sync.guid(timeline) ?? "") instanceof otio.Timeline, true);
  const mine = sync.newGuid();
  sync.setGuid(timeline, mine);
  strictEqual(sync.guid(timeline), mine);

  sync.startJournal(timeline);
  strictEqual(sync.commitJournal(timeline), false);
  timeline.name = "journalled";
  strictEqual(sync.commitJournal(timeline), true);
  strictEqual(sync.commitJournal(timeline), false, "a commit starts a fresh record");
  sync.stopJournal(timeline);
});

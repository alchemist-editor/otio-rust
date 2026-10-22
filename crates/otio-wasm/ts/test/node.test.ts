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

import * as ori from "../src/ext/ori.js";
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

/*
 * Sessions. A session owns no socket: the test plays the server, handing each
 * frame Furnace would send to `sessionReceive()` and reading what the client
 * would send from `sessionPollTransmit()`.
 */

/** What the server sends a viewer joining a session whose host shared one timeline. */
const VIEWER_WELCOME =
  '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","role":"viewer","roster":[{"id":"p_1","displayName":"Host","role":"host","joinedAt":0,"connectionState":"connected","muted":false}],"session":{"id":"s_1","createdAt":0},"snapshot":{"sequenceId":"main","generation":1,"otioJson":{"OTIO_SCHEMA":"Timeline.1","name":"Coastline","metadata":{"sync":{"guid":"t-1"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"s-1"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"v-1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Waves","metadata":{"sync":{"guid":"c-1"}}}]}]}}}}}';

/** What the server sends the host of a new session. */
const HOST_WELCOME =
  '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_1","role":"host","roster":[],"session":{"id":"s_1","createdAt":0}}}';

/** Every frame the session has to send, as text, until it has none. */
function drain(session: number): string[] {
  const frames: string[] = [];
  for (;;) {
    const frame = sync.sessionPollTransmit(session, 1_000);
    if (frame === undefined) {
      return frames;
    }
    strictEqual(frame.binary, false, "a session that accepts no encoding sends text");
    frames.push(new TextDecoder().decode(frame.bytes));
  }
}

/** Every event the session has, until it has none. */
function events(session: number): string[] {
  const all: string[] = [];
  for (let event = sync.sessionPollEvent(session); event !== undefined; event = sync.sessionPollEvent(session)) {
    all.push(event);
  }
  return all;
}

function receive(session: number, frame: string): void {
  sync.sessionReceive(session, new TextEncoder().encode(frame), false);
}

function readTimeline(json: string): otio.Timeline {
  const timeline = otio.deserializeJsonFromString(json);
  if (!(timeline instanceof otio.Timeline)) {
    throw new Error("not a timeline");
  }
  return timeline;
}

function throwsStatus(call: () => unknown, status: string, what: string): void {
  try {
    call();
  } catch (thrown) {
    if (!(thrown instanceof otio.OtioError) || thrown.status !== status) {
      throw thrown;
    }
    return;
  }
  throw new Error(`${what} succeeded`);
}

test("a viewer joins a session and takes the host's timeline", () => {
  sync.enable();
  const session = sync.sessionNew('{"token":"join-code","identity":{"displayName":"Ada"},"encodings":[]}');
  strictEqual(sync.sessionPollTransmit(session, 0), undefined, "nothing is sent before connecting");

  sync.sessionConnect(session);
  const hello = sync.sessionPollTransmit(session, 1_000);
  if (hello === undefined) {
    throw new Error("connecting sent no hello");
  }
  strictEqual(hello.binary, false);
  const helloText = new TextDecoder().decode(hello.bytes);
  strictEqual(helloText.includes('"type":"hello"'), true, helloText);
  strictEqual(helloText.includes("Ada"), true, helloText);
  strictEqual(sync.sessionPollTransmit(session, 1_000), undefined);

  receive(session, VIEWER_WELCOME);
  const arrived = events(session);
  strictEqual(arrived.length, 2, arrived.join("\n"));
  const [timelineEvent, joinedEvent] = arrived.map((each) => JSON.parse(each) as { type: string; joining?: boolean });
  strictEqual(timelineEvent?.type, "timeline");
  strictEqual(timelineEvent?.joining, true);
  strictEqual(joinedEvent?.type, "joined");

  const timeline = readTimeline('{"OTIO_SCHEMA":"Timeline.1"}');
  deepStrictEqual(JSON.parse(sync.handle(timeline, arrived[0] ?? "")), { handled: "applied" });
  strictEqual(timeline.name, "Coastline");
  const tracks = timeline.tracks;
  strictEqual(tracks?.childCount(), 1);
  const track = tracks?.childAt(0);
  if (!(track instanceof otio.Track)) {
    throw new Error("the timeline's first child is not a track");
  }
  strictEqual(track.childCount(), 1);
  strictEqual(track.childAt(0).name, "Waves");
  deepStrictEqual(JSON.parse(sync.handle(timeline, arrived[1] ?? "")), { handled: "joined" });

  const state = sync.sessionState(session);
  strictEqual(state.includes('"role":"viewer"'), true, state);
  strictEqual(state.includes('"joined":true'), true, state);
  strictEqual(sync.sessionTimeline(session, "main")?.includes('"sequenceId":"main"'), true);
  strictEqual(sync.sessionTimeline(session, "other"), undefined);

  strictEqual(sync.sessionMaySend(session, "timeline"), false, "a viewer does not hold the timeline");
  throwsStatus(() => sync.sessionPublish(session, timeline, "main"), "invalidArgument", "a viewer's publish");

  sync.sessionSetPlayhead(session, "main", 12, true, 1);
  const presence = drain(session);
  strictEqual(presence.some((frame) => frame.includes('"type":"presence"')), true, presence.join("\n"));

  sync.sessionClose(session);
  sync.sessionClose(session);
  throwsStatus(() => sync.sessionState(session), "invalidArgument", "a call on a freed session");
});

test("a host publishes its timeline, and only what changed", () => {
  sync.enable();
  const session = sync.sessionNew('{"token":"host-token","encodings":[]}');
  sync.sessionConnect(session);
  drain(session);
  receive(session, HOST_WELCOME);
  const first = JSON.parse(sync.sessionPollEvent(session) ?? "{}") as { type?: string; role?: string };
  strictEqual(first.type, "joined");
  strictEqual(first.role, "host");
  events(session);

  const timeline = openCut();
  const patches = sync.sessionPublish(session, timeline, "main");
  strictEqual(patches.includes("replace_timeline"), true, patches);
  const sent = drain(session);
  strictEqual(sent.some((frame) => frame.includes('"type":"snapshot"')), true, sent.join("\n"));

  strictEqual(sync.sessionPublish(session, timeline, "main"), "[]");
  timeline.name = "fine cut";
  const renamed = sync.sessionPublish(session, timeline, "main");
  strictEqual(renamed.includes("set_property"), true, renamed);
  sync.sessionClose(session);
});

test("a session refuses settings it cannot honour", () => {
  sync.enable();
  throwsStatus(() => sync.sessionNew('{"encodings":["zstd"]}'), "parseError", "a session that accepts zstd");
});

/** A file ORI's own Python wrote: a shot annotated at frames 10 and 4. */
function oriBuilt(): otio.Timeline {
  const text = readFileSync(
    join(REPOSITORY, "extensions/ori/otio-ext-ori-annotations/tests/fixtures/ori_built.otio"),
    "utf8",
  );
  const timeline = otio.deserializeJsonFromString(text);
  if (!(timeline instanceof otio.Timeline)) {
    throw new Error("ori_built.otio is not a timeline");
  }
  return timeline;
}

test("ORI Annotations turn on with sync, and read what ORI wrote", () => {
  ori.enable();
  strictEqual(ori.isEnabled(), true);
  strictEqual(otio.isExtensionEnabled("ori-annotations"), true);
  strictEqual(otio.isExtensionEnabled("sync"), true);

  const timeline = oriBuilt();
  const holders = ori.list(timeline);
  strictEqual(holders.length, 2);
  strictEqual(
    ori.describe(holders[0]!),
    '{"form":"track","clip_guid":"shot","clip_name":null,"frame":{"value":10.0,"rate":24.0},"note":null,"status":null,"command_count":4}',
  );
  const track = ori.findTrack(timeline);
  strictEqual(track?.name, "Annotations");
  strictEqual(ori.findClip(track!, "shot", 99), undefined);
  strictEqual(ori.listAtFrame(timeline, "shot", 4).length, 1);
  strictEqual(ori.listForClip(timeline, "shot").length, 2);
});

test("ORI Annotations annotate a frame and edit it by uuid", () => {
  ori.enable();
  const commands = ori.commands(ori.list(oriBuilt())[0]!, false);

  const timeline = otio.deserializeJsonFromString(
    '{"OTIO_SCHEMA": "Timeline.1", "tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}',
  ) as otio.Timeline;
  const track = ori.ensureTrack(timeline);
  const clip = ori.ensureClip(track, "shot", 10, 24);
  ori.addCommands(clip, commands);
  ori.ensureClip(track, "shot", 10, 24);
  strictEqual(ori.list(timeline).length, 1, "asking again adds no second clip");
  strictEqual(ori.commands(clip, false), commands);

  strictEqual(
    ori.replaceCommands(clip, "stroke-1", '[{"OTIO_SCHEMA": "TextAnnotation.1", "uuid": "stroke-1", "text": "was a stroke"}]'),
    true,
  );
  strictEqual(ori.removeCommands(clip, '["text-1"]'), 1);
  const left = ori.commands(clip, false);
  strictEqual(left.includes('"text":"was a stroke"'), true, left);
  strictEqual(left.includes("text-1"), false, left);
  ori.setCommands(clip, "[]");
  strictEqual(ori.commands(clip, false), "[]");
  try {
    ori.addCommands(clip, "not json");
    throw new Error("commands that are not JSON were added");
  } catch (thrown) {
    if (!(thrown instanceof otio.OtioError)) {
      throw thrown;
    }
  }

  const corner = ori.pixelsToOri(1920, 1080, 0, 0);
  strictEqual(Math.abs(corner.x + 8 / 9) < 1e-12 && Math.abs(corner.y - 0.5) < 1e-12, true);
  const pixel = ori.oriToPixels(1920, 1080, corner.x, corner.y);
  strictEqual(Math.abs(pixel.px) < 1e-9 && Math.abs(pixel.py) < 1e-9, true);
  deepStrictEqual(ori.oriToBounds(0.5, 0.5, -16, -9, 16, 9), { x: 9, y: 9 });
});

const ORI_TIMELINE =
  '{"OTIO_SCHEMA":"Timeline.1","name":"Program","metadata":{"sync":{"guid":"main"}},"tracks":{"OTIO_SCHEMA":"Stack.1","metadata":{"sync":{"guid":"main/stack"}},"children":[{"OTIO_SCHEMA":"Track.1","name":"V1","kind":"Video","metadata":{"sync":{"guid":"main/v1"}},"children":[{"OTIO_SCHEMA":"Clip.2","name":"Beach Run","metadata":{"sync":{"guid":"main/clip-1"}},"source_range":{"OTIO_SCHEMA":"TimeRange.1","start_time":{"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":24},"duration":{"OTIO_SCHEMA":"RationalTime.1","value":48,"rate":24}}}]}]}}';
const ORI_STROKE =
  '[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s1","rgba":[1,0,0,1]},{"OTIO_SCHEMA":"PaintPoint.1","uuid":"s1","points":{"OTIO_SCHEMA":"PaintVertices.1","x":[0.1],"y":[0.2],"size":[0.01],"alpha":[1]}},{"OTIO_SCHEMA":"PaintEnd.1","uuid":"s1"}]';
const ORI_ANCHOR =
  '{"sequenceId":"main","capturedGeneration":1,"clipGuid":"main/clip-1","sourceTime":{"value":10,"rate":24},"capturedSequenceTime":{"value":10,"rate":24},"capturedAt":1787742000000}';
const ORI_WELCOME =
  '{"v":2,"type":"welcome","sessionId":"s_1","seq":0,"ts":0,"order":1,"payload":{"selfId":"p_2","selfUserId":"u_bea","role":"viewer","roster":[],"session":{"id":"s_1","createdAt":0}}}';
const ORI_RELAY = `{"type":"review_relay","commit":{"clientMutationId":"m1","anchor":${ORI_ANCHOR},"bodyMarkdown":"Too dark.","annotationCommands":${ORI_STROKE}},"ack":{"clientMutationId":"m1","commentId":"c1","serverOrder":1,"revision":1},"authorUserId":"u_bea","authorPresenceId":"p_2","createdAt":0}`;
const ORI_PARTIAL =
  '{"type":"message","kind":"Annotation.1/PARTIAL","sender":"p_3","payload":{"clip_guid":"main/clip-1","frame":10.0,"fps":24.0,"events":[{"OTIO_SCHEMA":"PaintStart.1","uuid":"s2"}]}}';

test("ORI Annotations merge by uuid, and kind by kind", () => {
  ori.enable();
  const timeline = readTimeline(ORI_TIMELINE);
  const clip = ori.ensureClip(ori.ensureTrack(timeline), "main/clip-1", 10, 24);
  ori.setCommands(clip, '[{"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"a"}]');
  deepStrictEqual(
    ori.mergeCommands(clip, '[{"OTIO_SCHEMA":"PaintStart.1","uuid":"a","brush":"gauss"},{"OTIO_SCHEMA":"PaintStart.1","uuid":"b"}]'),
    { replaced: 1, added: 1 },
  );
  strictEqual(ori.replaceByKind(clip, '[{"OTIO_SCHEMA":"PaintStart.1","uuid":"c"}]'), 2);
  const left = ori.commands(clip, false);
  strictEqual(left.includes('"uuid":"t"') && left.includes('"uuid":"c"'), true, left);
  strictEqual(ori.mergeDuplicates(timeline), 0);
  strictEqual(ori.ownerOf(clip, "t"), undefined);
});

test("ORI Annotations: a reviewer commits a stroke and the host folds it in", () => {
  ori.enable();
  const partial = ori.readPartial(ORI_PARTIAL);
  strictEqual(partial?.includes('"brush":"circle"'), true, partial);
  strictEqual(ori.readPartial('{"type":"leader","leader":null}'), undefined);

  const reviewer = sync.sessionNew(
    '{"token":"join-code","identity":{"capabilities":["review.comment.create","review.annotation.create"]},"encodings":[]}',
  );
  sync.sessionConnect(reviewer);
  while (sync.sessionPollTransmit(reviewer, 0) !== undefined) {
    // The hello.
  }
  receive(reviewer, ORI_WELCOME);
  events(reviewer);
  const id = ori.commit(reviewer, readTimeline(ORI_TIMELINE), ORI_ANCHOR, "Too dark.", ORI_STROKE, "");
  strictEqual(id.startsWith("mutation_"), true, id);
  const sent = sync.sessionPollTransmit(reviewer, 0);
  const sentText = new TextDecoder().decode(sent?.bytes);
  strictEqual(sentText.includes('"type":"review_commit"'), true, sentText);
  ori.sendPartial(reviewer, '{"clip_guid":"main/clip-1","frame":10,"fps":24,"events":[]}');
  throwsStatus(() => sync.sessionPollTransmit(reviewer, 0), "unsupported", "Furnace sending a stroke in progress");
  sync.sessionClose(reviewer);

  const timeline = readTimeline(ORI_TIMELINE);
  const folded = ori.fold(timeline, ORI_RELAY);
  strictEqual(folded.already, false);
  strictEqual(ori.fold(timeline, ORI_RELAY).already, true);
  const commands = ori.commands(folded.clip, false);
  strictEqual(commands.split('"uuid":"s1"').length - 1, 3, commands);
  strictEqual(ori.ownerOf(folded.clip, "s1"), "u_bea");
});

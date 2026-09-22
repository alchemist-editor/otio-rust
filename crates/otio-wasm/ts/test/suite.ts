/**
 * The tests, written once and run twice.
 *
 * A package that ships to a browser and to Node has two of everything that
 * could go wrong — how the module is fetched, whether `TextDecoder` is there,
 * whether the memory grows the same way — and one API. So the cases live here,
 * as plain functions over the package's own public surface, and the two
 * runners in this directory each hand them an initialised copy of it. A case
 * that passes in Node and fails in Chromium is a real difference between the
 * two, not a difference between two test files.
 *
 * Nothing here imports `node:test`, `node:assert`, or anything else a browser
 * does not have. The runner supplies the reporting.
 */

import type * as otio from "../src/index.js";

import { exports, scratchForTesting } from "../src/runtime.js";
import { conformance } from "./conformance.js";

/** The package, as the tests see it. */
export type Otio = typeof otio;

/**
 * The EDL's timeline, with what an AAF needs and an EDL does not say: how
 * much media each clip's source holds.
 */
function aafReady(api: Otio): otio.Timeline {
  const { RationalTime, TimeRange } = api;
  const timeline = api.readTimelineFromString("cmx3600", EDL, { rate: 24 });
  for (const clip of timeline.findClips()) {
    const media = clip.mediaReference();
    ok(media !== undefined, "an EDL clip with no media reference");
    media.availableRange = new TimeRange(
      new RationalTime(86400, 24),
      new RationalTime(24 * 60, 24),
    );
  }
  return timeline;
}

/** One test. */
export interface Case {
  /** What it is called, in the report. */
  readonly name: string;
  /** What it does. Throwing is failing. */
  run(api: Otio): void;
}

/** Fails the test unless `condition` holds. */
function ok(condition: boolean, what: string): asserts condition {
  if (!condition) {
    throw new Error(what);
  }
}

/** Fails the test unless two values are the same. */
function is<T>(found: T, wanted: T, what: string): void {
  if (!Object.is(found, wanted)) {
    throw new Error(`${what}: expected ${String(wanted)}, found ${String(found)}`);
  }
}

/** Fails the test unless `body` throws. */
function throws(body: () => unknown, what: string): unknown {
  try {
    body();
  } catch (thrown) {
    return thrown;
  }
  throw new Error(`${what}: nothing was thrown`);
}

/** A two-cut EDL, small enough to read in the failure message. */
const EDL = [
  "TITLE: cut",
  "",
  "001  AX       V     C        00:00:00:00 00:00:04:00 01:00:00:00 01:00:04:00",
  "* FROM CLIP NAME: shot_01",
  "002  AX       V     C        00:00:10:00 00:00:14:00 01:00:04:00 01:00:08:00",
  "* FROM CLIP NAME: shot_02",
  "",
].join("\n");

/**
 * Runs `body` from a fresh scratch block, with every block the runtime lets go
 * of filled with junk as it goes.
 *
 * Freed memory usually still holds what was in it, so a call that goes on
 * reading a block after it was freed passes by luck until the allocator hands
 * those bytes to someone else. Filling each block as it is released is that
 * reuse, made to happen every time and at the worst moment. Starting from the
 * first size means the block has to grow, whatever earlier cases left it at.
 *
 * This reaches past the package's surface into the runtime, which the runners
 * share with the package they hand the cases, so it is the same block.
 */
function outgrowingScratch(body: () => void): void {
  scratchForTesting.reset();
  let released = 0;
  scratchForTesting.onRelease = (pointer, size) => {
    released += 1;
    new Uint8Array(exports().memory.buffer, pointer, size).fill(0xa5);
  };
  try {
    body();
  } finally {
    scratchForTesting.onRelease = undefined;
  }
  ok(released > 0, "the scratch block never grew, so nothing was tested");
}

export const cases: readonly Case[] = [
  {
    name: "the module reports a version",
    run(api) {
      ok(api.version().length > 0, "version() said nothing");
    },
  },

  {
    name: "rational time does upstream's arithmetic",
    run(api) {
      const { RationalTime } = api;
      const a = new RationalTime(24, 24);
      const b = new RationalTime(12, 24);
      is(a.add(b).toSeconds(), 1.5, "24/24 + 12/24 in seconds");
      is(a.subtract(b).value, 12, "24/24 - 12/24");
      is(new RationalTime(48, 48).toSeconds(), 1, "48/48 in seconds");
      is(RationalTime.fromSecondsAtRate(1.5, 24).value, 36, "1.5s at 24");
    },
  },

  {
    name: "timecode round-trips through a rational time",
    run(api) {
      const { RationalTime } = api;
      const time = RationalTime.fromTimecode("01:00:04:00", 24);
      is(time.value, 86_496, "01:00:04:00 at 24 in frames");
      is(time.toTimecode(), "01:00:04:00", "back to timecode");
      is(new RationalTime(0, 24).toTimecode(), "00:00:00:00", "zero");
    },
  },

  {
    name: "a bad timecode throws, and says which status",
    run(api) {
      const thrown = throws(
        () => api.RationalTime.fromTimecode("not a timecode", 24),
        "a bad timecode",
      );
      ok(thrown instanceof api.OtioError, "the error was not an OtioError");
      ok(thrown.status.length > 0, "the error carried no status");
      ok(thrown.message.length > 0, "the error carried no message");
    },
  },

  {
    name: "every failure carries its own message, however they interleave",
    run(api) {
      // The message comes back from the call that failed, in a slot that
      // call was handed, and not from a second call asking the library what
      // went wrong last. So two kinds of failure taken in turn, with answers
      // and "there is nothing"s in between, each have to arrive with the
      // sentence about themselves: the timecode that was bad, the index that
      // was out of range. A message read from anywhere shared would, sooner
      // or later, name the other one.
      //
      // JavaScript runs one call into the module at a time, so there is no
      // thread to race here the way there is in Go; interleaving is the
      // nearest thing, and it is what would catch a message left over from
      // one call being handed to the next.
      const { Clip, RationalTime, Track } = api;
      const track = new Track({ name: "V1" });
      const clip = new Clip({ name: "shot_01" });
      for (let round = 0; round < 200; round += 1) {
        const timecode = `not a timecode ${round}`;
        const early = throws(
          () => RationalTime.fromTimecode(timecode, 24),
          `bad timecode, round ${round}`,
        );
        ok(early instanceof api.OtioError, "the timecode error was not an OtioError");
        is(early.status, "timeError", `the timecode error's status, round ${round}`);
        ok(
          early.message.includes(timecode),
          `round ${round}: the timecode error said ${JSON.stringify(early.message)}`,
        );

        // An answer, and an answer that is nothing, between the two failures:
        // neither may disturb what the next failure says, and the nothing
        // comes with a message of its own that has to be let go of quietly.
        is(track.childCount(), 0, "an empty track's children");
        is(clip.sourceRange, undefined, "a new clip's source range");

        const index = 1000 + round;
        const late = throws(() => track.childAt(index), `child ${index}`);
        ok(late instanceof api.OtioError, "the index error was not an OtioError");
        is(late.status, "invalidArgument", `the index error's status, round ${round}`);
        ok(
          late.message.includes(`index ${index}`),
          `round ${round}: the index error said ${JSON.stringify(late.message)}`,
        );
      }

      // And one the types would never allow, reached round them: `kind` on a
      // track read off a clip. The library is the one that knows it is not a
      // track, and its reason is the one that should come back.
      const kind = Object.getOwnPropertyDescriptor(Track.prototype, "kind");
      ok(kind?.get !== undefined, "Track has no kind to read");
      const wrong = throws(() => kind.get?.call(clip), "a clip asked for a track's kind");
      ok(wrong instanceof api.OtioError, "the kind error was not an OtioError");
      ok(
        wrong.message.includes("not a track"),
        `the kind error said ${JSON.stringify(wrong.message)}`,
      );
    },
  },

  {
    name: "a time range knows where it ends",
    run(api) {
      const { RationalTime, TimeRange } = api;
      const range = new TimeRange(
        new RationalTime(24, 24),
        new RationalTime(48, 24),
      );
      is(range.endTimeExclusive().value, 72, "end, exclusive");
      is(range.endTimeInclusive().value, 71, "end, inclusive");
      ok(range.containsTime(new RationalTime(30, 24)), "30 is inside");
      ok(!range.containsTime(new RationalTime(90, 24)), "90 is outside");
    },
  },

  {
    name: "an object is built on its own and put inside a track",
    run(api) {
      const { Clip, RationalTime, Track, TimeRange } = api;
      const track = new Track({ name: "V1" });
      const clip = new Clip({ name: "shot_01" });
      clip.sourceRange = new TimeRange(
        new RationalTime(0, 24),
        new RationalTime(48, 24),
      );
      track.appendChild(clip);

      is(track.childCount(), 1, "the track's children");
      is(track.childAt(0).name, "shot_01", "the child's name");
      is(track.duration().value, 48, "the track's duration");
      is(clip.parent()?.name, "V1", "the clip's parent, after the move");
    },
  },

  {
    name: "a wrapper survives the move into another document",
    run(api) {
      const { Clip, Track } = api;
      const clip = new Clip({ name: "shot_01" });
      const track = new Track({ name: "V1" });
      track.appendChild(clip);

      // `clip` was made in a document of its own and now lives in the track's.
      // Reading it back has to find the same object, and the wrapper handed
      // out before the move has to go on working.
      is(clip.name, "shot_01", "the old wrapper still reads");
      clip.name = "renamed";
      is(track.childAt(0).name, "renamed", "the write reached the same object");
      ok(track.childAt(0).equals(clip), "the child is the clip");
    },
  },

  {
    name: "an object from another timeline is refused, not mistaken for a local one",
    run(api) {
      // Handles are per document and their numbers repeat across documents,
      // so the same pair of integers names a different object in each. A call
      // that took one without checking would answer about whatever happened
      // to sit in that slot here, quietly and wrongly.
      const { Clip, Track } = api;
      const here = new Track({ name: "A" });
      here.appendChild(new Clip({ name: "in A" }));
      const elsewhere = new Track({ name: "B" });
      const theirs = new Clip({ name: "in B" });
      elsewhere.appendChild(theirs);

      ok(!here.childAt(0).equals(theirs), "two documents' objects compared equal");
      for (const [what, body] of [
        ["hasChild", () => here.hasChild(theirs)],
        ["indexOfChild", () => here.indexOfChild(theirs)],
        ["rangeOfChild", () => here.rangeOfChild(theirs)],
        ["isParentOf", () => here.isParentOf(theirs)],
      ] as const) {
        const thrown = throws(body, `${what} across documents`);
        ok(thrown instanceof Error, `${what} threw something odd`);
      }
    },
  },

  {
    name: "an editing call that only names an object refuses a foreign one",
    run(api) {
      // Refusing a foreign object on the calls that ask questions is half of
      // it. Most of the editing calls *name* an object rather than place
      // one: `flattenTracks` is handed the tracks it reads, `detachChild`
      // the child it is about to remove. Moving a foreign object into this
      // document first makes those calls succeed, and what they succeed at
      // is not what was asked: two timelines that shared no memory now share
      // one, silently, on a call that was only supposed to read.
      const { Clip, RationalTime, Stack, TimeRange, Track, algorithms } = api;
      const span = () =>
        new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24));
      const shot = (name: string) => {
        const clip = new Clip({ name });
        clip.sourceRange = span();
        return clip;
      };
      const layer = (name: string) => {
        const track = new Track({ name });
        track.appendChild(shot(`in ${name}`));
        return track;
      };

      // Two tracks in one timeline flatten, which is the call working.
      const stack = new Stack({ name: "tracks" });
      const lower = layer("V1");
      const upper = layer("V2");
      stack.appendChild(lower);
      stack.appendChild(upper);
      is(algorithms.flattenTracks([lower, upper]).childCount(), 1, "the flattened track's children");

      // One from somewhere else does not, and does not drag its timeline in
      // behind it either.
      const foreign = layer("elsewhere");
      const thrown = throws(
        () => algorithms.flattenTracks([lower, foreign]),
        "flattening two tracks from different timelines",
      );
      ok(thrown instanceof Error, "flattenTracks threw something odd");
    },
  },

  {
    name: "a refused object is left in the timeline it came from",
    run(api) {
      // What the refusal is protecting. The two timelines here are
      // independent, so disposing one has to leave the other alone. A call
      // that quietly moved the second timeline's objects into the first
      // would make the first's disposal take both — and nothing in between
      // would have reported anything wrong.
      const { Clip, Track } = api;
      const here = new Track({ name: "A" });
      here.appendChild(new Clip({ name: "in A" }));
      const elsewhere = new Track({ name: "B" });
      const theirs = new Clip({ name: "in B" });
      elsewhere.appendChild(theirs);

      for (const [what, body] of [
        ["detachChild", () => here.detachChild(theirs)],
        ["neighborsOf", () => here.neighborsOf(theirs, "never")],
      ] as const) {
        const thrown = throws(body, `${what} across documents`);
        ok(thrown instanceof Error, `${what} threw something odd`);
      }

      here.dispose();
      is(elsewhere.childCount(), 1, "the other track's children after this one was disposed");
      is(theirs.name, "in B", "the other track's child after this one was disposed");
    },
  },

  {
    name: "an editing call that places an object still takes one from elsewhere",
    run(api) {
      // The other half: `appendChild` is handed an object precisely so it
      // can put it in, and every object starts life in a document of its
      // own. Refusing there would mean nothing could ever be appended.
      const { Clip, Track } = api;
      const track = new Track({ name: "V1" });
      const clip = new Clip({ name: "shot_01" });
      track.appendChild(clip);
      is(track.childCount(), 1, "the track's children after an append");
      ok(track.childAt(0).equals(clip), "the appended clip is the one handed over");

      // And once it is in, naming it is no longer foreign.
      is(track.indexOfChild(clip), 0, "the index of the clip just appended");
      track.detachChild(clip);
      is(track.childCount(), 0, "the track's children after detaching its own child");
    },
  },

  {
    name: "an edit puts a newly built item into a composition that already exists",
    run(api) {
      // The reason the document is hidden at all. An item is built on its
      // own, in a document of its own, and the edit operations have to bring
      // it into the timeline rather than refuse it for being elsewhere —
      // while still making the call in the *timeline's* document, because
      // the composition is the object that cannot be moved.
      const { Clip, Gap, RationalTime, TimeRange, Track, edit } = api;
      const span = (start: number, length: number) =>
        new TimeRange(new RationalTime(start, 24), new RationalTime(length, 24));

      const track = new Track({ name: "V1" });
      const first = new Clip({ name: "shot_01" });
      first.sourceRange = span(0, 24);
      track.appendChild(first);

      const second = new Clip({ name: "shot_02" });
      second.sourceRange = span(0, 24);
      edit.insert(second, track, new RationalTime(24, 24), false);
      is(track.childCount(), 2, "the track's children after inserting a new clip");
      is(track.childAt(1).name, "shot_02", "the name of the inserted clip");

      const third = new Clip({ name: "shot_03" });
      third.sourceRange = span(0, 24);
      edit.overwrite(third, track, span(0, 24), false);
      is(track.childAt(0).name, "shot_03", "the name of the clip laid over the first");

      // Fill is the same story, into a gap the track already has.
      const hole = new Gap({ name: "hole" });
      hole.sourceRange = span(0, 24);
      track.appendChild(hole);
      const fourth = new Clip({ name: "shot_04" });
      fourth.sourceRange = span(0, 24);
      edit.fill(fourth, track, new RationalTime(48, 24), "sequence");
      is(track.childCount(), 3, "the track's children after filling its gap");
      is(track.childAt(2).name, "shot_04", "the name of the filled clip");
    },
  },

  {
    name: "an optional object left out is still nothing, not a stray handle",
    run(api) {
      // The check on a node argument must not swallow `undefined`: an
      // optional object nobody passed has to reach the core as its own
      // "there is none" handle.
      const { Clip, RationalTime, TimeRange, Track, edit } = api;
      const track = new Track({ name: "V1" });
      const clip = new Clip({ name: "shot_01" });
      clip.sourceRange = new TimeRange(
        new RationalTime(0, 24),
        new RationalTime(24, 24),
      );
      track.appendChild(clip);
      edit.remove(track, new RationalTime(0, 24), false);
      is(track.childCount(), 0, "the clip after a remove with no fill template");
    },
  },

  {
    name: "reading the same object twice gives the same wrapper",
    run(api) {
      const { Clip, Track } = api;
      const track = new Track({ name: "V1" });
      track.appendChild(new Clip({ name: "shot_01" }));
      ok(track.childAt(0) === track.childAt(0), "=== disagreed with identity");
    },
  },

  {
    name: "a handle read out of a document arrives as its own class",
    run(api) {
      const { Clip, Stack, Timeline, Track } = api;
      const timeline = new Timeline({ name: "cut" });
      const stack = timeline.tracks;
      ok(stack instanceof Stack, "a timeline's tracks are a stack");
      const track = new Track({ name: "V1" });
      track.appendChild(new Clip({ name: "shot_01" }));
      stack?.appendChild(track);

      const found = timeline.findClips();
      is(found.length, 1, "clips under the timeline");
      ok(found[0] instanceof Clip, "findClips found something else");
      is(found[0]?.schemaKind(), "clip", "the clip's kind");
      is(found[0]?.schemaName(), "Clip", "the clip's schema");
    },
  },

  {
    name: "a subclass of a generated class is still built",
    run(api) {
      class Shot extends api.Clip {
        get slate(): string {
          return this.name.toUpperCase();
        }
      }
      const shot = new Shot({ name: "shot_01" });
      is(shot.slate, "SHOT_01", "the subclass's own member");
      is(shot.schemaName(), "Clip", "what the core thinks it is");
    },
  },

  {
    name: "an EDL reads, and its clips come back in order",
    run(api) {
      const timeline = api.readTimelineFromString("cmx3600", EDL, { rate: 24 });
      is(timeline.name, "cut", "the timeline's name");
      const clips = timeline.findClips();
      is(clips.length, 2, "how many clips the EDL held");
      is(clips[0]?.name, "shot_01", "the first clip");
      is(clips[1]?.name, "shot_02", "the second clip");
      is(clips[0]?.trimmedRange().duration.toTimecode(), "00:00:04:00", "its duration");
    },
  },

  {
    name: "an EDL round-trips through the writer",
    run(api) {
      const timeline = api.readTimelineFromString("cmx3600", EDL, { rate: 24 });
      const written = api.writeToString("cmx3600", timeline, { rate: 24 });
      const again = api.readTimelineFromString("cmx3600", written, { rate: 24 });
      is(
        again.findClips().map((clip) => clip.name).join(","),
        "shot_01,shot_02",
        "the clips, after a round trip",
      );
    },
  },

  {
    name: "an AAF round-trips, and the module needs no clock or randomness",
    run(api) {
      const timeline = aafReady(api);
      // The EDL's clips carry no MobIDs, so the writer has to make them up.
      const written = api.writeToBytes("aaf", timeline, { aafUseEmptyMobIds: true });
      is(
        Array.from(written.subarray(0, 4), (b) => b.toString(16)).join(" "),
        "d0 cf 11 e0",
        "a compound file's signature",
      );
      // An AAF can hold more than one composition, so it may read back as a
      // collection rather than a timeline; this takes whatever it holds.
      const again = api.readFromBytes("aaf", written);
      is(
        again.findClips().map((clip) => clip.name).join(","),
        "shot_01,shot_02",
        "the clips, after a round trip",
      );
      const nested = api.readFromBytes("aaf", written, { aafKeepNesting: true });
      is(nested.findClips().length, 2, "the clips, read with the nesting kept");
    },
  },

  {
    name: "an AAF is the same file for the same time and seed, and not otherwise",
    run(api) {
      const timeline = aafReady(api);
      const fixed = { aafUseEmptyMobIds: true, aafTime: 1714979289, aafIdSeed: 59 };
      const first = api.writeToBytes("aaf", timeline, fixed);
      const second = api.writeToBytes("aaf", timeline, fixed);
      ok(
        first.length === second.length && first.every((b, i) => b === second[i]),
        "two writes with the same time and seed differ",
      );
      const fresh = api.writeToBytes("aaf", timeline, { aafUseEmptyMobIds: true });
      const other = api.writeToBytes("aaf", timeline, { aafUseEmptyMobIds: true });
      ok(
        fresh.length !== other.length || fresh.some((b, i) => b !== other[i]),
        "two writes drew the same identifiers",
      );
    },
  },

  {
    name: "a timeline round-trips through OTIO JSON",
    run(api) {
      const timeline = api.readTimelineFromString("cmx3600", EDL, { rate: 24 });
      const json = api.serializeJsonToString(timeline);
      ok(json.includes("\"OTIO_SCHEMA\""), "that was not OTIO JSON");
      const again = api.deserializeJsonFromString(json);
      ok(again instanceof api.Timeline, "the JSON did not hold a timeline");
      is(again.findClips().length, 2, "the clips, after JSON");
    },
  },

  {
    name: "reading something that is not a timeline says so",
    run(api) {
      const json = api.serializeJsonToString(new api.Clip({ name: "shot_01" }));
      // The plain reader answers with whatever the file held, which here is a
      // clip, and the checked one says so rather than handing back something
      // that is not a timeline.
      ok(
        api.deserializeJsonFromString(json) instanceof api.Clip,
        "the JSON did not hold a clip",
      );
      const thrown = throws(
        () => api.readTimelineFromString("otioJson", json),
        "a clip read as a timeline",
      );
      ok(thrown instanceof TypeError, "nothing useful was thrown");
      ok(
        String((thrown as Error).message).includes("Clip"),
        "the error did not say what it found",
      );
    },
  },

  {
    name: "metadata holds a tree and reads it back",
    run(api) {
      const clip = new api.Clip({ name: "shot_01" });
      clip.metadata.set("cmx_3600.reel", "AX");
      clip.metadata.set("shot.take", 3);
      clip.metadata.set("shot.good", true);
      is(clip.metadata.get("cmx_3600.reel"), "AX", "a string");
      is(clip.metadata.get("shot.take"), 3, "a number");
      is(clip.metadata.get("shot.good"), true, "a flag");
      ok(clip.metadata.has("shot.take"), "has() said no");
      clip.metadata.delete("shot.take");
      ok(!clip.metadata.has("shot.take"), "delete() left it there");
    },
  },

  {
    name: "metadata survives a trip through OTIO JSON",
    run(api) {
      const clip = new api.Clip({ name: "shot_01" });
      clip.metadata.set("cmx_3600.reel", "AX");
      const again = api.deserializeJsonFromString(api.serializeJsonToString(clip));
      is(again.metadata.get("cmx_3600.reel"), "AX", "the reel, after JSON");
    },
  },

  {
    name: "a marker and an effect attach to an item",
    run(api) {
      const { Clip, LinearTimeWarp, Marker, RationalTime, TimeRange } = api;
      const clip = new Clip({ name: "shot_01" });
      clip.appendMarker(
        new Marker({
          name: "look here",
          markedRange: new TimeRange(
            new RationalTime(12, 24),
            new RationalTime(1, 24),
          ),
        }),
      );
      clip.appendEffect(new LinearTimeWarp({ name: "half", timeScalar: 0.5 }));

      is(clip.markerCount(), 1, "markers");
      is(clip.markerAt(0).name, "look here", "the marker's name");
      is(clip.markerAt(0).markedRange.startTime.value, 12, "where it is");
      is(clip.effectCount(), 1, "effects");
      is(clip.effectAt(0).timeScalar, 0.5, "how fast");
    },
  },

  {
    name: "a constructor's defaults are upstream's",
    run(api) {
      // Upstream's `LinearTimeWarp()` is a warp that changes nothing, and its
      // `Marker()` an empty range. Leaving the options out has to mean that
      // and not zero.
      is(new api.LinearTimeWarp().timeScalar, 1, "the default time scalar");
      is(new api.Marker().markedRange.duration.value, 0, "the default range");
      is(new api.Track().kind, "Video", "the default track kind");
    },
  },

  {
    name: "an edit operation moves a clip",
    run(api) {
      const { Clip, RationalTime, TimeRange, Track, edit } = api;
      const track = new Track({ name: "V1" });
      for (const name of ["a", "b"]) {
        const clip = new Clip({ name });
        clip.sourceRange = new TimeRange(
          new RationalTime(0, 24),
          new RationalTime(24, 24),
        );
        track.appendChild(clip);
      }
      edit.slip(track.childAt(0), new RationalTime(12, 24));
      const first = track.childAt(0);
      ok(first instanceof Clip, "the first child was not a clip");
      is(
        first.sourceRange?.startTime.value,
        12,
        "where the source starts after a slip",
      );
    },
  },

  {
    name: "an algorithm answers with something new",
    run(api) {
      const { Clip, RationalTime, TimeRange, Track, algorithms } = api;
      const track = new Track({ name: "V1" });
      const clip = new Clip({ name: "shot_01" });
      clip.sourceRange = new TimeRange(
        new RationalTime(0, 24),
        new RationalTime(48, 24),
      );
      track.appendChild(clip);

      const trimmed = algorithms.trackTrimmedToRange(
        track,
        new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24)),
      );
      ok(trimmed instanceof Track, "that was not a track");
      is(trimmed.duration().value, 24, "the trimmed duration");
      is(track.duration().value, 48, "the original was changed");
    },
  },

  {
    name: "a format is named by its suffix",
    run(api) {
      is(api.formatFromSuffix("edl"), "cmx3600", "edl");
      is(api.formatFromSuffix("otio"), "otioJson", "otio");
      is(api.formatFromSuffix("AAF"), "aaf", "AAF");
      is(api.formatFromSuffix("wav"), undefined, "a suffix nothing reads");
    },
  },

  {
    name: "disposing a timeline makes what lived in it throw",
    run(api) {
      const timeline = api.readTimelineFromString("cmx3600", EDL, { rate: 24 });
      const clip = timeline.findClips()[0];
      ok(clip !== undefined, "the EDL held no clips");
      timeline.dispose();
      throws(() => clip.name, "reading a clip whose timeline was disposed");
    },
  },

  {
    name: "a big file reads, which grows the module's memory",
    run(api) {
      // The scratch stack takes a view on the module's memory, and growing it
      // replaces the buffer underneath. Anything holding the old view reads
      // zeroes afterwards, which is the one bug that only shows up on inputs
      // big enough to matter.
      const lines = ["TITLE: long", ""];
      for (let index = 0; index < 500; index += 1) {
        const start = 10 * index;
        const at = (seconds: number) =>
          `${String(Math.floor(seconds / 3600)).padStart(2, "0")}:` +
          `${String(Math.floor(seconds / 60) % 60).padStart(2, "0")}:` +
          `${String(seconds % 60).padStart(2, "0")}:00`;
        lines.push(
          `${String(index + 1).padStart(3, "0")}  AX       V     C        ` +
            `${at(start)} ${at(start + 4)} ${at(start)} ${at(start + 4)}`,
          `* FROM CLIP NAME: shot_${String(index).padStart(4, "0")}`,
        );
      }
      const timeline = api.readTimelineFromString(
        "cmx3600",
        `${lines.join("\n")}\n`,
        { rate: 24 },
      );
      is(timeline.findClips().length, 500, "clips in the long EDL");
      is(timeline.findClips()[499]?.name, "shot_0499", "the last one");
    },
  },

  {
    name: "a list that outgrows the scratch block keeps its call's arguments",
    run(api) {
      // A list call asks twice: once for the count, then again with room for
      // that many. The room is allocated between the passes, after the
      // receiver, the count and the error slot were written, and six hundred
      // handles do not fit in the first block. The second pass still names
      // all three by where they were written, so the block they are in has
      // to outlive the call.
      const { Clip, Track } = api;
      const track = new Track({ name: "V1" });
      for (let index = 0; index < 600; index += 1) {
        track.appendChild(new Clip({ name: `shot_${String(index).padStart(4, "0")}` }));
      }
      outgrowingScratch(() => {
        const children = track.children();
        is(children.length, 600, "children of the track");
        is(children[599]?.name, "shot_0599", "the last one");
      });
    },
  },

  {
    name: "a string that outgrows the scratch block keeps the arguments before it",
    run(api) {
      // The receiver is written first and the name after it, so a name too
      // long for the block moves the call to a bigger one with the receiver
      // left behind in the old.
      const clip = new api.Clip({ name: "short" });
      const long = "x".repeat(10_000);
      outgrowingScratch(() => {
        clip.name = long;
      });
      is(clip.name, long, "the name that was set");
    },
  },

  // What every SDK has to agree on, rendered from the conformance scenarios
  // into `conformance.ts`. Spread here once, whole, so both runners run every
  // scenario the generator wrote and none has to be listed by hand.
  ...conformance,
];

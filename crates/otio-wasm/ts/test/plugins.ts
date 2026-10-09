/**
 * Media linkers and hook scripts written in TypeScript.
 *
 * The registry is the module's, shared by everything that loaded it, so
 * every case here registers under names of its own and unregisters them in
 * a `finally`. Run by both runners, through `suite.ts`.
 */

import type * as otio from "../src/index.js";

import { pluginTable } from "../src/runtime.js";
import type { Case, Otio } from "./suite.js";

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

/** Runs `body`, which must throw a plugin error mentioning `words`. */
function pluginError(api: Otio, body: () => unknown, words: string, what: string): void {
  try {
    body();
  } catch (thrown) {
    ok(thrown instanceof api.OtioError, `${what}: threw ${String(thrown)}, not an OtioError`);
    is(thrown.status, "pluginError", `${what}: the status`);
    ok(thrown.message.includes(words), `${what}: the message was ${JSON.stringify(thrown.message)}`);
    return;
  }
  throw new Error(`${what}: nothing was thrown`);
}

/** A two-clip timeline as OTIO JSON, each clip on media under file:///media. */
function cut(api: Otio): string {
  const { Clip, ExternalReference, Stack, Timeline, Track } = api;
  const timeline = new Timeline({ name: "Cut" });
  const stack = new Stack({ name: "tracks" });
  const track = new Track({ name: "V1", kind: "Video" });
  timeline.tracks = stack;
  stack.appendChild(track);
  for (const name of ["first", "second"]) {
    const clip = new Clip({ name });
    clip.setMediaReference(
      "DEFAULT_MEDIA",
      new ExternalReference({ name, targetUrl: `file:///media/${name}.mov` }),
    );
    clip.activeMediaReferenceKey = "DEFAULT_MEDIA";
    track.appendChild(clip);
  }
  return api.writeToString("otioJson", timeline);
}

/** The target URL of the first clip's active media. */
function firstUrl(api: Otio, root: otio.Node): string {
  const clip = root.findClips()[0];
  ok(clip !== undefined, "the timeline has no clips");
  const media = clip.mediaReference();
  ok(media instanceof api.ExternalReference, "the clip's media is not an external reference");
  return media.targetUrl;
}

/** A hook script that writes who ran it into what it is handed, under `key`. */
function stamp(key: string): otio.HookScript {
  return (target, args) => {
    target.metadata.set(key, typeof args.who === "string" ? args.who : "nobody");
    return target;
  };
}

export const pluginCases: readonly Case[] = [
  {
    name: "a media linker written in TypeScript links every clip",
    run(api) {
      const written = cut(api);
      let seen = 0;
      api.registerMediaLinker("ts_proxies", (clip, args) => {
        seen += 1;
        ok(typeof args.root === "string", "no root to link under");
        return new api.ExternalReference({
          name: "proxy",
          targetUrl: `${args.root}/${clip.name}.mov`,
        });
      });
      try {
        const options = {
          mediaLinker: "ts_proxies",
          mediaLinkerArguments: JSON.stringify({ root: "/proxies" }),
        };
        const root = api.readFromString("otioJson", written, options);
        is(seen, 2, "clips the linker saw");
        is(firstUrl(api, root), "/proxies/first.mov", "the first clip's media");
        is(root.findClips()[1]?.mediaReference()?.name, "proxy", "the second clip's media");

        // Asked not to link, it does not.
        const unlinked = api.readFromString("otioJson", written, {
          ...options,
          doNotLinkMedia: true,
        });
        is(seen, 2, "clips the linker saw when told not to link");
        is(firstUrl(api, unlinked), "file:///media/first.mov", "the unlinked clip's media");
      } finally {
        api.unregisterMediaLinker("ts_proxies");
      }
    },
  },

  {
    name: "a linker that answers nothing leaves the clip's media alone",
    run(api) {
      const written = cut(api);
      let seen = 0;
      api.registerMediaLinker("ts_watcher", () => {
        seen += 1;
      });
      try {
        const root = api.readFromString("otioJson", written, { mediaLinker: "ts_watcher" });
        is(seen, 2, "clips the linker saw");
        is(firstUrl(api, root), "file:///media/first.mov", "the first clip's media");
      } finally {
        api.unregisterMediaLinker("ts_watcher");
      }
    },
  },

  {
    name: "a linker that throws stops the read in its own words",
    run(api) {
      const written = cut(api);
      api.registerMediaLinker("ts_offline", () => {
        throw new Error("the proxies are offline");
      });
      try {
        pluginError(
          api,
          () => api.readFromString("otioJson", written, { mediaLinker: "ts_offline" }),
          "the proxies are offline",
          "a linker that throws",
        );

        // Throwing something that is not an Error is a failure too.
        api.registerMediaLinker("ts_offline", () => {
          // eslint-disable-next-line @typescript-eslint/only-throw-error
          throw "no disk";
        });
        pluginError(
          api,
          () => api.readFromString("otioJson", written, { mediaLinker: "ts_offline" }),
          "no disk",
          "a linker that throws a string",
        );

        // So is an answer that is not a media reference, or a promise of one.
        api.registerMediaLinker("ts_offline", (clip) => clip as unknown as otio.MediaReference);
        pluginError(
          api,
          () => api.readFromString("otioJson", written, { mediaLinker: "ts_offline" }),
          "media reference",
          "a linker that answers with a clip",
        );
        api.registerMediaLinker(
          "ts_offline",
          (async () => undefined) as unknown as otio.MediaLinker,
        );
        pluginError(
          api,
          () => api.readFromString("otioJson", written, { mediaLinker: "ts_offline" }),
          "async",
          "an async linker",
        );

        // A long message is cut to the room the library gives, not refused.
        api.registerMediaLinker("ts_offline", () => {
          throw new Error("é".repeat(5_000));
        });
        pluginError(
          api,
          () => api.readFromString("otioJson", written, { mediaLinker: "ts_offline" }),
          "éé",
          "a linker with a lot to say",
        );
      } finally {
        api.unregisterMediaLinker("ts_offline");
      }

      // And a linker nobody registered is refused, as upstream refuses one.
      pluginError(
        api,
        () => api.readFromString("otioJson", written, { mediaLinker: "ts_nowhere" }),
        "ts_nowhere",
        "an unknown linker",
      );
    },
  },

  {
    name: "hook scripts written in TypeScript run around reads and writes",
    run(api) {
      const written = cut(api);
      api.registerHookScript("ts_stamp_read", stamp("read_by"));
      api.attachHookScript("post_adapter_read", "ts_stamp_read");
      api.registerHookScript("ts_stamp_write", stamp("written_by"));
      api.attachHookScript("pre_adapter_write", "ts_stamp_write");
      try {
        const root = api.readFromString("otioJson", written, {
          hookArguments: JSON.stringify({ who: "the TypeScript test" }),
        });
        is(root.metadata.get("read_by"), "the TypeScript test", "what the read hook left");

        // A write runs its hooks on a copy, so the timeline is left alone.
        const out = api.writeToString("otioJson", root, {
          hookArguments: JSON.stringify({ who: "the writer" }),
        });
        ok(out.includes('"written_by": "the writer"'), "the write hook did not reach the file");
        ok(!root.metadata.has("written_by"), "the write hook changed the timeline");
      } finally {
        api.detachHookScript("post_adapter_read", "ts_stamp_read");
        api.unregisterHookScript("ts_stamp_read");
        api.detachHookScript("pre_adapter_write", "ts_stamp_write");
        api.unregisterHookScript("ts_stamp_write");
      }
    },
  },

  {
    name: "a hook of your own runs when asked",
    run(api) {
      const clip = new api.Clip({ name: "A" });
      api.registerHookScript("ts_stamp", stamp("stamped_by"));
      // A script may answer with a different object to go on with.
      api.registerHookScript("ts_replace", () => new api.Clip({ name: "replacement" }));
      // One that answers nothing fails: a hook needs an object to go on with.
      api.registerHookScript("ts_nothing", (() => undefined) as unknown as otio.HookScript);
      // One that tries to free the timeline it was lent is refused.
      api.registerHookScript("ts_dispose", (target) => {
        clip.dispose();
        return target;
      });
      api.attachHookScript("ts_mine", "ts_stamp");
      api.attachHookScript("ts_swap", "ts_replace");
      api.attachHookScript("ts_empty", "ts_nothing");
      api.attachHookScript("ts_free", "ts_dispose");
      try {
        const result = clip.runHook("ts_mine", JSON.stringify({ who: "me" }));
        ok(result.equals(clip), "the hook answered with something other than the clip");
        is(clip.metadata.get("stamped_by"), "me", "what the hook left");

        const swapped = clip.runHook("ts_swap");
        is(swapped.name, "replacement", "what the swapping hook answered");
        ok(swapped.equals(swapped), "the replacement is usable after the hook");

        pluginError(api, () => clip.runHook("ts_empty"), "", "a script that answers nothing");
        pluginError(api, () => clip.runHook("ts_free"), "disposed", "a script that disposes");
        is(clip.name, "A", "the clip after a script tried to dispose of it");
        pluginError(api, () => clip.runHook("ts_undeclared"), "", "an undeclared hook");
      } finally {
        api.detachHookScript("ts_mine", "ts_stamp");
        api.detachHookScript("ts_swap", "ts_replace");
        api.detachHookScript("ts_empty", "ts_nothing");
        api.detachHookScript("ts_free", "ts_dispose");
        for (const name of ["ts_stamp", "ts_replace", "ts_nothing", "ts_dispose"]) {
          api.unregisterHookScript(name);
        }
      }
    },
  },

  {
    name: "what a plugin is lent cannot be kept or moved away",
    run(api) {
      // A bare track, so what the read hook is handed has no parent and the
      // only thing stopping a move is that it is on loan.
      const track = new api.Track({ name: "V1" });
      track.appendChild(new api.Clip({ name: "A" }));
      const written = api.serializeJsonToString(track);
      const elsewhere = new api.Stack({ name: "elsewhere" });
      let kept: otio.Node | undefined;
      api.registerHookScript("ts_keeper", (target) => {
        kept = target;
        elsewhere.appendChild(target as otio.Track);
        return target;
      });
      api.attachHookScript("post_adapter_read", "ts_keeper");
      try {
        pluginError(
          api,
          () => api.readFromString("otioJson", written),
          "cannot be moved",
          "a hook that moves what it was lent away",
        );
        ok(kept !== undefined, "the hook never ran");
        let threw = false;
        try {
          void kept.name;
        } catch {
          threw = true;
        }
        ok(threw, "an object kept past the call still answered");
        is(elsewhere.childCount(), 0, "children the other stack gained");
      } finally {
        api.detachHookScript("post_adapter_read", "ts_keeper");
        api.unregisterHookScript("ts_keeper");
      }
    },
  },

  {
    name: "unregistering says whether there was anything, and lets the function go",
    run(api) {
      const before = pluginTable.size();
      api.registerHookScript("ts_brief", stamp("x"));
      is(pluginTable.size(), before + 1, "functions held after registering");
      api.registerHookScript("ts_brief", stamp("y"));
      is(pluginTable.size(), before + 1, "functions held after registering again");
      ok(api.unregisterHookScript("ts_brief"), "the first unregister found nothing");
      ok(!api.unregisterHookScript("ts_brief"), "the second unregister found something");
      is(pluginTable.size(), before, "functions held after unregistering");

      let thrown: unknown;
      try {
        api.registerMediaLinker("", () => undefined);
      } catch (error) {
        thrown = error;
      }
      ok(thrown instanceof api.OtioError, "a linker with no name was accepted");
      is(pluginTable.size(), before, "functions held after a refused registration");
      let refused = false;
      try {
        api.registerMediaLinker("ts_null", null as unknown as otio.MediaLinker);
      } catch {
        refused = true;
      }
      ok(refused, "a linker that is not a function was accepted");
    },
  },
];

/**
 * Media linkers and hook scripts written in TypeScript.
 *
 * Upstream OpenTimelineIO has two plugin points. A *media linker* is handed
 * each clip a read produced and answers the media reference the clip should
 * use instead: upstream's `link_media_reference(in_clip, media_linker_argument_map)`.
 * A *hook script* is handed what a read produced, or what is about to be
 * written, and answers what to go on with: upstream's
 * `hook_function(in_timeline, argument_map)`. Upstream finds both through
 * plugin manifests; here they are functions, registered under a name.
 *
 * ```ts
 * registerMediaLinker("proxies", (clip, args) =>
 *   new ExternalReference({ targetUrl: `${args.root}/${clip.name}.mov` }));
 * const track = readFromString("otio_json", text, {
 *   mediaLinker: "proxies",
 *   mediaLinkerArguments: JSON.stringify({ root: "/proxies" }),
 * });
 * ```
 *
 * # How a call gets here
 *
 * The C ABI registers a plugin as a function pointer and a context, and a
 * WebAssembly module cannot be handed a JavaScript function. So every plugin
 * registered from here is the same function inside the module, which calls
 * the module's one import, `otio_js_plugin`, with the context it was
 * registered with; the runtime looks the context up and lands here. When the
 * module lets go of a plugin, because its name was registered again or
 * unregistered, it says so through `otio_js_release` and the runtime forgets
 * the function.
 *
 * The document the plugin works in is the library's, lent for the call: the
 * objects handed in throw once the call is over, and nothing here frees it.
 */

import { MediaReference } from "./generated/api.js";
import type { Clip, Node } from "./generated/api.js";
import { metadataOf, type MetadataValue } from "./metadata.js";
import { Doc } from "./objects.js";
import {
  NONE,
  OtioPanic,
  check,
  exports,
  openStack,
  pluginTable,
  poison,
  writeMessage,
  type NodeHandle,
  type PluginCall,
} from "./runtime.js";

/**
 * What a plugin is handed as its arguments: the JSON object the read or
 * write was given for it, as a plain object. Upstream's `argument_map`.
 */
export type PluginArguments = Readonly<Record<string, MetadataValue>>;

/**
 * A media linker: handed each clip a read produced, and the arguments the
 * read was given for it, it answers the media reference the clip should use
 * in place of its active one.
 *
 * It may build the reference fresh, with `new ExternalReference(...)` or any
 * other constructor, or edit the clip itself. Answering nothing leaves the
 * clip as it is. Throwing stops the read, which fails with an `OtioError`
 * whose status is `"pluginError"` and whose message is the one thrown.
 *
 * The clip, and anything else reached through it, is valid only for the
 * call: it belongs to the read. What it answers joins the clip's timeline.
 */
export type MediaLinker = (
  clip: Clip,
  args: PluginArguments,
) => MediaReference | null | undefined | void;

/**
 * A hook script: handed what a hook runs on, and the arguments the read or
 * write was given for its hooks, it answers what to go on with: the same
 * object, changed or not, or another.
 *
 * What it is handed is valid only for the call; what it answers joins the
 * timeline it was handed. Answering nothing, or throwing, fails the read,
 * write or `runHook` with status `"pluginError"`.
 */
export type HookScript = (target: Node, args: PluginArguments) => Node;

/**
 * Registers a media linker under `name`, replacing any registered already.
 *
 * A read runs it on every clip when its options' `mediaLinker` names it,
 * unless they also say `doNotLinkMedia`. `unregisterMediaLinker` lets it go.
 */
export function registerMediaLinker(name: string, linker: MediaLinker): void {
  if (typeof linker !== "function") {
    throw new TypeError("a media linker is a function");
  }
  register(name, call(linker, true), true);
}

/**
 * Registers a hook script under `name`, replacing any registered already.
 *
 * It runs at the hooks `attachHookScript` attaches it to: the four every
 * read and write runs, `post_adapter_read`, `post_media_linker`,
 * `pre_adapter_write` and `post_adapter_write`, or one of your own that
 * `Node#runHook` runs. `unregisterHookScript` lets it go.
 */
export function registerHookScript(name: string, script: HookScript): void {
  if (typeof script !== "function") {
    throw new TypeError("a hook script is a function");
  }
  register(name, call(script, false), false);
}

/** Hands the module a plugin, under a context the runtime will know it by. */
function register(name: string, plugin: PluginCall, linker: boolean): void {
  const module = exports();
  const context = pluginTable.add(plugin);
  const stack = openStack();
  try {
    const at = stack.text(name);
    const error = stack.alloc(8, 4); /* OtioBuffer */
    const status = linker
      ? module.otio_wasm_register_media_linker(at, context, error)
      : module.otio_wasm_register_hook_script(at, context, error);
    if (status !== 0) {
      // A registration that fails keeps nothing, so nothing will release it.
      pluginTable.remove(context);
    }
    check(status, error);
  } finally {
    stack.close();
  }
}

/** The status a plugin's failure is reported with. */
const PLUGIN_ERROR = 12;

/**
 * What the runtime calls for one plugin: lends the document, hands the
 * function its target and arguments, and writes back what it answered.
 */
function call(
  plugin: (target: never, args: PluginArguments) => unknown,
  linker: boolean,
): PluginCall {
  return (document, target, args, outResult, message, capacity) => {
    const lent = Doc.borrow(document);
    try {
      const argumentMap = metadataOf(lent.wrap(args)).toObject();
      const answer = plugin(lent.wrap(target) as never, argumentMap);
      if (isThenable(answer)) {
        throw new TypeError(
          "a plugin runs inside the read or write that calls it, so it cannot be async",
        );
      }
      let handle: NodeHandle = NONE;
      if (answer !== undefined && answer !== null) {
        if (linker && !(answer instanceof MediaReference)) {
          throw new TypeError("a media linker answers with a media reference, or with nothing");
        }
        // Built fresh, it lives in a document of its own until now.
        handle = lent.adopt(answer as Node);
      }
      const view = new DataView(exports().memory.buffer);
      view.setUint32(outResult, handle.index, true);
      view.setUint32(outResult + 4, handle.generation, true);
      return 0;
    } catch (thrown) {
      // A trap leaves the module in pieces; that is not the plugin's failure
      // to report, and nothing can be called to report it.
      if (thrown instanceof OtioPanic) {
        throw thrown;
      }
      if (thrown instanceof WebAssembly.RuntimeError) {
        throw poison(thrown);
      }
      writeMessage(message, capacity, describe(thrown));
      return PLUGIN_ERROR;
    } finally {
      lent.revoke();
    }
  };
}

/** The sentence a thrown value is reported with. */
function describe(thrown: unknown): string {
  const text = thrown instanceof Error ? thrown.message : String(thrown);
  return text === "" ? "it threw without saying why" : text;
}

/** Whether a value is a promise, or anything awaitable like one. */
function isThenable(value: unknown): boolean {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { then?: unknown }).then === "function"
  );
}

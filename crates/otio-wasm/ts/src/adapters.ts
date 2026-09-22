/**
 * Reading and writing the interchange formats.
 *
 * Upstream OpenTimelineIO calls this `otio.adapters`, and the names here are
 * its names. What is missing is the half of it that touches a filesystem:
 * there is none behind a WebAssembly module, so these take and return bytes
 * and leave getting hold of the bytes to the host. In a browser that is a
 * `fetch` or a file input; in Node it is `fs`. Either way it is a line of
 * code, and it is the host's line rather than ours.
 */

import * as raw from "./generated/raw.js";
import { Timeline } from "./generated/api.js";
import type { Node } from "./generated/api.js";
import type { Format, ReadOptions, WriteOptions } from "./generated/types.js";
import { Doc, place } from "./objects.js";
import { guard } from "./runtime.js";

/** What the C ABI is given when a caller says nothing. */
const READ_DEFAULTS: ReadOptions = {
  rate: 0,
  ignoreTimecodeMismatch: false,
  aafKeepNesting: false,
  aafMarkersOnSlots: false,
  aafBakeKeyframes: false,
};

/**
 * The same, for writing.
 *
 * `aafTime` and `aafIdSeed` are missing on purpose. The module has no clock
 * and no randomness of its own, so where the library would read the system
 * clock and draw fresh identifiers, this side does it for the module: see
 * {@link writeDefaults}.
 */
const WRITE_DEFAULTS: Omit<WriteOptions, "aafTime" | "aafIdSeed"> = {
  rate: 0,
  edlStyle: "avid",
  reelnameLen: 0,
  aafPreferFileMobId: false,
  aafUseEmptyMobIds: false,
  aafEmbedEssence: false,
  aafCreateEdgecode: false,
};

/**
 * The defaults for one write: the time now and a fresh seed, which is what
 * a native build of the library does for itself when both are zero.
 */
function writeDefaults(): WriteOptions {
  const seed = new Uint32Array(2);
  crypto.getRandomValues(seed);
  return {
    ...WRITE_DEFAULTS,
    aafTime: Math.floor(Date.now() / 1000),
    // 53 bits, the most a number holds exactly, and never zero, which
    // would ask the module for randomness it does not have.
    aafIdSeed: (seed[0]! & 0x1fffff) * 2 ** 32 + seed[1]! || 1,
  };
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Reads a timeline from the bytes of a file.
 *
 * ```ts
 * const bytes = new Uint8Array(await (await fetch("cut.edl")).arrayBuffer());
 * const timeline = readFromBytes("cmx3600", bytes, { rate: 24 });
 * ```
 *
 * The options a format does not use are ignored, so one set of them can be
 * filled in once and used for several.
 */
export function readFromBytes(
  format: Format,
  data: Uint8Array,
  options: Partial<ReadOptions> = {},
): Node {
  return guard(() => {
    const pointer = raw.readFromBytes(format, data, {
      ...READ_DEFAULTS,
      ...options,
    });
    return rootOf(Doc.take(pointer));
  });
}

/** Reads a timeline from the text of a file, for the formats that are text. */
export function readFromString(
  format: Format,
  text: string,
  options: Partial<ReadOptions> = {},
): Node {
  return readFromBytes(format, encoder.encode(text), options);
}

/**
 * Writes an object out in one of the interchange formats.
 *
 * What is written is the object given and everything under it, so passing a
 * timeline writes the timeline and passing a track writes the track.
 *
 * ```ts
 * const aaf = writeToBytes("aaf", timeline, { aafUseEmptyMobIds: true });
 * ```
 *
 * An AAF records who made each new marker, and there is no login to ask
 * here, so a timeline with markers that name no user of their own needs
 * `aafUser`.
 */
export function writeToBytes(
  format: Format,
  root: Node,
  options: Partial<WriteOptions> = {},
): Uint8Array {
  return guard(() => {
    const at = place(root);
    // The C ABI writes a document, starting from its root. An object read out
    // of a file is already that root; one built here is not, so it is made so.
    raw.documentSetRoot(at.document, at.handle);
    return raw.writeToBytes(at.document, format, {
      ...writeDefaults(),
      ...options,
    });
  });
}

/** The same, as text, for the formats that are text. */
export function writeToString(
  format: Format,
  root: Node,
  options: Partial<WriteOptions> = {},
): string {
  return decoder.decode(writeToBytes(format, root, options));
}

/**
 * Writes an object out as OTIO JSON.
 *
 * Upstream's name, and upstream's behaviour: it starts wherever it is pointed,
 * so a bare clip serialises as happily as a whole timeline.
 */
export function serializeJsonToString(node: Node, indent = raw.defaultIndent()): string {
  return guard(() => {
    const at = place(node);
    return raw.nodeToJson(at.document, at.handle, indent);
  });
}

/** Reads an object back from OTIO JSON. */
export function deserializeJsonFromString(json: string): Node {
  return guard(() => rootOf(Doc.take(raw.documentFromJson(json))));
}

/**
 * Reads a timeline from the bytes of a file, and says so if it is not one.
 *
 * `readFromBytes` answers with whatever the file held, because an ALE holds a
 * collection and a bare `.otio` may hold a single clip. Most files hold a
 * timeline and most callers want one, and this is that call: the same read,
 * with the check written once here rather than at every call site.
 */
export function readTimelineFromBytes(
  format: Format,
  data: Uint8Array,
  options: Partial<ReadOptions> = {},
): Timeline {
  return asTimeline(readFromBytes(format, data, options));
}

/** The same, from the text of a file. */
export function readTimelineFromString(
  format: Format,
  text: string,
  options: Partial<ReadOptions> = {},
): Timeline {
  return asTimeline(readFromString(format, text, options));
}

/** Insists that what was read is a timeline. */
function asTimeline(root: Node): Timeline {
  if (root instanceof Timeline) {
    return root;
  }
  throw new TypeError(
    `that file holds a ${root.schemaName()} rather than a timeline`,
  );
}

/** The root of a freshly read document, which every reader produces. */
function rootOf(doc: Doc): Node {
  const root = raw.documentRoot(doc.pointer);
  if (root === undefined) {
    throw new Error("that file held nothing");
  }
  return doc.wrap(root);
}

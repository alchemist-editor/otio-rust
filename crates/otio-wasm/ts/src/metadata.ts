/**
 * The free-form metadata an OTIO object carries.
 *
 * Every object has a dictionary hanging off it that adapters use to keep
 * whatever the format they read said and OTIO has no field for: an EDL's reel
 * name, an AAF's mob ID, a house convention nobody else has heard of. It is a
 * tree of dictionaries, arrays and values, and it round-trips whether or not
 * anything understands it.
 *
 * The C ABI reads and writes it with forty typed calls, one per kind, because
 * C has to be told what it is fetching. TypeScript does not: `get` asks what
 * is at a path and hands back a JavaScript value of the matching type, and
 * `set` looks at what it was given. That is why this file is written rather
 * than generated — the shape TypeScript wants is one call where C needs
 * twenty, and no rule about C signatures would have produced it.
 */

import * as raw from "./generated/raw.js";
import type { Node } from "./generated/api.js";
import type { Box2d, Color, V2d } from "./generated/types.js";
import { RationalTime, TimeRange, TimeTransform } from "./generated/values.js";
import { place, type Doc } from "./objects.js";
import type { NodeHandle } from "./runtime.js";

/** Anything metadata can hold. */
export type MetadataValue =
  | null
  | boolean
  | number
  | string
  | RationalTime
  | TimeRange
  | TimeTransform
  | MetadataColor
  | V2d
  | Box2d
  | Node
  | readonly MetadataValue[]
  | { readonly [key: string]: MetadataValue };

/**
 * An object's metadata.
 *
 * Reached as `clip.metadata`, and addressed by path: `"cmx_3600.reel"` reaches
 * into a nested dictionary and `"comments[0]"` into an array, and the two mix
 * freely. An empty path names the whole dictionary.
 *
 * ```ts
 * clip.metadata.set("cmx_3600.reel", "TAPE01");
 * clip.metadata.get("cmx_3600.reel"); // "TAPE01"
 * clip.metadata.keys("cmx_3600");     // ["reel"]
 * ```
 */
export class Metadata {
  readonly #owner: Node;

  /** @internal */
  constructor(owner: Node) {
    this.#owner = owner;
  }

  /** Where the object this belongs to lives. */
  get #at(): { doc: Doc; document: number; handle: NodeHandle } {
    return place(this.#owner);
  }

  /** What kind of value sits at a path, or `undefined` for nothing. */
  kindOf(path = ""): ReturnType<typeof raw.metadataKind> {
    const at = this.#at;
    return raw.metadataKind(at.document, at.handle, path);
  }

  /** Whether anything sits at a path. */
  has(path: string): boolean {
    const at = this.#at;
    return raw.metadataContains(at.document, at.handle, path);
  }

  /** How many entries a dictionary or an array at a path holds. */
  size(path = ""): number {
    const at = this.#at;
    return raw.metadataLen(at.document, at.handle, path) ?? 0;
  }

  /** The keys of a dictionary at a path, in the order it keeps them. */
  keys(path = ""): string[] {
    const at = this.#at;
    const length = raw.metadataLen(at.document, at.handle, path) ?? 0;
    const found: string[] = [];
    for (let index = 0; index < length; index += 1) {
      const key = raw.metadataKeyAt(at.document, at.handle, path, index);
      if (key !== undefined) {
        found.push(key);
      }
    }
    return found;
  }

  /**
   * The value at a path, as the JavaScript type that matches it.
   *
   * Answers `undefined` when there is nothing there, which is a different
   * thing from a stored `null`.
   */
  get(path = ""): MetadataValue | undefined {
    const at = this.#at;
    const kind = raw.metadataKind(at.document, at.handle, path);
    if (kind === undefined) {
      return undefined;
    }
    switch (kind) {
      case "null":
        return null;
      case "bool":
        return raw.metadataGetBool(at.document, at.handle, path);
      case "int":
        return raw.metadataGetInt(at.document, at.handle, path);
      case "uint":
        return raw.metadataGetUint(at.document, at.handle, path);
      case "double":
        return raw.metadataGetDouble(at.document, at.handle, path);
      case "string":
        return raw.metadataGetString(at.document, at.handle, path);
      case "rationalTime":
        return raw.metadataGetRationalTime(at.document, at.handle, path);
      case "timeRange":
        return raw.metadataGetTimeRange(at.document, at.handle, path);
      case "timeTransform":
        return raw.metadataGetTimeTransform(at.document, at.handle, path);
      case "color": {
        const found = raw.metadataGetColor(at.document, at.handle, path);
        return found.name === "" ? found.value : { ...found.value, name: found.name };
      }
      case "v2d":
        return raw.metadataGetV2d(at.document, at.handle, path);
      case "box2d":
        return raw.metadataGetBox2d(at.document, at.handle, path);
      case "object":
        return at.doc.wrap(raw.metadataGetObject(at.document, at.handle, path));
      case "vector": {
        const length = raw.metadataLen(at.document, at.handle, path) ?? 0;
        const found: MetadataValue[] = [];
        for (let index = 0; index < length; index += 1) {
          found.push(this.get(`${path}[${index}]`) ?? null);
        }
        return found;
      }
      case "dictionary": {
        const found: Record<string, MetadataValue> = {};
        for (const key of this.keys(path)) {
          found[key] = this.get(join(path, key)) ?? null;
        }
        return found;
      }
      default:
        // A kind the core has grown since this SDK was built. The honest
        // answer is that there is something there and we cannot read it.
        throw new Error(
          `the value at ${JSON.stringify(path)} is of a kind this version does not know`,
        );
    }
  }

  /**
   * Writes a value at a path.
   *
   * A dictionary the path passes through is made if it is not there, so
   * `set("cmx_3600.reel", "AX")` works on a clip that has no metadata yet.
   * Setting a dictionary or an array builds the container and fills it, so a
   * whole nest of metadata goes in with one call either way.
   *
   * An array is the exception: its length is fixed when it is made, so a path
   * through `[n]` reaches into an array that already exists rather than
   * growing one.
   */
  set(path: string, value: MetadataValue): void {
    this.#makeParents(path);
    const at = this.#at;
    const { document, handle } = at;

    if (value === null) {
      raw.metadataSetNull(document, handle, path);
      return;
    }
    switch (typeof value) {
      case "boolean":
        raw.metadataSetBool(document, handle, path, value);
        return;
      case "string":
        raw.metadataSetString(document, handle, path, value);
        return;
      case "number":
        // An integer written as a double comes back as a double, which would
        // turn a frame count into `24.0` on the next round trip.
        if (Number.isSafeInteger(value)) {
          raw.metadataSetInt(document, handle, path, value);
        } else {
          raw.metadataSetDouble(document, handle, path, value);
        }
        return;
      default:
        break;
    }
    if (value instanceof RationalTime) {
      raw.metadataSetRationalTime(document, handle, path, value);
      return;
    }
    if (value instanceof TimeRange) {
      raw.metadataSetTimeRange(document, handle, path, value);
      return;
    }
    if (value instanceof TimeTransform) {
      raw.metadataSetTimeTransform(document, handle, path, value);
      return;
    }
    if (Array.isArray(value)) {
      raw.metadataSetVector(document, handle, path, value.length);
      value.forEach((each, index) => {
        this.set(`${path}[${index}]`, each);
      });
      return;
    }
    if (isNode(value)) {
      raw.metadataSetObject(document, handle, path, at.doc.adopt(value));
      return;
    }
    if (isColor(value)) {
      raw.metadataSetColor(document, handle, path, value, value.name);
      return;
    }
    if (isBox2d(value)) {
      raw.metadataSetBox2d(document, handle, path, value);
      return;
    }
    if (isV2d(value)) {
      raw.metadataSetV2d(document, handle, path, value);
      return;
    }

    raw.metadataSetDictionary(document, handle, path);
    for (const [key, each] of Object.entries(value)) {
      this.set(join(path, key), each);
    }
  }

  /**
   * Makes the dictionaries a path passes through, where they are missing.
   *
   * Only the dotted parts: a `[n]` names a place in an array, which has its
   * length already, so nothing here can conjure one.
   */
  #makeParents(path: string): void {
    const parts = path.split(".");
    if (parts.length < 2) {
      return;
    }
    const { document, handle } = this.#at;
    let prefix = "";
    for (const part of parts.slice(0, -1)) {
      prefix = join(prefix, part);
      if (prefix.includes("[")) {
        return;
      }
      if (raw.metadataKind(document, handle, prefix) === undefined) {
        raw.metadataSetDictionary(document, handle, prefix);
      }
    }
  }

  /** Removes whatever is at a path, and says whether anything was there. */
  delete(path: string): boolean {
    const at = this.#at;
    return raw.metadataRemove(at.document, at.handle, path) !== undefined;
  }

  /** Empties the whole dictionary. */
  clear(): void {
    const at = this.#at;
    raw.metadataClear(at.document, at.handle);
  }

  /** The whole dictionary, as a plain JavaScript object. */
  toObject(): Record<string, MetadataValue> {
    return (this.get("") ?? {}) as Record<string, MetadataValue>;
  }

  /** The entries of the top-level dictionary. */
  *[Symbol.iterator](): Iterator<[string, MetadataValue]> {
    for (const key of this.keys()) {
      yield [key, this.get(key) ?? null];
    }
  }
}

/** Joins a path and a key, which at the root is just the key. */
function join(path: string, key: string): string {
  return path === "" ? key : `${path}.${key}`;
}

/** Whether a value is one of the object model's objects. */
function isNode(value: object): value is Node {
  // Everything the SDK hands out is bound; a plain object is not.
  try {
    place(value);
    return true;
  } catch {
    return false;
  }
}

/**
 * A colour in metadata, which may have a name of its own.
 *
 * Upstream stores a colour alongside the name it was given, where it was
 * given one, and keeps both when the file is written back out.
 */
export interface MetadataColor extends Color {
  /** What the colour is called, for one that was named. */
  readonly name?: string;
}

/** Whether a plain object is a colour. */
function isColor(value: object): value is MetadataColor {
  return "r" in value && "g" in value && "b" in value && "a" in value;
}

/** Whether a plain object is a point. */
function isV2d(value: object): value is V2d {
  return "x" in value && "y" in value;
}

/** Whether a plain object is a rectangle. */
function isBox2d(value: object): value is Box2d {
  return "min" in value && "max" in value;
}

/** The metadata of an object, made once and remembered. */
const metadata = new WeakMap<Node, Metadata>();

/**
 * The metadata of an object.
 *
 * Remembered per object so that `clip.metadata === clip.metadata`, the way a
 * field would be.
 *
 * @internal
 */
export function metadataOf(owner: Node): Metadata {
  const held = metadata.get(owner);
  if (held !== undefined) {
    return held;
  }
  const made = new Metadata(owner);
  metadata.set(owner, made);
  return made;
}

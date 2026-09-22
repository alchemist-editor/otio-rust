/**
 * How a JavaScript object refers to an object in a document, and what happens
 * when it moves.
 *
 * # The problem
 *
 * The core keeps every object in a document's arena and names it with a
 * handle, which is an index and a generation rather than a pointer. That is
 * what ADR 0001 chose and it is why nothing here can dangle: an object that
 * has been removed fails a lookup instead of reaching whatever took its slot.
 *
 * Upstream OpenTimelineIO's API has no document in it. You write
 * `new Clip({ name: "a" })`, and put it inside a track later. Those two facts
 * have to be reconciled somewhere, and this is where.
 *
 * # What this does about it
 *
 * Each object built on its own gets a document of its own, holding just that
 * object and whatever hangs off it. Putting one inside another moves it into
 * the other's document, because a handle means nothing outside the arena it
 * came from, and the move leaves behind a forwarding note: the emptied
 * document records where its contents went and the map from old handles to
 * new. Every wrapper already handed out keeps working, because each one
 * follows that chain before it touches anything.
 *
 * The alternative was to find every wrapper and update its handle. That works
 * for the object being appended and fails for everything under it — a clip's
 * media reference, a marker someone happened to be holding. Forwarding costs a
 * lookup per call and cannot go stale.
 *
 * # Who frees the memory
 *
 * JavaScript has no destructors, so nothing here can free a document at the
 * moment the last reference to it goes away. What it has is a
 * `FinalizationRegistry`, which says afterwards. A document is registered when
 * it is made and released when the collector gets round to it, which is late
 * but correct, and the only reference into the module's memory is the one this
 * file holds.
 *
 * For code that would rather not wait, `Timeline#dispose` releases a document
 * and everything in it on the spot. After that, using anything that lived in
 * it throws rather than reading freed memory.
 */

import * as raw from "./generated/raw.js";
import type { Node } from "./generated/api.js";
import type { NodeKind } from "./generated/types.js";
import {
  OtioError,
  OtherTimelineError,
  check,
  exports,
  isNone,
  openStack,
  type NodeHandle,
} from "./runtime.js";

/** A class in the object model, however its constructor is declared. */
type Wrapper = abstract new (...arguments_: never[]) => Node;

/** Which class wraps each kind of object, filled in by `api.ts`. */
let classes: Record<NodeKind, Wrapper> | undefined;

/**
 * Records which class wraps each kind of object.
 *
 * Called once, by the generated class layer, after the classes exist.
 *
 * @internal
 */
export function register(table: Record<NodeKind, Wrapper>): void {
  classes = table;
}

/** The classes whose own constructor builds an object. */
const building = new Set<Wrapper>();

/**
 * Records the classes that build an object of their own.
 *
 * Called once, by the generated class layer.
 *
 * @internal
 */
export function builders(classes_: readonly Wrapper[]): void {
  for (const each of classes_) {
    building.add(each);
  }
}

/**
 * Whether a class below `self` is going to build the object itself.
 *
 * `new Clip()` runs `Item`'s constructor before `Clip`'s, and only one of
 * them should make an object: the most derived one. Each generated
 * constructor asks this first and does nothing if the answer is yes, so a
 * clip is one clip rather than an item that is then thrown away.
 *
 * The walk stops at `self` rather than testing `new.target === self`, so
 * that a class someone writes themselves on top of `Clip` still gets a clip
 * built for it: nothing between it and `Clip` builds, so `Clip` does.
 *
 * @internal
 */
export function deferred(target: Wrapper | undefined, self: Wrapper): boolean {
  let here: unknown = target;
  while (typeof here === "function" && here !== self) {
    if (building.has(here as Wrapper)) {
      return true;
    }
    here = Object.getPrototypeOf(here);
  }
  return false;
}

/** A handle, as a key a `Map` can use. */
function key(handle: NodeHandle): number {
  // An index and a generation are thirty-two bits each, and a number carries
  // fifty-three exactly, so this is a lossless pair and not a hash.
  return handle.index * 0x1_0000_0000 + handle.generation;
}

/**
 * Releases a document once nothing refers to it any more.
 *
 * Registered against the `Doc`, not against any wrapper, so a document lives
 * exactly as long as something can still reach an object in it.
 */
const collector = new FinalizationRegistry<number>((pointer) => {
  if (pointer !== 0) {
    raw.documentFree(pointer);
  }
});

/** A document, shared by every object that lives in it. */
export class Doc {
  /** The document in the module's memory, or 0 once it has moved or gone. */
  #pointer: number;

  /** Where this document's contents went, if they went somewhere. */
  #movedInto: Doc | undefined;

  /** What each of this document's handles became in its new home. */
  #translation: Map<number, NodeHandle> | undefined;

  /** The wrapper handed out for each object, so identity holds. */
  readonly #wrappers = new Map<number, WeakRef<Node>>();

  private constructor(pointer: number) {
    this.#pointer = pointer;
    collector.register(this, pointer, this);
  }

  /**
   * Takes a document the module has just handed over.
   *
   * Reading a file produces one; from here on its lifetime is this object's.
   */
  static take(pointer: number): Doc {
    if (pointer === 0) {
      throw new Error("the OpenTimelineIO module handed back no document");
    }
    return new Doc(pointer);
  }

  /** Makes an empty document for an object about to be built. */
  static create(): Doc {
    const pointer = raw.documentNew();
    if (pointer === 0) {
      throw new Error("the OpenTimelineIO module could not make a document");
    }
    return new Doc(pointer);
  }

  /** Follows the forwarding chain to the document that holds the objects. */
  get live(): Doc {
    // Iterative rather than recursive: a long chain is unlikely but a stack
    // overflow on a timeline someone assembled piece by piece would be a
    // ridiculous way to fail.
    let doc: Doc = this;
    while (doc.#movedInto !== undefined) {
      doc = doc.#movedInto;
    }
    return doc;
  }

  /** The document in the module's memory, following any move. */
  get pointer(): number {
    const live = this.live;
    if (live.#pointer === 0) {
      throw new Error(
        "this timeline has been disposed; the objects in it cannot be used any more",
      );
    }
    return live.#pointer;
  }

  /** Where a handle issued by this document lives now. */
  translate(handle: NodeHandle): NodeHandle {
    let doc: Doc = this;
    let current = handle;
    while (doc.#movedInto !== undefined) {
      current = doc.#translation?.get(key(current)) ?? current;
      doc = doc.#movedInto;
    }
    return current;
  }

  /** Whether two wrappers' objects live in the same document. */
  same(other: Doc): boolean {
    return this.live === other.live;
  }

  /**
   * The handle of an object that already lives here.
   *
   * Used by the calls that only ask questions. An object from another timeline
   * is not in this one and the honest answer is to say so, rather than to move
   * it because somebody asked whether it was here.
   */
  handleOf(node: Node): NodeHandle {
    const at = place(node);
    if (!this.same(at.doc)) {
      throw new OtherTimelineError();
    }
    return at.handle;
  }

  /**
   * The handle of an object, bringing it into this document if it is elsewhere.
   *
   * Used by the calls that edit. This is where `new Clip(...)` followed by
   * `track.append(clip)` turns into one document rather than two.
   */
  adopt(node: Node): NodeHandle {
    return this.#bringHere(node, false);
  }

  /**
   * `adopt`, for the calls that make an object a child.
   *
   * The library refuses to give an object a second parent, and so does this,
   * before anything moves.
   */
  adoptOrphan(node: Node): NodeHandle {
    return this.#bringHere(node, true);
  }

  /**
   * `adopt` and `adoptOrphan`: brings an object here, refusing first what the
   * library would refuse.
   *
   * Bringing an object here brings its whole timeline, and that cannot be
   * taken back: were the library to refuse afterwards, the call would fail
   * with the two timelines already merged, and disposing of either would
   * dispose of both. So an object from another timeline is first asked, there,
   * for its parent. A handle that has gone stale fails that question with the
   * library's own `OtioError`, status and message, and so does anything else
   * the library would not accept, and the refusal moves nothing. Where the
   * call makes the object a child, an answer that it has a parent is refused
   * too, as the library refuses it.
   */
  #bringHere(node: Node, orphan: boolean): NodeHandle {
    const at = place(node);
    if (this.same(at.doc)) {
      return at.handle;
    }
    const parent = raw.nodeParent(at.document, at.handle);
    if (orphan && parent !== undefined) {
      throw new OtioError("coreError", raw.ALREADY_PARENTED);
    }
    this.live.absorb(at.doc.live);
    return place(node).handle;
  }

  /**
   * Moves every object out of another document into this one.
   *
   * Marshalled here rather than generated, because it is the one call in the
   * ABI that consumes what it is given: the two-pass protocol every other list
   * call uses would have to ask twice, and the first pass would already have
   * destroyed the thing being asked about. So the arrays are sized from the
   * source's own object count, which is exactly how many will move.
   */
  private absorb(other: Doc): void {
    const source = other.#pointer;
    if (source === 0) {
      throw new Error("that object's timeline has been disposed");
    }
    const module = exports();
    const moving = module.otio_document_node_count(source);
    const stack = openStack();
    try {
      const slot = stack.alloc(4, 4);
      stack.view.setUint32(slot, source, true);
      const from = stack.alloc(Math.max(moving * 8, 1), 4);
      const to = stack.alloc(Math.max(moving * 8, 1), 4);
      const counted = stack.alloc(4, 4);
      const error = stack.alloc(8, 4);
      const status = module.otio_document_absorb(
        this.#pointer,
        slot,
        from,
        to,
        moving,
        counted,
        error,
      );
      // `check` reads the slot the call wrote its message into and frees it,
      // on success as well as failure, so it is handed every status. What it
      // throws is not what this throws: the error says what was being done
      // when it failed, which is worth more to someone reading it than the
      // library's own sentence about arenas. That sentence is still the
      // reason, so it rides along as the cause.
      try {
        check(status, error);
      } catch (failed) {
        throw new Error(
          `moving an object between timelines failed: ${status}`,
          { cause: failed },
        );
      }

      const moved = stack.view.getUint32(counted, true);
      const translation = new Map<number, NodeHandle>();
      for (let index = 0; index < moved; index += 1) {
        const was = {
          index: stack.view.getUint32(from + index * 8, true),
          generation: stack.view.getUint32(from + index * 8 + 4, true),
        };
        translation.set(key(was), {
          index: stack.view.getUint32(to + index * 8, true),
          generation: stack.view.getUint32(to + index * 8 + 4, true),
        });
      }

      // The source is gone: the call released it and nulled the slot, so the
      // collector must not release it a second time.
      collector.unregister(other);
      other.#pointer = 0;
      other.#movedInto = this;
      other.#translation = translation;

      // The wrappers that were handed out for the source's objects keep
      // working through the chain, and they belong to this document now.
      for (const [handle, wrapper] of other.#wrappers) {
        const arrived = translation.get(handle);
        const held = wrapper.deref();
        if (arrived !== undefined && held !== undefined) {
          this.#wrappers.set(key(arrived), wrapper);
        }
      }
      other.#wrappers.clear();
    } finally {
      stack.close();
    }
  }

  /**
   * The wrapper for an object, making one if this is the first time it has
   * been asked for.
   *
   * Caching them is what makes `track.childAt(0)` and `track.childAt(0)` the
   * same object, the way `track[0] is track[0]` is true in Python. The
   * reference is weak, so a wrapper nobody kept is collected and the cache
   * does not become a leak that grows with every traversal.
   */
  wrap(handle: NodeHandle): Node {
    const live = this.live;
    const at = key(handle);
    const held = live.#wrappers.get(at)?.deref();
    if (held !== undefined) {
      return held;
    }

    const table = classes;
    if (table === undefined) {
      throw new Error("the object model has not finished loading");
    }
    const kind = raw.nodeKind(live.pointer, handle);
    const Class = table[kind] ?? table.item;
    const made = Object.create(Class.prototype) as Node;
    bind(made, live, handle);
    live.#wrappers.set(at, new WeakRef(made));
    return made;
  }

  /**
   * Releases this document and everything in it.
   *
   * Everything that lived here throws afterwards rather than reading memory
   * that has been handed back. Leaving it to the collector is correct too;
   * this is for code that would rather say when.
   */
  dispose(): void {
    const live = this.live;
    if (live.#pointer === 0) {
      return;
    }
    raw.documentFree(live.#pointer);
    collector.unregister(live);
    live.#pointer = 0;
    live.#wrappers.clear();
  }
}

/**
 * Where each wrapper lives, kept beside the wrappers rather than on them.
 *
 * A `WeakMap` and not two fields, so that a `Clip` has the API upstream gives
 * a clip and nothing else: no `$doc` to autocomplete past, nothing to
 * accidentally serialise, and nothing a caller can set. It also means a
 * wrapper nobody holds is collected with its entry.
 */
const bound = new WeakMap<object, { doc: Doc; handle: NodeHandle }>();

/**
 * Gives a freshly built wrapper its document and its handle.
 *
 * @internal
 */
export function bind(node: object, doc: Doc, handle: NodeHandle): void {
  bound.set(node, { doc, handle });
}

/** Where an object is: its document, and its handle there, right now. */
export interface Placed {
  /** The document it lives in, following any move. */
  readonly doc: Doc;
  /** The document in the module's memory. */
  readonly document: number;
  /** Its handle in that document. */
  readonly handle: NodeHandle;
}

/**
 * Finds an object, following any move it has been through.
 *
 * Every generated method begins with this. It is a map lookup and a walk along
 * a forwarding chain that is almost always empty.
 *
 * @internal
 */
export function place(node: object): Placed {
  const at = bound.get(node);
  if (at === undefined) {
    throw new Error("that object was not built by this library");
  }
  const doc = at.doc.live;
  return { doc, document: doc.pointer, handle: at.doc.translate(at.handle) };
}

/**
 * The same, for a call whose subject is a list of objects.
 *
 * It finds the document through the first of them, and the rest are checked
 * against that one as they are passed, so a list drawn from two timelines is
 * refused rather than quietly merging them. An empty list names no timeline
 * at all, which is the one case with no answer.
 *
 * @internal
 */
export function placeAll(nodes: readonly object[], what: string): Placed {
  const first = nodes[0];
  if (first === undefined) {
    throw new Error(`${what} needs at least one object to say which timeline it is about`);
  }
  return place(first);
}

/**
 * Wraps a handle read out of a document as the class its kind names.
 *
 * @internal
 */
export function adopt<T extends Node = Node>(doc: Doc, handle: NodeHandle): T {
  if (isNone(handle)) {
    throw new Error("that object is not there");
  }
  return doc.wrap(handle) as T;
}

/**
 * The boundary between JavaScript and the WebAssembly module.
 *
 * Everything here is about one problem: JavaScript cannot hand the module a
 * pointer, because nothing outside the module's linear memory has an address.
 * To pass a string in, the bytes have to be written into that memory first,
 * and to read one back, the bytes have to be copied out of it. The generated
 * bindings do that through the scratch stack below, which reserves one block
 * from the module and hands out slices of it.
 *
 * None of this is visible from the SDK's own surface. It is here so that the
 * classes in `generated/api.ts` can be three lines each.
 */

import type { WasmExports } from "./generated/exports.js";
import { decodeStatus, type Status } from "./generated/types.js";

/**
 * A handle to an object: which slot of a document's arena it sits in, and
 * which occupant of that slot it was issued for.
 *
 * Handles are values. Two of them naming the same object are equal field by
 * field, and one naming an object that has been removed fails a lookup rather
 * than reaching whatever took its place.
 */
export interface NodeHandle {
  /** Which slot of the arena the object sits in. */
  readonly index: number;
  /** Which occupant of that slot the handle was issued for. */
  readonly generation: number;
}

/** A call into the library failed. */
export class OtioError extends Error {
  /**
   * Which kind of failure it was.
   *
   * `"staleHandle"` means an object was used after it was removed;
   * `"parseError"` means a file was not what it claimed to be. The rest are
   * the C ABI's own statuses, spelled the way TypeScript spells things.
   */
  readonly status: Status;

  constructor(status: Status, message: string) {
    super(message === "" ? status : message);
    this.name = "OtioError";
    this.status = status;
  }
}

/**
 * The library trapped and cannot be used again.
 *
 * `wasm32-unknown-unknown` has no unwinding, so a panic in the Rust core
 * cannot be caught at the boundary the way it is in a native build: it traps,
 * and a trapped instance's memory is in whatever state the panic left it.
 * There is nothing safe to do with it, so the module is marked poisoned and
 * every later call says so rather than reading rubble.
 */
export class OtioPanic extends Error {
  constructor(cause: unknown) {
    super(
      "the OpenTimelineIO module trapped and cannot be used again; " +
        "load a fresh one with init()",
      { cause },
    );
    this.name = "OtioPanic";
  }
}

let instance: WasmExports | undefined;
let poisoned: OtioPanic | undefined;

/** The module's exports, for the generated bindings. */
export function exports(): WasmExports {
  if (poisoned !== undefined) {
    throw poisoned;
  }
  if (instance === undefined) {
    throw new Error(
      "OpenTimelineIO has not been loaded yet: await init() before using it",
    );
  }
  return instance;
}

/** Whether the module is loaded and usable. */
export function ready(): boolean {
  return instance !== undefined && poisoned === undefined;
}

/**
 * Loads a module that has already been compiled or fetched.
 *
 * Most callers want the `init` of `@otio/otio` instead, which finds the
 * `.wasm` for the environment it is running in. This is for the cases where
 * the caller already has the bytes: a bundler that inlined them, a service
 * worker's cache, a compiled module shared between workers.
 */
export function initSync(source: WebAssembly.Module | BufferSource): void {
  const module =
    source instanceof WebAssembly.Module ? source : new WebAssembly.Module(source);
  adopt(new WebAssembly.Instance(module, {}));
}

/** Loads a module from bytes, or from a streaming response. */
export async function initAsync(
  source: BufferSource | Response | PromiseLike<Response>,
): Promise<void> {
  if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
    const { instance: made } = await WebAssembly.instantiate(source, {});
    adopt(made);
    return;
  }
  const { instance: made } = await WebAssembly.instantiateStreaming(source, {});
  adopt(made);
}

/** Takes a freshly made instance into use. */
function adopt(made: WebAssembly.Instance): void {
  instance = made.exports as unknown as WasmExports;
  poisoned = undefined;
  scratch = undefined;
  cached = undefined;
}

/**
 * The block of the module's memory the bindings marshal through, and how much
 * of it is in use.
 *
 * One block is reserved on first use and reused for every call afterwards, so
 * a call that passes a string does not allocate. It grows when something does
 * not fit and never shrinks, which is the right trade for a buffer whose high
 * water mark is a few kilobytes.
 */
interface Scratch {
  pointer: number;
  size: number;
  used: number;
}

let scratch: Scratch | undefined;
let cached: { buffer: ArrayBufferLike; view: DataView } | undefined;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** The size of the first scratch block, in bytes. */
const INITIAL_SCRATCH = 4096;

/**
 * A slice of the scratch block, held for the length of one call.
 *
 * Opening one remembers how much of the block was in use and closing one puts
 * it back, so nesting works and nothing is freed per call. The generated
 * bindings open one, marshal their arguments, call, read the results and close
 * it in a `finally`.
 */
export class Stack {
  /** Where this slice started, so closing can rewind to it. */
  readonly #mark: number;

  /** @internal */
  constructor(mark: number) {
    this.#mark = mark;
  }

  /**
   * A view over the module's memory.
   *
   * Refetched whenever the memory has grown, because growing it detaches every
   * `ArrayBuffer` over the old one and a `DataView` held across a call that
   * allocates would throw.
   */
  get view(): DataView {
    const memory = exports().memory;
    if (cached === undefined || cached.buffer !== memory.buffer) {
      cached = { buffer: memory.buffer, view: new DataView(memory.buffer) };
    }
    return cached.view;
  }

  /** Reserves `size` bytes, aligned, and returns where they are. */
  alloc(size: number, alignment: number): number {
    const block = reserve();
    const at = (block.used + alignment - 1) & ~(alignment - 1);
    if (at + size > block.size) {
      grow(at + size);
      return this.alloc(size, alignment);
    }
    block.used = at + size;
    return block.pointer + at;
  }

  /** Writes a string as UTF-8, NUL-terminated, and returns where it is. */
  text(value: string): number {
    const bytes = encoder.encode(value);
    const at = this.alloc(bytes.length + 1, 1);
    new Uint8Array(exports().memory.buffer, at, bytes.length).set(bytes);
    this.view.setUint8(at + bytes.length, 0);
    return at;
  }

  /** Writes bytes, and returns where they are and how many there were. */
  bytes(value: Uint8Array): { pointer: number; length: number } {
    const at = this.alloc(Math.max(value.length, 1), 1);
    new Uint8Array(exports().memory.buffer, at, value.length).set(value);
    return { pointer: at, length: value.length };
  }

  /** Writes a struct, and returns where it is. */
  record<T>(
    write: (stack: Stack, at: number, value: T) => void,
    size: number,
    alignment: number,
    value: T,
  ): number {
    const at = this.alloc(size, alignment);
    write(this, at, value);
    return at;
  }

  /** Writes a handle, and returns where it is. */
  node(handle: NodeHandle): number {
    const at = this.alloc(8, 4);
    this.view.setUint32(at, handle.index, true);
    this.view.setUint32(at + 4, handle.generation, true);
    return at;
  }

  /** Writes the handle that names no object, and returns where it is. */
  noneNode(): number {
    return this.node(NONE);
  }

  /** Writes an array of handles, and returns where it is and how many. */
  nodes(handles: readonly NodeHandle[]): { pointer: number; length: number } {
    const at = this.alloc(Math.max(handles.length * 8, 1), 4);
    handles.forEach((handle, index) => {
      this.view.setUint32(at + index * 8, handle.index, true);
      this.view.setUint32(at + index * 8 + 4, handle.generation, true);
    });
    return { pointer: at, length: handles.length };
  }

  /** Gives the slice back. */
  close(): void {
    if (scratch !== undefined) {
      scratch.used = this.#mark;
    }
  }
}

/** The handle that names no object. */
export const NONE: NodeHandle = { index: 0xffffffff, generation: 0xffffffff };

/** Whether a handle names no object. */
export function isNone(handle: NodeHandle): boolean {
  return handle.index === NONE.index && handle.generation === NONE.generation;
}

/** Opens a slice of the scratch block for the length of one call. */
export function openStack(): Stack {
  return new Stack(reserve().used);
}

/** The scratch block, reserved on first use. */
function reserve(): Scratch {
  if (scratch === undefined) {
    const pointer = exports().otio_wasm_alloc(INITIAL_SCRATCH);
    if (pointer === 0) {
      throw new Error("the OpenTimelineIO module could not reserve scratch memory");
    }
    scratch = { pointer, size: INITIAL_SCRATCH, used: 0 };
  }
  return scratch;
}

/** Replaces the scratch block with a bigger one. */
function grow(needed: number): void {
  const block = reserve();
  let size = block.size;
  while (size < needed) {
    size *= 2;
  }
  const pointer = exports().otio_wasm_alloc(size);
  if (pointer === 0) {
    throw new Error("the OpenTimelineIO module ran out of memory");
  }
  // Nothing in the old block outlives the call being marshalled, and a call is
  // never in progress while this runs, so the contents do not have to move.
  exports().otio_wasm_free(block.pointer, block.size);
  block.pointer = pointer;
  block.size = size;
}

/**
 * Turns a failing status into a thrown error, with the message the same call
 * wrote beside it.
 *
 * `error` is the address of the `OtioBuffer` the call was handed as its last
 * argument. The library writes it on every return — empty after a success, an
 * owned message after anything else — so it is read and freed here whatever
 * the status says, once for every time the library wrote it: a list call's
 * two passes each come here before the next can write over the slot. After a
 * success there is nothing in it and nothing to free.
 *
 * `OTIO_STATUS_NO_VALUE` never reaches here: the generated bindings answer
 * `undefined` for it, because "there is nothing" is an answer and not a
 * failure, and they give its message to `release` instead.
 */
export function check(status: number, error: number): void {
  const message = readBuffer(error, "text");
  if (status === 0) {
    return;
  }
  throw new OtioError(decodeStatus(status), message);
}

/**
 * Frees the message a call wrote, without reading it.
 *
 * For the one status that is not a failure but still comes with a sentence:
 * `OTIO_STATUS_NO_VALUE` says what there was nothing of, and that sentence is
 * an owned buffer like any other even though nobody is going to see it.
 */
export function release(error: number): void {
  if (new DataView(exports().memory.buffer).getUint32(error, true) !== 0) {
    exports().otio_buffer_free(error);
  }
}

/**
 * Reads a NUL-terminated string the library owns and does not hand over.
 *
 * Version strings and status names live in the module's static data, so there
 * is nothing to free.
 */
export function readCString(pointer: number): string {
  if (pointer === 0) {
    return "";
  }
  const bytes = new Uint8Array(exports().memory.buffer, pointer);
  let length = 0;
  while (bytes[length] !== 0) {
    length += 1;
  }
  return decoder.decode(bytes.subarray(0, length));
}

/**
 * Reads a buffer the library handed over, and frees it.
 *
 * Every string and every written file comes back this way. Copying out and
 * freeing here is what keeps `free` out of the SDK's surface entirely.
 */
export function readBuffer(at: number, as: "text"): string;
export function readBuffer(at: number, as: "bytes"): Uint8Array;
export function readBuffer(at: number, as: "text" | "bytes"): string | Uint8Array {
  const memory = exports().memory;
  const view = new DataView(memory.buffer);
  const data = view.getUint32(at, true);
  const length = view.getUint32(at + 4, true);
  if (data === 0) {
    return as === "text" ? "" : new Uint8Array(0);
  }
  try {
    const bytes = new Uint8Array(memory.buffer, data, length);
    // `slice` copies, which it has to: the next call may grow the memory and
    // detach every view over it, and freeing the buffer below makes the bytes
    // fair game for the allocator.
    return as === "text" ? decoder.decode(bytes) : bytes.slice();
  } finally {
    exports().otio_buffer_free(at);
  }
}

/**
 * Runs a call, turning a trap into a poisoned module.
 *
 * Not used by the generated bindings, which call straight through: a trap is
 * rare enough that paying for a try/catch on every one of two hundred and
 * sixty calls would be the wrong trade. The SDK's entry points that run whole
 * jobs — reading a file, writing one — go through here.
 */
export function guard<T>(run: () => T): T {
  try {
    return run();
  } catch (error) {
    if (error instanceof WebAssembly.RuntimeError) {
      poisoned = new OtioPanic(error);
      instance = undefined;
      throw poisoned;
    }
    throw error;
  }
}

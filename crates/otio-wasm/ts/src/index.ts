/**
 * OpenTimelineIO for the browser and for Node.
 *
 * The Rust core, compiled to WebAssembly, behind the API OpenTimelineIO's own
 * users already know: the same class hierarchy, the same names for the same
 * ideas, spelled the way TypeScript spells things.
 *
 * ```ts
 * import { init, readFromString, Clip, RationalTime, TimeRange } from "@alchemist-edit/otio";
 *
 * await init();
 *
 * const timeline = readFromString("cmx3600", edl, { rate: 24 });
 * for (const clip of timeline.findClips()) {
 *   console.log(clip.name, clip.trimmedRange().duration.toTimecode());
 * }
 *
 * const shot = new Clip({ name: "shot_01" });
 * shot.sourceRange = new TimeRange(
 *   RationalTime.fromTimecode("01:00:00:00", 24),
 *   RationalTime.fromFrames(48, 24),
 * );
 * ```
 *
 * Most people want `init` from this module, which finds the `.wasm` for
 * wherever it is running. `initSync` and `initAsync` are for the cases where
 * the caller already has the bytes.
 */

export * from "./generated/api.js";
export * from "./generated/values.js";
export * from "./adapters.js";
export {
  Metadata,
  type MetadataColor,
  type MetadataValue,
} from "./metadata.js";
export {
  OtioError,
  OtioPanic,
  initAsync,
  initSync,
  ready,
  type NodeHandle,
} from "./runtime.js";
export type {
  DropFrame,
  EdlStyle,
  Format,
  MissingFramePolicy,
  NeighborGapPolicy,
  NodeKind,
  ReadOptions,
  ReferencePoint,
  Status,
  ValueKind,
  WriteOptions,
  Box2d,
  Color,
  Handles,
  ImageSequence,
  V2d,
} from "./generated/types.js";


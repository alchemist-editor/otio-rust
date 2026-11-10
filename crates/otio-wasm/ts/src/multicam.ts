/**
 * Making multicam objects: a prototype of the schema proposed for
 * OpenTimelineIO (`Multicam.1`, `MulticamClip.1`), and not yet part of it.
 *
 * Everything else about multicam is in the generated `multicam` namespace.
 * These two build objects, which in this library means giving each its own
 * document until it is put inside something else, as `new Clip()` does.
 */

import * as raw from "./generated/raw.js";
import type { Item, Stack } from "./generated/api.js";
import type { RationalTimeLike, TimeRangeLike } from "./generated/values.js";
import { Doc, adopt } from "./objects.js";

/**
 * Creates an empty multicam definition. Add one stack per angle as its
 * children; an angle's name is its identity. Put it on a timeline with
 * `multicam.set(timeline, key, definition)`.
 */
export function newMulticam(name?: string, globalStartTime?: RationalTimeLike): Stack {
  const doc = Doc.create();
  return adopt<Stack>(doc, raw.multicamNew(doc.pointer, name, globalStartTime));
}

/** What a cut plays: which multicam, which angle, and which of its tracks. */
export interface MulticamCut {
  /** The key the timeline holds the multicam under. */
  multicam: string;
  /** The angle's name. */
  angle: string;
  /**
   * Which of the angle's tracks, counting only those of the kind of the track
   * the cut is on, from 0.
   */
  angleTrack?: number;
}

/**
 * Creates a cut over `sourceRange`, in the multicam's own time, not yet on a
 * track.
 */
export function newMulticamClip(cut: MulticamCut, sourceRange: TimeRangeLike, name?: string): Item {
  const doc = Doc.create();
  return adopt<Item>(
    doc,
    raw.multicamClipNew(doc.pointer, name, cut.multicam, cut.angle, cut.angleTrack ?? 0, sourceRange),
  );
}

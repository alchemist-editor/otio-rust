/*
 * otio_ext_ori.h: ORI Annotations, an otio-rust extension.
 *
 * ORI Annotations is NOT part of OpenTimelineIO. It reads, writes and edits
 * the review annotations of the ASWF's ORI project (paint strokes, text and
 * shapes drawn on a frame), and does nothing until it is enabled, with
 * otio_ext_ori_enable() or otio_ext_enable("ori-annotations"). Enabling it
 * enables timeline sync, which it is built on.
 *
 * Its functions are compiled into the same libotio as otio.h's, and follow
 * the same contract: see otio.h.
 */

#ifndef OTIO_EXT_ORI_H
#define OTIO_EXT_ORI_H

#include "otio.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Turns ORI Annotations on, and timeline sync which it is built on. The
 *  same as `otio_ext_enable("ori-annotations")`.
 *
 *  Enabling it again does nothing and succeeds. */
OtioStatus otio_ext_ori_enable(OtioBuffer *out_error);

/** Writes whether ORI Annotations is on to `out_enabled`. */
OtioStatus otio_ext_ori_is_enabled(bool *out_enabled, OtioBuffer *out_error);

/* Every call below reports OTIO_STATUS_PLUGIN_ERROR until the extension is
 * enabled.
 *
 * Commands cross as JSON: a list of ORI objects, each with its
 * `OTIO_SCHEMA` (`PaintStart.1`, `PaintPoint.1`, `PaintEnd.1`,
 * `TextAnnotation.1` and the rest), exactly as an annotation clip's
 * `metadata.annotation_commands` holds them in a file.
 *
 * A frame's annotations live on a one-frame clip in a track named
 * `Annotations`, with `metadata.clip_guid` naming the annotated media clip
 * by its sync GUID. */

/** Finds the annotation track of `timeline`, the first track whose name
 *  contains "annotation" in any case, and writes it to `out_track`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when it has none. */
OtioStatus otio_ext_ori_find_track(const OtioDocument *source,
                                   OtioNode timeline, OtioNode *out_track,
                                   OtioBuffer *out_error);

/** Writes the annotation track of `timeline` to `out_track`, first adding a
 *  video track named `Annotations`, with a new sync GUID, on top of its
 *  stack if it has none. */
OtioStatus otio_ext_ori_ensure_track(OtioDocument *target, OtioNode timeline,
                                     OtioNode *out_track,
                                     OtioBuffer *out_error);

/** Finds the annotation clip on `track` for the media clip whose sync GUID
 *  is `clip_guid`, at source frame `frame`, and writes it to `out_clip`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when there is none. */
OtioStatus otio_ext_ori_find_clip(const OtioDocument *source, OtioNode track,
                                  const char *clip_guid, int64_t frame,
                                  OtioNode *out_clip, OtioBuffer *out_error);

/** Writes the annotation clip on `track` for `clip_guid` at the time
 *  `value` at `rate` to `out_clip`, first adding it if there is none: one
 *  frame long at that time, at the end of the track, after a gap if the time
 *  is later than the track's end, as ORI adds one. */
OtioStatus otio_ext_ori_ensure_clip(OtioDocument *target, OtioNode track,
                                    const char *clip_guid, double value,
                                    double rate, OtioNode *out_clip,
                                    OtioBuffer *out_error);

/** Returns the object holding each annotation on `timeline`: an annotation
 *  clip, or an `AnnotationEffect.1` on a clip, in the order the timeline is
 *  walked. Fills `out_nodes` as `otio_node_children` does. */
OtioStatus otio_ext_ori_list(const OtioDocument *source, OtioNode timeline,
                             OtioNode *out_nodes, size_t capacity,
                             size_t *out_count, OtioBuffer *out_error);

/** Returns the object holding each annotation of the media clip whose sync
 *  GUID is `clip_guid`. */
OtioStatus otio_ext_ori_list_for_clip(const OtioDocument *source,
                                      OtioNode timeline,
                                      const char *clip_guid,
                                      OtioNode *out_nodes, size_t capacity,
                                      size_t *out_count,
                                      OtioBuffer *out_error);

/** Returns the object holding each annotation of the media clip whose sync
 *  GUID is `clip_guid` at source frame `frame`. An `AnnotationEffect.1`
 *  covers every frame of its clip. */
OtioStatus otio_ext_ori_list_at_frame(const OtioDocument *source,
                                      OtioNode timeline,
                                      const char *clip_guid, int64_t frame,
                                      OtioNode *out_nodes, size_t capacity,
                                      size_t *out_count,
                                      OtioBuffer *out_error);

/** Writes what `holder`, an annotation clip or an `AnnotationEffect.1`,
 *  says about its annotation to `out_json`, as a JSON object: `form`
 *  (`track` or `effect`), `clip_guid`, `clip_name` (where only the name is
 *  known, as in ORI's review export), `frame` (`{"value", "rate"}`, or
 *  `null` for an effect), `note`, `status` and `command_count`.
 *
 *  Reports `OTIO_STATUS_INVALID_ARGUMENT` for an object that holds no
 *  annotation. */
OtioStatus otio_ext_ori_describe(const OtioDocument *source, OtioNode holder,
                                 OtioBuffer *out_json, OtioBuffer *out_error);

/** Writes the commands `holder` holds to `out_json`, as a JSON list of ORI
 *  objects, indented as OTIO files are with `pretty` and compact without. */
OtioStatus otio_ext_ori_commands(const OtioDocument *source, OtioNode holder,
                                 bool pretty, OtioBuffer *out_json,
                                 OtioBuffer *out_error);

/** Adds the commands in `commands_json` at the end of the annotation clip
 *  `clip`'s commands. */
OtioStatus otio_ext_ori_add_commands(OtioDocument *target, OtioNode clip,
                                     const char *commands_json,
                                     OtioBuffer *out_error);

/** Replaces every command of `clip` whose `uuid` is `uuid`, such as one
 *  whole stroke, with the commands in `commands_json`, put where the first
 *  was, and writes whether any had that uuid to `out_replaced`. If none did,
 *  nothing changes. */
OtioStatus otio_ext_ori_replace_commands(OtioDocument *target, OtioNode clip,
                                         const char *uuid,
                                         const char *commands_json,
                                         bool *out_replaced,
                                         OtioBuffer *out_error);

/** Removes every command of `clip` whose `uuid` is in `uuids_json`, a JSON
 *  list of strings, and writes how many it removed to `out_count`. */
OtioStatus otio_ext_ori_remove_commands(OtioDocument *target, OtioNode clip,
                                        const char *uuids_json,
                                        size_t *out_count,
                                        OtioBuffer *out_error);

/** Replaces all of `clip`'s commands with those in `commands_json`, as
 *  ORI's `REPLACE_ANNOTATION_COMMANDS` does. An empty list, `[]`, clears
 *  it. */
OtioStatus otio_ext_ori_set_commands(OtioDocument *target, OtioNode clip,
                                     const char *commands_json,
                                     OtioBuffer *out_error);

/** Merges the commands in `commands_json` into the annotation clip `clip`
 *  by `uuid`: a stroke, text or shape the clip already holds is replaced
 *  where it was, and the rest are added at the end. Writes how many uuids
 *  it replaced to `out_replaced` and how many it added to `out_added`. */
OtioStatus otio_ext_ori_merge_commands(OtioDocument *target, OtioNode clip,
                                       const char *commands_json,
                                       size_t *out_replaced,
                                       size_t *out_added,
                                       OtioBuffer *out_error);

/** Replaces the commands of `clip` kind by kind with those in
 *  `commands_json`: an empty list, `[]`, clears it, but a list holding no
 *  command of one kind (strokes, text, each kind of shape) leaves the
 *  clip's commands of that kind alone. Writes how many commands it removed
 *  to `out_removed`. */
OtioStatus otio_ext_ori_replace_by_kind(OtioDocument *target, OtioNode clip,
                                        const char *commands_json,
                                        size_t *out_removed,
                                        OtioBuffer *out_error);

/** Makes the annotation track of `timeline` hold one annotation clip for
 *  each media clip and source frame, merging later clips for the same
 *  frame into the first by uuid, and writes how many clips it removed to
 *  `out_count`. */
OtioStatus otio_ext_ori_merge_duplicates(OtioDocument *target,
                                         OtioNode timeline, size_t *out_count,
                                         OtioBuffer *out_error);

/** Writes who the commands with `uuid` on the annotation clip `clip`
 *  belong to, by person id, to `out_owner`: the author of the review
 *  comment that added them.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when no comment did, as for strokes
 *  drawn over ORI's wire or read from a file. */
OtioStatus otio_ext_ori_owner_of(const OtioDocument *source, OtioNode clip,
                                 const char *uuid, OtioBuffer *out_owner,
                                 OtioBuffer *out_error);

/* Live review works on a sync session, the number otio_ext_sync_session_new
 * hands out (see otio_ext_sync.h), and on events as
 * otio_ext_sync_session_poll_event writes them. */

/** Sends a stroke in progress to everyone in the sync session `session`:
 *  `partial_json` is ORI's `PARTIAL` payload, an object with `clip_guid`,
 *  `frame`, `fps` and `events`, the commands drawn so far. Peers show it
 *  and never store it. A newer one not yet sent replaces it.
 *
 *  Furnace cannot relay it yet, and refuses it with
 *  `OTIO_STATUS_UNSUPPORTED` when it is about to be sent. */
OtioStatus otio_ext_ori_send_partial(uint64_t session,
                                     const char *partial_json,
                                     OtioBuffer *out_error);

/** Writes the stroke in progress the sync event `event_json` carries to
 *  `out_partial_json`, as ORI's `PARTIAL` payload with every field a
 *  command left out filled in with ORI's default.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when the event is not a stroke in
 *  progress. */
OtioStatus otio_ext_ori_read_partial(const char *event_json,
                                     OtioBuffer *out_partial_json,
                                     OtioBuffer *out_error);

/** Commits a review comment to the sync session `session`'s ledger, and
 *  writes its id to `out_id`: `client_mutation_id`, or a new one if that
 *  is empty.
 *
 *  `anchor_json` is Furnace's anchor: `sequenceId`, `capturedGeneration`,
 *  `clipGuid`, `sourceTime` and `capturedSequenceTime` (each
 *  `{"value", "rate"}`), `capturedAt` in Unix milliseconds, and optionally
 *  `sessionId` (filled in when empty) and `mediaFingerprint`.
 *  `body_markdown` is the comment, and `commands_json` its strokes, text
 *  and shapes. `source` and `timeline` are this client's copy: the commit
 *  is refused, with `OTIO_STATUS_INVALID_ARGUMENT`, if it would replace
 *  commands that belong to someone else. */
OtioStatus otio_ext_ori_commit(uint64_t session, const OtioDocument *source,
                               OtioNode timeline, const char *anchor_json,
                               const char *body_markdown,
                               const char *commands_json,
                               const char *client_mutation_id,
                               OtioBuffer *out_id, OtioBuffer *out_error);

/** Folds the review comment a `review_relay` event, `event_json`, carries
 *  into `timeline`, on the host: adds the annotation track and clip if
 *  need be, merges the comment's commands in by uuid, and records the
 *  comment on the clip. Writes the clip to `out_clip`, and to
 *  `out_already` whether the comment was there already, so nothing
 *  changed. The host then publishes the timeline.
 *
 *  Reports `OTIO_STATUS_INVALID_ARGUMENT` for another kind of event, or a
 *  comment that would replace commands that are not its author's, in which
 *  case nothing changes. */
OtioStatus otio_ext_ori_fold(OtioDocument *target, OtioNode timeline,
                             const char *event_json, OtioNode *out_clip,
                             bool *out_already, OtioBuffer *out_error);

/** Writes a new uuid for a command, a version 4 UUID, to `out_uuid`. */
OtioStatus otio_ext_ori_new_uuid(OtioBuffer *out_uuid, OtioBuffer *out_error);

/** Writes the current time in ISO 8601, for a command's `timestamp`, to
 *  `out_timestamp`. */
OtioStatus otio_ext_ori_timestamp_now(OtioBuffer *out_timestamp,
                                      OtioBuffer *out_error);

/* Coordinates. ORI's are height-normalised with Y up and the origin at the
 * centre of the picture: x = (px - width / 2) / height and
 * y = -(py - height / 2) / height. Points cross as two numbers. */

/** Converts the pixel position `px`, `py`, counted from the top left of an
 *  image `width` by `height` pixels, to ORI's coordinates, and writes them
 *  to `out_x` and `out_y`. */
OtioStatus otio_ext_ori_pixels_to_ori(double width, double height, double px,
                                      double py, double *out_x, double *out_y,
                                      OtioBuffer *out_error);

/** Converts the point `x`, `y`, in ORI's coordinates, to a pixel position
 *  counted from the top left of an image `width` by `height` pixels,
 *  unrounded, and writes it to `out_px` and `out_py`. */
OtioStatus otio_ext_ori_ori_to_pixels(double width, double height, double x,
                                      double y, double *out_px, double *out_py,
                                      OtioBuffer *out_error);

/** Writes half the width of an image `width` by `height` pixels in ORI's
 *  units, `width / (2 * height)`, to `out_half`: ORI's 16:9 default, 8/9,
 *  when the height is not positive. */
OtioStatus otio_ext_ori_aspect_half(double width, double height,
                                    double *out_half, OtioBuffer *out_error);

/** Converts the point `x`, `y`, in ORI's coordinates, to OpenTimelineIO
 *  image space within a picture's `available_image_bounds`, the box from
 *  `min_x`, `min_y` to `max_x`, `max_y`, whose centre is ORI's origin and
 *  whose height is ORI's unit, and writes it to `out_x` and `out_y`. */
OtioStatus otio_ext_ori_ori_to_bounds(double x, double y, double min_x,
                                      double min_y, double max_x, double max_y,
                                      double *out_x, double *out_y,
                                      OtioBuffer *out_error);

/** Converts the point `x`, `y`, in OpenTimelineIO image space, to ORI's
 *  coordinates within a picture's `available_image_bounds`, the box from
 *  `min_x`, `min_y` to `max_x`, `max_y`, and writes them to `out_x` and
 *  `out_y`. The inverse of `otio_ext_ori_ori_to_bounds`. */
OtioStatus otio_ext_ori_bounds_to_ori(double x, double y, double min_x,
                                      double min_y, double max_x, double max_y,
                                      double *out_x, double *out_y,
                                      OtioBuffer *out_error);

#ifdef __cplusplus
}
#endif

#endif /* OTIO_EXT_ORI_H */

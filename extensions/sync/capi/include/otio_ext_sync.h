/*
 * otio_ext_sync.h: timeline sync, an otio-rust extension.
 *
 * Timeline sync is NOT part of OpenTimelineIO. It shares an OpenTimelineIO
 * timeline between programs while it is edited, and does nothing until it
 * is enabled, with otio_ext_sync_enable() or otio_ext_enable("sync").
 *
 * Its functions are compiled into the same libotio as otio.h's, and follow
 * the same contract: see otio.h.
 */

#ifndef OTIO_EXT_SYNC_H
#define OTIO_EXT_SYNC_H

#include "otio.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Turns timeline sync on. The same as `otio_ext_enable("sync")`.
 *
 *  Enabling it again does nothing and succeeds. */
OtioStatus otio_ext_sync_enable(OtioBuffer *out_error);

/** Writes whether timeline sync is on to `out_enabled`. */
OtioStatus otio_ext_sync_is_enabled(bool *out_enabled, OtioBuffer *out_error);

/* Every call below reports OTIO_STATUS_PLUGIN_ERROR until sync is enabled.
 *
 * A snapshot is JSON as Alchemist's Furnace sends one: `sequenceId`,
 * `generation`, and the timeline as `otioJson`. Patches are a JSON list of
 * Furnace's patches, each naming the objects it is about by the GUID they
 * carry in `metadata.sync.guid`. */

/** Writes a new GUID, a version 4 UUID, to `out_guid`. */
OtioStatus otio_ext_sync_new_guid(OtioBuffer *out_guid, OtioBuffer *out_error);

/** Writes the GUID `node` carries in `metadata.sync.guid` to `out_guid`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` for an object that carries none. */
OtioStatus otio_ext_sync_guid(const OtioDocument *source, OtioNode node,
                              OtioBuffer *out_guid, OtioBuffer *out_error);

/** Gives `node` the GUID `guid`, replacing any it had. */
OtioStatus otio_ext_sync_set_guid(OtioDocument *target, OtioNode node,
                                  const char *guid, OtioBuffer *out_error);

/** Gives every object sync addresses under `timeline` a GUID, if it lacks
 *  one or shares one with an object met earlier, and writes how many it gave
 *  one to to `out_count`. */
OtioStatus otio_ext_sync_stamp(OtioDocument *target, OtioNode timeline,
                               size_t *out_count, OtioBuffer *out_error);

/** Finds the object under `timeline` that carries the GUID `guid`, and
 *  writes it to `out_node`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when none does, and
 *  `OTIO_STATUS_INVALID_ARGUMENT` when two do; stamp first to rule that out. */
OtioStatus otio_ext_sync_find(const OtioDocument *source, OtioNode timeline,
                              const char *guid, OtioNode *out_node,
                              OtioBuffer *out_error);

/** Works out the patches that take the snapshot `old_snapshot` to the
 *  snapshot `new_snapshot`, as Furnace's `otiodiff` does, and writes them to `out_patches` as a JSON
 *  list.
 *
 *  With `pretty`, the list is indented as Furnace's golden files are;
 *  otherwise it is compact. A change too large for patches, or one between
 *  sequences or back in generation, is one `replace_timeline` patch;
 *  `otio_ext_sync_diff_stats` says why. */
OtioStatus otio_ext_sync_diff(const char *old_snapshot,
                              const char *new_snapshot, bool pretty,
                              OtioBuffer *out_patches, OtioBuffer *out_error);

/** Writes what `otio_ext_sync_diff` does with the same snapshots to
 *  `out_stats`, as a JSON object: `insertOps`, `moveOps`, `removeOps`,
 *  `setPropertyOps`, `totalOps`, `nodeCount`, `fellBack` and
 *  `fallbackReason`. */
OtioStatus otio_ext_sync_diff_stats(const char *old_snapshot,
                              const char *new_snapshot,
                                    OtioBuffer *out_stats,
                                    OtioBuffer *out_error);

/** Applies the JSON list of patches `patches` to the timeline JSON
 *  `timeline`, as Furnace's `otiodiff` does, and writes the patched timeline
 *  to `out_timeline` as compact JSON. */
OtioStatus otio_ext_sync_apply(const char *timeline, const char *patches,
                               OtioBuffer *out_timeline, OtioBuffer *out_error);

/** Applies the JSON list of patches `patches` to `timeline` in place.
 *
 *  The timeline keeps its handle, as does every synced object in it that the
 *  patches keep; one they remove is stale afterwards. On failure the
 *  document is left as it was. */
OtioStatus otio_ext_sync_apply_to(OtioDocument *target, OtioNode timeline,
                                  const char *patches, OtioBuffer *out_error);

/** Starts a transaction on `timeline`, which is `sequence_id` at
 *  `generation`: stamps any missing GUIDs, and writes the timeline as it is
 *  to `out_before` as a snapshot.
 *
 *  The snapshot is the transaction. Keep it, edit the timeline, and pass it
 *  to `otio_ext_sync_commit` for the patches, or to `otio_ext_sync_rollback`
 *  to undo the edits. */
OtioStatus otio_ext_sync_begin(OtioDocument *target, OtioNode timeline,
                               const char *sequence_id, int64_t generation,
                               OtioBuffer *out_before, OtioBuffer *out_error);

/** Ends the transaction `otio_ext_sync_begin` wrote as `before`, writing the
 *  patches that take the timeline as it was to the timeline as it is, the
 *  next generation, to `out_patches` as a compact JSON list.
 *
 *  An edit that changed nothing writes `[]`. */
OtioStatus otio_ext_sync_commit(OtioDocument *target, OtioNode timeline,
                                const char *before, OtioBuffer *out_patches,
                                OtioBuffer *out_error);

/** Ends the transaction `otio_ext_sync_begin` wrote as `before`, putting the
 *  timeline back as it was.
 *
 *  The timeline and every synced object that was in it keep their handles. */
OtioStatus otio_ext_sync_rollback(OtioDocument *target, OtioNode timeline,
                                  const char *before, OtioBuffer *out_error);

/** Starts recording which objects any edit to the document `timeline` is in
 *  touches.
 *
 *  The journal is the document's, so it records edits anywhere in it; the
 *  timeline only says which document. Recording again does nothing and
 *  succeeds. */
OtioStatus otio_ext_sync_start_journal(OtioDocument *target, OtioNode timeline,
                                       OtioBuffer *out_error);

/** Stops recording edits to the document `timeline` is in, and forgets what
 *  was recorded since the last commit. */
OtioStatus otio_ext_sync_stop_journal(OtioDocument *target, OtioNode timeline,
                                      OtioBuffer *out_error);

/** Takes what the journal of the document `timeline` is in recorded since it
 *  started or was last committed, runs the `document_changed` hook on the
 *  document's root with it if anything changed, and writes whether anything
 *  did to `out_changed`.
 *
 *  The hook's arguments are `touched`, the GUIDs of the synced objects that
 *  were added or may have changed, and `root_changed`. */
OtioStatus otio_ext_sync_commit_journal(OtioDocument *target,
                                        OtioNode timeline, bool *out_changed,
                                        OtioBuffer *out_error);

#ifdef __cplusplus
}
#endif

#endif /* OTIO_EXT_SYNC_H */

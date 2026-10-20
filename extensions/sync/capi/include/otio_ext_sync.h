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

/* A session shares timelines with other programs through a server, speaking
 * Alchemist's Furnace protocol v2. It owns no socket: the program opens one,
 * hands the session each frame that arrives with otio_ext_sync_session_receive,
 * and sends each frame otio_ext_sync_session_poll_transmit gives it. A
 * session is named by a number from otio_ext_sync_session_new. Settings,
 * events and state cross as JSON. */

/** Makes a session from the JSON object `settings` (`token`, `identity`,
 *  `encodings`, each optional), and writes the number that names it to
 *  `out_session`. Release it with `otio_ext_sync_session_close`. */
OtioStatus otio_ext_sync_session_new(const char *settings,
                                     uint64_t *out_session,
                                     OtioBuffer *out_error);

/** Releases `session`. Closing one already closed does nothing and
 *  succeeds. It sends nothing: leave first to say goodbye. */
OtioStatus otio_ext_sync_session_close(uint64_t session, OtioBuffer *out_error);

/** Starts joining on a new connection: call it once the socket is open, and
 *  again after a reconnect. */
OtioStatus otio_ext_sync_session_connect(uint64_t session,
                                         OtioBuffer *out_error);

/** Hands `session` a frame that arrived; `binary` says whether it was a
 *  binary WebSocket frame. A failure means the connection must be made
 *  again. */
OtioStatus otio_ext_sync_session_receive(uint64_t session, const uint8_t *data,
                                         size_t len, bool binary,
                                         OtioBuffer *out_error);

/** Writes the next frame to send to `out_bytes`, stamped with `now` in Unix
 *  milliseconds, and whether it is a binary frame to `out_binary`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when there is nothing to send. */
OtioStatus otio_ext_sync_session_poll_transmit(uint64_t session, int64_t now,
                                               OtioBuffer *out_bytes,
                                               bool *out_binary,
                                               OtioBuffer *out_error);

/** Writes the next event to `out_event` as a JSON object with a `type`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when there is none. */
OtioStatus otio_ext_sync_session_poll_event(uint64_t session,
                                            OtioBuffer *out_event,
                                            OtioBuffer *out_error);

/** Tells `session` the socket closed. */
OtioStatus otio_ext_sync_session_disconnected(uint64_t session,
                                              OtioBuffer *out_error);

/** Leaves the session: the goodbye is the next frame to send. */
OtioStatus otio_ext_sync_session_leave(uint64_t session, OtioBuffer *out_error);

/** Writes where `session` stands to `out_state` as a JSON object. */
OtioStatus otio_ext_sync_session_state(uint64_t session, OtioBuffer *out_state,
                                       OtioBuffer *out_error);

/** Writes the latest snapshot of `sequence_id` the session sent or received
 *  to `out_snapshot`.
 *
 *  Reports `OTIO_STATUS_NO_VALUE` when it has none. */
OtioStatus otio_ext_sync_session_timeline(uint64_t session,
                                          const char *sequence_id,
                                          OtioBuffer *out_snapshot,
                                          OtioBuffer *out_error);

/** Writes whether `session` may send on `channel` now to `out_may`. */
OtioStatus otio_ext_sync_session_may_send(uint64_t session,
                                          const char *channel, bool *out_may,
                                          OtioBuffer *out_error);

/** Sends `timeline`, as the sequence `sequence_id`, and writes the patches
 *  from the version sent before to `out_patches` as a JSON list. Only the
 *  host may. */
OtioStatus otio_ext_sync_session_publish(uint64_t session,
                                         OtioDocument *target,
                                         OtioNode timeline,
                                         const char *sequence_id,
                                         OtioBuffer *out_patches,
                                         OtioBuffer *out_error);

/** Moves this client's playhead in `sequence_id` to `frame`, playing at
 *  `rate` or stopped. */
OtioStatus otio_ext_sync_session_set_playhead(uint64_t session,
                                              const char *sequence_id,
                                              double frame, bool playing,
                                              double rate,
                                              OtioBuffer *out_error);

/** Selects the objects in `sequence_id` whose GUIDs the JSON list `guids`
 *  holds. */
OtioStatus otio_ext_sync_session_set_selection(uint64_t session,
                                               const char *sequence_id,
                                               const char *guids,
                                               OtioBuffer *out_error);

/** Commits the review comment `commit`, a JSON object, and writes its id to
 *  `out_id`. */
OtioStatus otio_ext_sync_session_commit_review(uint64_t session,
                                               const char *commit,
                                               OtioBuffer *out_id,
                                               OtioBuffer *out_error);

/** Stops sending the review commit `client_mutation_id`, and writes whether
 *  it was waiting to `out_cancelled`. */
OtioStatus otio_ext_sync_session_cancel_review(uint64_t session,
                                               const char *client_mutation_id,
                                               bool *out_cancelled,
                                               OtioBuffer *out_error);

/** Makes `participant` the playback leader. Only the host may. */
OtioStatus otio_ext_sync_session_set_leader(uint64_t session,
                                            const char *participant,
                                            OtioBuffer *out_error);

/** Mutes or unmutes `participant`. Only the host may. */
OtioStatus otio_ext_sync_session_set_muted(uint64_t session,
                                           const char *participant, bool muted,
                                           OtioBuffer *out_error);

/** Sends a message of type `kind` with the JSON `payload` on `channel`. */
OtioStatus otio_ext_sync_session_send(uint64_t session, const char *channel,
                                      const char *kind, const char *payload,
                                      OtioBuffer *out_error);

/** Registers the channel `name` with the delivery rule `delivery`:
 *  `latest_wins`, `ordered` or `transient`. */
OtioStatus otio_ext_sync_session_register_channel(uint64_t session,
                                                  const char *name,
                                                  const char *delivery,
                                                  OtioBuffer *out_error);

/** Acts on the JSON `event` from `otio_ext_sync_session_poll_event` in
 *  `timeline`, and writes what it did to `out_handled` as a JSON object:
 *  `handled` (`applied`, `replaced`, `rejected`, `joined` or `ignored`),
 *  and a `rejected` event's `message`. */
OtioStatus otio_ext_sync_handle(OtioDocument *target, OtioNode timeline,
                                const char *event, OtioBuffer *out_handled,
                                OtioBuffer *out_error);

#ifdef __cplusplus
}
#endif

#endif /* OTIO_EXT_SYNC_H */

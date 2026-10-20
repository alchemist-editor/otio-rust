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

#ifdef __cplusplus
}
#endif

#endif /* OTIO_EXT_SYNC_H */

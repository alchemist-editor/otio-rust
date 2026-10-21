//! Timeline sync's C interface, `otio_ext_sync_*`, declared in
//! `include/otio_ext_sync.h`.
//!
//! **Timeline sync is an otio-rust extension, not part of OpenTimelineIO.**
//! This source is compiled into `libotio` as a module of `otio-capi`, behind
//! its `ext-sync` feature, so the extension shares the library's handles,
//! buffers and statuses. Nothing here does anything until sync is enabled.

pub(crate) mod session;

use std::ffi::c_char;

use otio_ext_sync::document as sync_document;
use otio_ext_sync::{Snapshot, guid};

use crate::buffer::OtioBuffer;
use crate::handle::{OtioDocument, OtioNode, document, document_mut, text, write_out};
use crate::status::{Fault, OtioStatus, Outcome, guard};

pub use session::{
    otio_ext_sync_handle, otio_ext_sync_session_cancel_review, otio_ext_sync_session_close,
    otio_ext_sync_session_commit_review, otio_ext_sync_session_connect,
    otio_ext_sync_session_disconnected, otio_ext_sync_session_leave,
    otio_ext_sync_session_may_send, otio_ext_sync_session_new, otio_ext_sync_session_poll_event,
    otio_ext_sync_session_poll_transmit, otio_ext_sync_session_publish,
    otio_ext_sync_session_receive, otio_ext_sync_session_register_channel,
    otio_ext_sync_session_send, otio_ext_sync_session_set_leader, otio_ext_sync_session_set_muted,
    otio_ext_sync_session_set_playhead, otio_ext_sync_session_set_selection,
    otio_ext_sync_session_state, otio_ext_sync_session_timeline,
};

/// Turns timeline sync on. The same as `otio_ext_enable("sync")`.
///
/// Enabling it again does nothing and succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_enable(out_error: *mut OtioBuffer) -> OtioStatus {
    guard(out_error, || crate::ext::enable(otio_ext_sync::ID))
}

/// Writes whether timeline sync is on to `out_enabled`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_is_enabled(
    out_enabled: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || unsafe {
        write_out(
            out_enabled,
            otio_ext::is_enabled(otio_ext_sync::ID),
            "out_enabled",
        )
    })
}

/// Refuses every sync call but turning it on until sync is enabled.
fn enabled() -> Outcome<()> {
    if otio_ext::is_enabled(otio_ext_sync::ID) {
        Ok(())
    } else {
        Err(Fault::new(
            OtioStatus::PluginError,
            "timeline sync is not enabled: call otio_ext_sync_enable() first",
        ))
    }
}

fn sync_fault(error: &otio_ext_sync::Error) -> Fault {
    let status = match error {
        otio_ext_sync::Error::InvalidJson(_) | otio_ext_sync::Error::Decode(_) => {
            OtioStatus::ParseError
        }
        otio_ext_sync::Error::Document(_) => OtioStatus::CoreError,
        _ => OtioStatus::InvalidArgument,
    };
    Fault::new(status, error.to_string())
}

fn snapshot_from(json: &str) -> Outcome<Snapshot> {
    Snapshot::from_json(json).map_err(|error| sync_fault(&error))
}

fn patches_from(json: &str) -> Outcome<Vec<otio_ext_sync::Patch>> {
    otio_ext_sync::patch::from_json(json).map_err(|error| sync_fault(&error))
}

/// Writes a new GUID, a version 4 UUID, to `out_guid`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_new_guid(
    out_guid: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let made = guid::new_guid();
        unsafe { write_out(out_guid, OtioBuffer::from_str(&made), "out_guid") }
    })
}

/// Writes the GUID `node` carries in `metadata.sync.guid` to `out_guid`.
///
/// Reports `OTIO_STATUS_NO_VALUE` for an object that carries none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_guid(
    source: *const OtioDocument,
    node: OtioNode,
    out_guid: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        source.try_get(node.to_id())?;
        let found = guid::guid(source, node.to_id()).ok_or_else(|| Fault::no_value("a GUID"))?;
        unsafe { write_out(out_guid, OtioBuffer::from_str(found), "out_guid") }
    })
}

/// Gives `node` the GUID `guid`, replacing any it had.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_set_guid(
    target: *mut OtioDocument,
    node: OtioNode,
    guid: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let value = unsafe { text(guid, "guid") }?;
        guid::set_guid(target, node.to_id(), value)?;
        Ok(())
    })
}

/// Gives every object sync addresses under `timeline` a GUID, if it lacks
/// one or shares one with an object met earlier, and writes how many it gave
/// one to to `out_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_stamp(
    target: *mut OtioDocument,
    timeline: OtioNode,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let stamped = guid::stamp(target, timeline.to_id())?;
        unsafe { write_out(out_count, stamped.len(), "out_count") }
    })
}

/// Finds the object under `timeline` that carries the GUID `guid`, and
/// writes it to `out_node`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when none does, and
/// `OTIO_STATUS_INVALID_ARGUMENT` when two do; stamp first to rule that out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_find(
    source: *const OtioDocument,
    timeline: OtioNode,
    guid: *const c_char,
    out_node: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let wanted = unsafe { text(guid, "guid") }?;
        source.try_get(timeline.to_id())?;
        let index = guid::GuidIndex::build(source, timeline.to_id())
            .map_err(|error| Fault::invalid(error.to_string()))?;
        let found = index
            .get(wanted)
            .ok_or_else(|| Fault::no_value("an object with that GUID"))?;
        unsafe { write_out(out_node, OtioNode::from_id(found), "out_node") }
    })
}

/// Works out the patches that take the snapshot `old_snapshot` to the
/// snapshot `new_snapshot`, as Furnace's `otiodiff` does, and writes them to
/// `out_patches` as a JSON list.
///
/// A snapshot is JSON as Furnace sends one: `sequenceId`, `generation`, and
/// the timeline as `otioJson`. With `pretty`, the list is indented as
/// Furnace's golden files are; otherwise it is compact. A change too large
/// for patches, or one between sequences or back in generation, is one
/// `replace_timeline` patch; `otio_ext_sync_diff_stats` says why.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_diff(
    old_snapshot: *const c_char,
    new_snapshot: *const c_char,
    pretty: bool,
    out_patches: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let old = snapshot_from(unsafe { text(old_snapshot, "old_snapshot") }?)?;
        let new = snapshot_from(unsafe { text(new_snapshot, "new_snapshot") }?)?;
        let (patches, _) = otio_ext_sync::diff(&old, &new).map_err(|error| sync_fault(&error))?;
        let json = if pretty {
            otio_ext_sync::patch::to_json_pretty(&patches)
        } else {
            otio_ext_sync::patch::to_json(&patches)
        };
        unsafe { write_out(out_patches, OtioBuffer::from_str(&json), "out_patches") }
    })
}

/// Writes what `otio_ext_sync_diff` does with the same snapshots to
/// `out_stats`, as a JSON object: `insertOps`, `moveOps`, `removeOps`,
/// `setPropertyOps`, `totalOps`, `nodeCount`, `fellBack` and
/// `fallbackReason`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_diff_stats(
    old_snapshot: *const c_char,
    new_snapshot: *const c_char,
    out_stats: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let old = snapshot_from(unsafe { text(old_snapshot, "old_snapshot") }?)?;
        let new = snapshot_from(unsafe { text(new_snapshot, "new_snapshot") }?)?;
        let (_, stats) = otio_ext_sync::diff(&old, &new).map_err(|error| sync_fault(&error))?;
        unsafe {
            write_out(
                out_stats,
                OtioBuffer::from_str(&stats.to_json()),
                "out_stats",
            )
        }
    })
}

/// Applies the JSON list of patches `patches` to the timeline JSON
/// `timeline`, as Furnace's `otiodiff` does, and writes the patched timeline
/// to `out_timeline` as compact JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_apply(
    timeline: *const c_char,
    patches: *const c_char,
    out_timeline: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let timeline = unsafe { text(timeline, "timeline") }?;
        let patches = patches_from(unsafe { text(patches, "patches") }?)?;
        let applied =
            otio_ext_sync::apply(timeline, &patches).map_err(|error| sync_fault(&error))?;
        unsafe { write_out(out_timeline, OtioBuffer::from_str(&applied), "out_timeline") }
    })
}

/// Applies the JSON list of patches `patches` to `timeline` in place.
///
/// The timeline keeps its handle, as does every synced object in it that the
/// patches keep; one they remove is stale afterwards. On failure the
/// document is left as it was.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_apply_to(
    target: *mut OtioDocument,
    timeline: OtioNode,
    patches: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let patches = patches_from(unsafe { text(patches, "patches") }?)?;
        sync_document::apply_to(target, timeline.to_id(), &patches)
            .map_err(|error| sync_fault(&error))
    })
}

/// Starts a transaction on `timeline`, which is `sequence_id` at
/// `generation`: stamps any missing GUIDs, and writes the timeline as it is
/// to `out_before` as a snapshot.
///
/// The snapshot is the transaction. Keep it, edit the timeline, and pass it
/// to `otio_ext_sync_commit` for the patches, or to
/// `otio_ext_sync_rollback` to undo the edits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_begin(
    target: *mut OtioDocument,
    timeline: OtioNode,
    sequence_id: *const c_char,
    generation: i64,
    out_before: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let sequence_id = unsafe { text(sequence_id, "sequence_id") }?;
        let transaction =
            sync_document::Transaction::begin(target, timeline.to_id(), sequence_id, generation)
                .map_err(|error| sync_fault(&error))?;
        let json = transaction.before().to_json();
        unsafe { write_out(out_before, OtioBuffer::from_str(&json), "out_before") }
    })
}

/// Ends the transaction `otio_ext_sync_begin` wrote as `before`, writing the
/// patches that take the timeline as it was to the timeline as it is, the
/// next generation, to `out_patches` as a compact JSON list.
///
/// An edit that changed nothing writes `[]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_commit(
    target: *mut OtioDocument,
    timeline: OtioNode,
    before: *const c_char,
    out_patches: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let before = snapshot_from(unsafe { text(before, "before") }?)?;
        let patches = sync_document::Transaction::resume(timeline.to_id(), before)
            .commit(target)
            .map_err(|error| sync_fault(&error))?;
        let json = otio_ext_sync::patch::to_json(&patches);
        unsafe { write_out(out_patches, OtioBuffer::from_str(&json), "out_patches") }
    })
}

/// Ends the transaction `otio_ext_sync_begin` wrote as `before`, putting
/// the timeline back as it was.
///
/// The timeline and every synced object that was in it keep their handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_rollback(
    target: *mut OtioDocument,
    timeline: OtioNode,
    before: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let before = snapshot_from(unsafe { text(before, "before") }?)?;
        sync_document::Transaction::resume(timeline.to_id(), before)
            .rollback(target)
            .map_err(|error| sync_fault(&error))
    })
}

/// Starts recording which objects any edit to the document `timeline` is in
/// touches.
///
/// The journal is the document's, so it records edits anywhere in it; the
/// timeline only says which document. Recording again does nothing and
/// succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_start_journal(
    target: *mut OtioDocument,
    timeline: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        target.try_get(timeline.to_id())?;
        if !target.is_journalling() {
            target.start_journal();
        }
        Ok(())
    })
}

/// Stops recording edits to the document `timeline` is in, and forgets what
/// was recorded since the last commit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_stop_journal(
    target: *mut OtioDocument,
    timeline: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        target.try_get(timeline.to_id())?;
        target.stop_journal();
        Ok(())
    })
}

/// Takes what the journal of the document `timeline` is in recorded since it
/// started or was last committed, runs the `document_changed` hook on the
/// document's root with it if anything changed, and writes whether anything did to `out_changed`.
///
/// The hook's arguments are `touched`, the GUIDs of the synced objects that
/// were added or may have changed, and `root_changed`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_commit_journal(
    target: *mut OtioDocument,
    timeline: OtioNode,
    out_changed: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        target.try_get(timeline.to_id())?;
        let changes = otio_ext_sync::journal::commit(target)?;
        unsafe { write_out(out_changed, !changes.is_empty(), "out_changed") }
    })
}

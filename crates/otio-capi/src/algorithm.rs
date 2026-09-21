//! The composition algorithms: trimming a track, and flattening layers into
//! one.

use crate::handle::{OtioDocument, OtioNode, document_mut, write_out};
use crate::status::{Fault, OtioStatus, guard};
use crate::time::OtioTimeRange;

/// Returns a copy of a track holding only what falls inside a span.
///
/// The copy is added to the same document and has no parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_algorithm_track_trimmed_to_range(
    target: *mut OtioDocument,
    track: OtioNode,
    trim_range: OtioTimeRange,
    out_track: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let trimmed =
            otio_core::algorithm::track_trimmed_to_range(target, track.to_id(), trim_range.into())?;
        unsafe { write_out(out_track, OtioNode::from_id(trimmed), "out_track") }
    })
}

/// Collapses a stack's tracks into one, top layer winning where it is visible.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_algorithm_flatten_stack(
    target: *mut OtioDocument,
    stack: OtioNode,
    out_track: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let flat = otio_core::algorithm::flatten_stack(target, stack.to_id())?;
        unsafe { write_out(out_track, OtioNode::from_id(flat), "out_track") }
    })
}

/// Collapses a list of tracks into one, lowest first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_algorithm_flatten_tracks(
    target: *mut OtioDocument,
    tracks: *const OtioNode,
    count: usize,
    out_track: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        if tracks.is_null() && count != 0 {
            return Err(Fault::null("tracks, when count is not zero"));
        }
        let handles: Vec<_> = if count == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(tracks, count) }
                .iter()
                .map(|node| node.to_id())
                .collect()
        };
        let target = unsafe { document_mut(target) }?;
        let flat = otio_core::algorithm::flatten_tracks(target, &handles)?;
        unsafe { write_out(out_track, OtioNode::from_id(flat), "out_track") }
    })
}

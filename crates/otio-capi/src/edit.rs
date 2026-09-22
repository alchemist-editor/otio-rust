//! The ten edit operations, for C.
//!
//! Each of these is upstream OpenTimelineIO's `otio.algorithms` operation of
//! the same name, with the same meaning and the same failure cases. An
//! operation that cannot be carried out leaves the document as it found it.

use crate::buffer::OtioBuffer;
use crate::handle::{OtioDocument, OtioNode, document_mut, optional_node};
use crate::status::{OtioStatus, guard};
use crate::time::{OtioRationalTime, OtioTimeRange};

/// Which clock a three- or four-point edit lines its media up against.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioReferencePoint {
    /// Use the media's own timing, and take as much of it as fits.
    Source = 0,
    /// Line the media up against the track, trimming it to the gap.
    Sequence = 1,
    /// Stretch or squeeze the media to fill the gap exactly.
    Fit = 2,
}

impl From<OtioReferencePoint> for otio_core::edit::ReferencePoint {
    fn from(point: OtioReferencePoint) -> Self {
        match point {
            OtioReferencePoint::Source => Self::Source,
            OtioReferencePoint::Sequence => Self::Sequence,
            OtioReferencePoint::Fit => Self::Fit,
        }
    }
}

/// Lays an item over a span of a composition, replacing what was there.
///
/// `fill_template` is the item to fill any gap the edit opens with, or
/// `otio_node_none` for a plain gap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_overwrite(
    target: *mut OtioDocument,
    item: OtioNode,
    composition: OtioNode,
    range: OtioTimeRange,
    remove_transitions: bool,
    fill_template: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::overwrite(
            target,
            item.to_id(),
            composition.to_id(),
            range.into(),
            remove_transitions,
            optional_node(fill_template),
        )?;
        Ok(())
    })
}

/// Inserts an item at an instant, pushing what follows later.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_insert(
    target: *mut OtioDocument,
    item: OtioNode,
    composition: OtioNode,
    time: OtioRationalTime,
    remove_transitions: bool,
    fill_template: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::insert(
            target,
            item.to_id(),
            composition.to_id(),
            time.into(),
            remove_transitions,
            optional_node(fill_template),
        )?;
        Ok(())
    })
}

/// Moves an item's in and out points without moving its neighbours.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_trim(
    target: *mut OtioDocument,
    item: OtioNode,
    delta_in: OtioRationalTime,
    delta_out: OtioRationalTime,
    fill_template: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::trim(
            target,
            item.to_id(),
            delta_in.into(),
            delta_out.into(),
            optional_node(fill_template),
        )?;
        Ok(())
    })
}

/// Cuts whatever sits at an instant into two.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_slice(
    target: *mut OtioDocument,
    composition: OtioNode,
    time: OtioRationalTime,
    remove_transitions: bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::slice(target, composition.to_id(), time.into(), remove_transitions)?;
        Ok(())
    })
}

/// Moves the media inside an item without moving the item.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_slip(
    target: *mut OtioDocument,
    item: OtioNode,
    delta: OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::slip(target, item.to_id(), delta.into())?;
        Ok(())
    })
}

/// Moves an item along its track, taking the time from its neighbours.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_slide(
    target: *mut OtioDocument,
    item: OtioNode,
    delta: OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::slide(target, item.to_id(), delta.into())?;
        Ok(())
    })
}

/// Moves an item's in and out points, sliding everything after it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_ripple(
    target: *mut OtioDocument,
    item: OtioNode,
    delta_in: OtioRationalTime,
    delta_out: OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::ripple(target, item.to_id(), delta_in.into(), delta_out.into())?;
        Ok(())
    })
}

/// Moves the cut between an item and its neighbour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_roll(
    target: *mut OtioDocument,
    item: OtioNode,
    delta_in: OtioRationalTime,
    delta_out: OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::roll(target, item.to_id(), delta_in.into(), delta_out.into())?;
        Ok(())
    })
}

/// Drops an item into a gap on a track, fitting it as the reference point
/// says.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_fill(
    target: *mut OtioDocument,
    item: OtioNode,
    track: OtioNode,
    track_time: OtioRationalTime,
    reference_point: OtioReferencePoint,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::fill(
            target,
            item.to_id(),
            track.to_id(),
            track_time.into(),
            reference_point.into(),
        )?;
        Ok(())
    })
}

/// Takes whatever sits at an instant out of a composition.
///
/// With `fill` set, a gap takes its place; without, what follows moves up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_edit_remove(
    target: *mut OtioDocument,
    composition: OtioNode,
    time: OtioRationalTime,
    fill: bool,
    fill_template: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        otio_core::edit::remove(
            target,
            composition.to_id(),
            time.into(),
            fill,
            optional_node(fill_template),
        )?;
        Ok(())
    })
}

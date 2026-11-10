//! Multicam: the prototype `Multicam.1` and `MulticamClip.1` schemas and
//! `Timeline.multicams`, proposed for OpenTimelineIO and not yet part of it.
//!
//! A `Multicam` is a stack with one child stack per angle, and is read back
//! as a stack. A `MulticamClip` is a cut that plays one track of one angle,
//! and is read back as an item. The definitions live on the timeline, keyed
//! the way a clip keys its media references. See
//! `docs/adr/0014-multicam-prototype.md`.

use std::ffi::c_char;

use otio_core::multicam::{self, Cut};

use crate::buffer::OtioBuffer;
use crate::composition::deliver;
use crate::handle::{
    OtioDocument, OtioNode, document, document_mut, optional_text, text, write_out,
};
use crate::status::{Fault, OtioStatus, guard};
use crate::time::{OtioRationalTime, OtioTimeRange};

/// Joins a list of problems into one buffer, a line each; empty when there
/// are none.
fn report(lines: &[String]) -> OtioBuffer {
    OtioBuffer::from_str(&lines.join("\n"))
}

/// Creates an empty multicam named `name`, with no angles yet.
///
/// A null `global_start_time` leaves it unset. Each angle is a stack added
/// as a child, and its name is its identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_new(
    target: *mut OtioDocument,
    name: *const c_char,
    global_start_time: *const OtioRationalTime,
    out_node: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let name = unsafe { optional_text(name, "name") }?.unwrap_or("");
        let start = if global_start_time.is_null() {
            None
        } else {
            Some(unsafe { *global_start_time }.into())
        };
        let target = unsafe { document_mut(target) }?;
        let created = multicam::new_multicam(target, name, start);
        unsafe { write_out(out_node, OtioNode::from_id(created), "out_node") }
    })
}

/// Writes whether `node` is a multicam to `out_is`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_is_multicam(
    source: *const OtioDocument,
    node: OtioNode,
    out_is: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let found = multicam::is_multicam(source.try_get(node.to_id())?);
        unsafe { write_out(out_is, found, "out_is") }
    })
}

/// Writes a multicam's global start time to `out_time`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_global_start_time(
    source: *const OtioDocument,
    node: OtioNode,
    out_time: *mut OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let time = multicam::global_start_time(source.try_get(node.to_id())?)
            .ok_or_else(|| Fault::no_value("the global start time"))?;
        unsafe { write_out(out_time, time.into(), "out_time") }
    })
}

/// Writes the angle stack named `name` in `node` to `out_angle`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when the multicam has no such angle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_angle(
    source: *const OtioDocument,
    node: OtioNode,
    name: *const c_char,
    out_angle: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let name = unsafe { text(name, "name") }?;
        let source = unsafe { document(source) }?;
        let angle = multicam::angle(source, node.to_id(), name)?
            .ok_or_else(|| Fault::new(OtioStatus::NoValue, format!("no angle named '{name}'")))?;
        unsafe { write_out(out_angle, OtioNode::from_id(angle), "out_angle") }
    })
}

/// Returns the tracks of `kind` in an angle, in order: the tracks a cut's
/// `angle_track` counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_angle_tracks(
    source: *const OtioDocument,
    angle: OtioNode,
    kind: *const c_char,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let kind = unsafe { text(kind, "kind") }?;
        let source = unsafe { document(source) }?;
        let tracks = multicam::angle_tracks(source, angle.to_id(), kind)?;
        unsafe { deliver(&tracks, out_nodes, capacity, out_count) }
    })
}

/// Creates a cut that plays track `angle_track` of angle `angle` in the
/// multicam the timeline keys as `multicam`, over `source_range` in that
/// multicam's time.
///
/// `angle_track` counts only the angle's tracks of the kind of the track the
/// cut is placed on, from 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_clip_new(
    target: *mut OtioDocument,
    name: *const c_char,
    multicam: *const c_char,
    angle: *const c_char,
    angle_track: i64,
    source_range: OtioTimeRange,
    out_node: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let name = unsafe { optional_text(name, "name") }?.unwrap_or("");
        let cut = Cut {
            multicam: unsafe { text(multicam, "multicam") }?.to_string(),
            angle: unsafe { text(angle, "angle") }?.to_string(),
            angle_track,
        };
        let target = unsafe { document_mut(target) }?;
        let created = multicam::new_multicam_clip(target, name, &cut, source_range.into());
        unsafe { write_out(out_node, OtioNode::from_id(created), "out_node") }
    })
}

/// Writes whether `node` is a multicam cut to `out_is`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_is_clip(
    source: *const OtioDocument,
    node: OtioNode,
    out_is: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let found = multicam::is_multicam_clip(source.try_get(node.to_id())?);
        unsafe { write_out(out_is, found, "out_is") }
    })
}

fn cut_of(source: &otio_core::Document, clip: OtioNode) -> crate::status::Outcome<Cut> {
    multicam::cut(source.try_get(clip.to_id())?)
        .ok_or_else(|| Fault::new(OtioStatus::WrongKind, "the object is not a MulticamClip"))
}

/// Writes the key of the multicam a cut plays to `out_key`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_clip_multicam(
    source: *const OtioDocument,
    clip: OtioNode,
    out_key: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let cut = cut_of(source, clip)?;
        unsafe { write_out(out_key, OtioBuffer::from_str(&cut.multicam), "out_key") }
    })
}

/// Writes the name of the angle a cut plays to `out_angle`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_clip_angle(
    source: *const OtioDocument,
    clip: OtioNode,
    out_angle: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let cut = cut_of(source, clip)?;
        unsafe { write_out(out_angle, OtioBuffer::from_str(&cut.angle), "out_angle") }
    })
}

/// Writes which of its angle's tracks a cut plays to `out_angle_track`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_clip_angle_track(
    source: *const OtioDocument,
    clip: OtioNode,
    out_angle_track: *mut i64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let cut = cut_of(source, clip)?;
        unsafe { write_out(out_angle_track, cut.angle_track, "out_angle_track") }
    })
}

/// Switches a cut to another angle, keeping its multicam and its range.
///
/// This is the edit a multicam editor makes most: the cut stays where it is
/// on the track and plays another camera.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_clip_set_angle(
    target: *mut OtioDocument,
    clip: OtioNode,
    angle: *const c_char,
    angle_track: i64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let angle = unsafe { text(angle, "angle") }?;
        let target = unsafe { document_mut(target) }?;
        let mut cut = cut_of(target, clip)?;
        angle.clone_into(&mut cut.angle);
        cut.angle_track = angle_track;
        multicam::set_cut(target, clip.to_id(), &cut)?;
        Ok(())
    })
}

/// Writes how many multicams `timeline` defines to `out_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_count(
    source: *const OtioDocument,
    timeline: OtioNode,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let count = multicam::multicams(source, timeline.to_id())?.len();
        unsafe { write_out(out_count, count, "out_count") }
    })
}

/// Writes the key at `index`, in key order, of the multicams `timeline`
/// defines to `out_key`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_key_at(
    source: *const OtioDocument,
    timeline: OtioNode,
    index: usize,
    out_key: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let all = multicam::multicams(source, timeline.to_id())?;
        let key = all.keys().nth(index).ok_or_else(|| {
            Fault::new(
                OtioStatus::InvalidArgument,
                format!("index {index} is past the {} multicams", all.len()),
            )
        })?;
        unsafe { write_out(out_key, OtioBuffer::from_str(key), "out_key") }
    })
}

/// Writes the multicam `timeline` keys as `key` to `out_multicam`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when there is none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_get(
    source: *const OtioDocument,
    timeline: OtioNode,
    key: *const c_char,
    out_multicam: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let key = unsafe { text(key, "key") }?;
        let source = unsafe { document(source) }?;
        let found = multicam::multicam(source, timeline.to_id(), key)?
            .ok_or_else(|| Fault::new(OtioStatus::NoValue, format!("no multicam keyed '{key}'")))?;
        unsafe { write_out(out_multicam, OtioNode::from_id(found), "out_multicam") }
    })
}

/// Puts `multicam` on `timeline` under `key`, replacing any already there.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_set(
    target: *mut OtioDocument,
    timeline: OtioNode,
    key: *const c_char,
    multicam: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let key = unsafe { text(key, "key") }?;
        let target = unsafe { document_mut(target) }?;
        if !multicam::is_multicam(target.try_get(multicam.to_id())?) {
            return Err(Fault::new(
                OtioStatus::WrongKind,
                "the object is not a Multicam",
            ));
        }
        multicam::set_multicam(target, timeline.to_id(), key, Some(multicam.to_id()))?;
        Ok(())
    })
}

/// Takes the multicam keyed `key` off `timeline`. A key that is not there
/// does nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_remove(
    target: *mut OtioDocument,
    timeline: OtioNode,
    key: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let key = unsafe { text(key, "key") }?;
        let target = unsafe { document_mut(target) }?;
        multicam::set_multicam(target, timeline.to_id(), key, None)?;
        Ok(())
    })
}

/// Writes the angle track a cut plays to `out_track`.
///
/// A null `kind` uses the kind of the track holding the cut. Reports
/// `OTIO_STATUS_CORE_ERROR`, naming what is missing, when the cut's multicam,
/// angle or angle track does not exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_resolve(
    source: *const OtioDocument,
    timeline: OtioNode,
    clip: OtioNode,
    kind: *const c_char,
    out_track: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let kind = unsafe { optional_text(kind, "kind") }?;
        let source = unsafe { document(source) }?;
        let resolved = multicam::resolve(source, timeline.to_id(), clip.to_id(), kind)?;
        unsafe { write_out(out_track, OtioNode::from_id(resolved.track), "out_track") }
    })
}

/// Checks a timeline's multicams and writes what is wrong to `out_report`,
/// a line each; the report is empty when they are valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_validate(
    source: *const OtioDocument,
    timeline: OtioNode,
    out_report: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let problems = multicam::validate(source, timeline.to_id())?;
        unsafe { write_out(out_report, report(&problems), "out_report") }
    })
}

/// Replaces every cut on `timeline` with the material it plays, then removes
/// the multicam definitions, so that a reader without multicam support sees
/// the same edit.
///
/// Writes what flattening lost to `out_report`, a line each: transitions
/// across an angle change, effects on a cut that became several items.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_multicam_flatten(
    target: *mut OtioDocument,
    timeline: OtioNode,
    out_report: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        let lost = multicam::flatten(target, timeline.to_id())?;
        unsafe { write_out(out_report, report(&lost), "out_report") }
    })
}

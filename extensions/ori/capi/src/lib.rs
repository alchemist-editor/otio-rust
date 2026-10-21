//! ORI Annotations' C interface, `otio_ext_ori_*`, declared in
//! `include/otio_ext_ori.h`.
//!
//! **ORI Annotations is an otio-rust extension, not part of
//! OpenTimelineIO.** This source is compiled into `libotio` as a module of
//! `otio-capi`, behind its `ext-ori` feature, so the extension shares the
//! library's handles, buffers and statuses. Nothing here does anything until
//! the extension is enabled.
//!
//! Commands cross the boundary as JSON: a list of ORI objects, each with its
//! `OTIO_SCHEMA`, exactly as an annotation clip's `annotation_commands` holds
//! them in a file. The tracks and clips they live on are ordinary objects.
//!
//! Times and points cross as plain numbers rather than the core's structs,
//! which every SDK writes into the core's module and not an extension's.

use std::ffi::c_char;

use otio_core::NodeId;
use otio_ext_ori_annotations::annotations::{self, Annotation};
use otio_ext_ori_annotations::{command, coords};

use crate::buffer::OtioBuffer;
use crate::composition::deliver;
use crate::handle::{OtioDocument, OtioNode, document, document_mut, text, write_out};
use crate::status::{Fault, OtioStatus, Outcome, guard};

/// Turns ORI Annotations on, and timeline sync which it is built on. The
/// same as `otio_ext_enable("ori-annotations")`.
///
/// Enabling it again does nothing and succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_enable(out_error: *mut OtioBuffer) -> OtioStatus {
    guard(out_error, || {
        crate::ext::enable(otio_ext_ori_annotations::ID)
    })
}

/// Writes whether ORI Annotations is on to `out_enabled`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_is_enabled(
    out_enabled: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || unsafe {
        write_out(
            out_enabled,
            otio_ext_ori_annotations::is_enabled(),
            "out_enabled",
        )
    })
}

/// Refuses every call but turning it on until the extension is enabled.
fn enabled() -> Outcome<()> {
    if otio_ext_ori_annotations::is_enabled() {
        Ok(())
    } else {
        Err(Fault::new(
            OtioStatus::PluginError,
            "ORI Annotations is not enabled: call otio_ext_ori_enable() first",
        ))
    }
}

fn ori_fault(error: otio_ext_ori_annotations::Error) -> Fault {
    use otio_ext_ori_annotations::Error;
    match error {
        Error::Core(error) => error.into(),
        Error::Json(_) => Fault::new(OtioStatus::ParseError, error.to_string()),
        Error::WrongKind { .. } => Fault::new(OtioStatus::WrongKind, error.to_string()),
        Error::NotEnabled => Fault::new(OtioStatus::PluginError, error.to_string()),
        _ => Fault::invalid(error.to_string()),
    }
}

/// Finds the annotation track of `timeline`, the first track whose name
/// contains "annotation" in any case, and writes it to `out_track`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_find_track(
    source: *const OtioDocument,
    timeline: OtioNode,
    out_track: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let found = annotations::find_track(source, timeline.to_id())
            .map_err(ori_fault)?
            .ok_or_else(|| {
                Fault::new(OtioStatus::NoValue, "the timeline has no annotation track")
            })?;
        unsafe { write_out(out_track, OtioNode::from_id(found), "out_track") }
    })
}

/// Writes the annotation track of `timeline` to `out_track`, first adding a
/// video track named `Annotations`, with a new sync GUID, on top of its
/// stack if it has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_ensure_track(
    target: *mut OtioDocument,
    timeline: OtioNode,
    out_track: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let track = annotations::ensure_track(target, timeline.to_id()).map_err(ori_fault)?;
        unsafe { write_out(out_track, OtioNode::from_id(track), "out_track") }
    })
}

/// Finds the annotation clip on `track` for the media clip whose sync GUID
/// is `clip_guid`, at source frame `frame`, and writes it to `out_clip`.
///
/// Reports `OTIO_STATUS_NO_VALUE` when there is none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_find_clip(
    source: *const OtioDocument,
    track: OtioNode,
    clip_guid: *const c_char,
    frame: i64,
    out_clip: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let clip_guid = unsafe { text(clip_guid, "clip_guid") }?;
        let found = annotations::find_clip(source, track.to_id(), clip_guid, frame)
            .map_err(ori_fault)?
            .ok_or_else(|| {
                Fault::new(
                    OtioStatus::NoValue,
                    "the track has no annotation clip for that clip and frame",
                )
            })?;
        unsafe { write_out(out_clip, OtioNode::from_id(found), "out_clip") }
    })
}

/// Writes the annotation clip on `track` for `clip_guid` at the time `value`
/// at `rate` to `out_clip`, first adding it if there is none: one frame long
/// at that time, at the end of the track, after a gap if the time is later
/// than the track's end, as ORI adds one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_ensure_clip(
    target: *mut OtioDocument,
    track: OtioNode,
    clip_guid: *const c_char,
    value: f64,
    rate: f64,
    out_clip: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let clip_guid = unsafe { text(clip_guid, "clip_guid") }?;
        let time = opentime::RationalTime::new(value, rate);
        let clip =
            annotations::ensure_clip(target, track.to_id(), clip_guid, time).map_err(ori_fault)?;
        unsafe { write_out(out_clip, OtioNode::from_id(clip), "out_clip") }
    })
}

/// Returns the object holding each annotation on `timeline`: an annotation
/// clip, or an `AnnotationEffect.1` on a clip, in the order the timeline is
/// walked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_list(
    source: *const OtioDocument,
    timeline: OtioNode,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let found = annotations::list(source, timeline.to_id()).map_err(ori_fault)?;
        unsafe { deliver(&holders(&found), out_nodes, capacity, out_count) }
    })
}

/// Returns the object holding each annotation of the media clip whose sync
/// GUID is `clip_guid`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_list_for_clip(
    source: *const OtioDocument,
    timeline: OtioNode,
    clip_guid: *const c_char,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let clip_guid = unsafe { text(clip_guid, "clip_guid") }?;
        let found =
            annotations::for_clip(source, timeline.to_id(), clip_guid).map_err(ori_fault)?;
        unsafe { deliver(&holders(&found), out_nodes, capacity, out_count) }
    })
}

/// Returns the object holding each annotation of the media clip whose sync
/// GUID is `clip_guid` at source frame `frame`. An `AnnotationEffect.1`
/// covers every frame of its clip.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_list_at_frame(
    source: *const OtioDocument,
    timeline: OtioNode,
    clip_guid: *const c_char,
    frame: i64,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let clip_guid = unsafe { text(clip_guid, "clip_guid") }?;
        let found =
            annotations::at_frame(source, timeline.to_id(), clip_guid, frame).map_err(ori_fault)?;
        unsafe { deliver(&holders(&found), out_nodes, capacity, out_count) }
    })
}

fn holders(found: &[Annotation]) -> Vec<NodeId> {
    found.iter().map(|annotation| annotation.holder).collect()
}

/// Writes what `holder`, an annotation clip or an `AnnotationEffect.1`,
/// says about its annotation to `out_json`, as a JSON object: `form`
/// (`track` or `effect`), `clip_guid` (the annotated clip's sync GUID, or
/// `null`), `clip_name` (the clip's name where only that is known, as in
/// ORI's review export, or `null`), `frame` (`{"value", "rate"}`, or `null`
/// for an effect), `note` and `status` (text or `null`), and `command_count`.
///
/// Reports `OTIO_STATUS_INVALID_ARGUMENT` for an object that holds no
/// annotation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_describe(
    source: *const OtioDocument,
    holder: OtioNode,
    out_json: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let annotation = annotations::describe(source, holder.to_id()).map_err(|error| {
            match error {
                // An object that holds no annotation is a bad argument here,
                // as the header says, not an object of the wrong kind.
                otio_ext_ori_annotations::Error::WrongKind { .. } => {
                    Fault::invalid(error.to_string())
                }
                _ => ori_fault(error),
            }
        })?;
        let json = annotation.to_json(source);
        unsafe { write_out(out_json, OtioBuffer::from_str(&json), "out_json") }
    })
}

/// Writes the commands `holder` holds to `out_json`, as a JSON list of ORI
/// objects, indented as OTIO files are with `pretty` and compact without.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_commands(
    source: *const OtioDocument,
    holder: OtioNode,
    pretty: bool,
    out_json: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let source = unsafe { document(source) }?;
        let ids = annotations::commands(source, holder.to_id()).map_err(ori_fault)?;
        let indent = pretty.then_some(otio_core::DEFAULT_INDENT);
        let json = command::to_json(source, &ids, indent).map_err(ori_fault)?;
        unsafe { write_out(out_json, OtioBuffer::from_str(&json), "out_json") }
    })
}

fn commands_from(json: *const c_char, what: &str) -> Outcome<Vec<command::Command>> {
    let json = unsafe { text(json, what) }?;
    command::from_json(json).map_err(ori_fault)
}

/// Adds the commands in `commands_json`, a JSON list of ORI objects, at the
/// end of the annotation clip `clip`'s commands.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_add_commands(
    target: *mut OtioDocument,
    clip: OtioNode,
    commands_json: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let commands = commands_from(commands_json, "commands_json")?;
        annotations::add_commands(target, clip.to_id(), &commands).map_err(ori_fault)?;
        Ok(())
    })
}

/// Replaces every command of `clip` whose `uuid` is `uuid`, such as one
/// whole stroke, with the commands in `commands_json`, put where the first
/// was, and writes whether any had that uuid to `out_replaced`. If none did,
/// nothing changes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_replace_commands(
    target: *mut OtioDocument,
    clip: OtioNode,
    uuid: *const c_char,
    commands_json: *const c_char,
    out_replaced: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let uuid = unsafe { text(uuid, "uuid") }?;
        let commands = commands_from(commands_json, "commands_json")?;
        let replaced = annotations::replace_commands(target, clip.to_id(), uuid, &commands)
            .map_err(ori_fault)?;
        unsafe { write_out(out_replaced, replaced, "out_replaced") }
    })
}

/// Removes every command of `clip` whose `uuid` is in `uuids_json`, a JSON
/// list of strings, and writes how many it removed to `out_count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_remove_commands(
    target: *mut OtioDocument,
    clip: OtioNode,
    uuids_json: *const c_char,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let json = unsafe { text(uuids_json, "uuids_json") }?;
        let parsed = otio_core::json::parse(json)
            .map_err(|error| Fault::new(OtioStatus::ParseError, error.to_string()))?;
        let uuids = parsed
            .as_array()
            .and_then(|values| {
                values
                    .iter()
                    .map(otio_core::json::Value::as_str)
                    .collect::<Option<Vec<&str>>>()
            })
            .ok_or_else(|| {
                Fault::new(
                    OtioStatus::ParseError,
                    "uuids_json must be a JSON list of strings",
                )
            })?;
        let removed =
            annotations::remove_commands(target, clip.to_id(), &uuids).map_err(ori_fault)?;
        unsafe { write_out(out_count, removed, "out_count") }
    })
}

/// Replaces all of `clip`'s commands with those in `commands_json`, as ORI's
/// `REPLACE_ANNOTATION_COMMANDS` does. An empty list, `[]`, clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_set_commands(
    target: *mut OtioDocument,
    clip: OtioNode,
    commands_json: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let target = unsafe { document_mut(target) }?;
        let commands = commands_from(commands_json, "commands_json")?;
        annotations::set_commands(target, clip.to_id(), &commands).map_err(ori_fault)?;
        Ok(())
    })
}

/// Writes a new uuid for a command, a version 4 UUID, to `out_uuid`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_new_uuid(
    out_uuid: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let made = otio_ext_sync::guid::new_guid();
        unsafe { write_out(out_uuid, OtioBuffer::from_str(&made), "out_uuid") }
    })
}

/// Writes the current time in ISO 8601, for a command's `timestamp`, to
/// `out_timestamp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_timestamp_now(
    out_timestamp: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let now = command::timestamp_now();
        unsafe { write_out(out_timestamp, OtioBuffer::from_str(&now), "out_timestamp") }
    })
}

/// Converts the pixel position `px`, `py`, counted from the top left of an
/// image `width` by `height` pixels, to ORI's height-normalised,
/// centre-origin, Y-up coordinates, and writes them to `out_x` and `out_y`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_pixels_to_ori(
    width: f64,
    height: f64,
    px: f64,
    py: f64,
    out_x: *mut f64,
    out_y: *mut f64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let (x, y) = coords::ImageSize::new(width, height).pixels_to_ori(px, py);
        unsafe { write_point(out_x, out_y, x, y) }
    })
}

/// Converts the point `x`, `y`, in ORI's coordinates, to a pixel position
/// counted from the top left of an image `width` by `height` pixels,
/// unrounded, and writes it to `out_px` and `out_py`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_ori_to_pixels(
    width: f64,
    height: f64,
    x: f64,
    y: f64,
    out_px: *mut f64,
    out_py: *mut f64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let (px, py) = coords::ImageSize::new(width, height).ori_to_pixels(x, y);
        unsafe { write_point(out_px, out_py, px, py) }
    })
}

/// Writes half the width of an image `width` by `height` pixels in ORI's
/// units, `width / (2 * height)`, to `out_half`: ORI's 16:9 default, 8/9,
/// when the height is not positive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_ori_aspect_half(
    width: f64,
    height: f64,
    out_half: *mut f64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        unsafe { write_out(out_half, coords::aspect_half(width, height), "out_half") }
    })
}

/// Converts the point `x`, `y`, in ORI's coordinates, to OpenTimelineIO
/// image space within a picture's `available_image_bounds`, the box from
/// `min_x`, `min_y` to `max_x`, `max_y`, whose centre is ORI's origin and
/// whose height is ORI's unit, and writes it to `out_x` and `out_y`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn otio_ext_ori_ori_to_bounds(
    x: f64,
    y: f64,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    out_x: *mut f64,
    out_y: *mut f64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let mapped = coords::ori_to_bounds(x, y, bounds(min_x, min_y, max_x, max_y));
        unsafe { write_point(out_x, out_y, mapped.x, mapped.y) }
    })
}

/// Converts the point `x`, `y`, in OpenTimelineIO image space, to ORI's
/// coordinates within a picture's `available_image_bounds`, the box from
/// `min_x`, `min_y` to `max_x`, `max_y`, and writes them to `out_x` and
/// `out_y`. The inverse of `otio_ext_ori_ori_to_bounds`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn otio_ext_ori_bounds_to_ori(
    x: f64,
    y: f64,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    out_x: *mut f64,
    out_y: *mut f64,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        enabled()?;
        let point = otio_core::V2d::new(x, y);
        let (x, y) = coords::bounds_to_ori(point, bounds(min_x, min_y, max_x, max_y));
        unsafe { write_point(out_x, out_y, x, y) }
    })
}

fn bounds(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> otio_core::Box2d {
    otio_core::Box2d::new(
        otio_core::V2d::new(min_x, min_y),
        otio_core::V2d::new(max_x, max_y),
    )
}

/// Writes both halves of a point, or neither if either pointer is missing.
unsafe fn write_point(out_x: *mut f64, out_y: *mut f64, x: f64, y: f64) -> Outcome<()> {
    if out_y.is_null() {
        return Err(Fault::invalid("out_y is null"));
    }
    unsafe { write_out(out_x, x, "out_x") }?;
    unsafe { write_out(out_y, y, "out_y") }
}

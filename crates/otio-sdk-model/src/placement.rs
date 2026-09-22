//! What an editing call does with the objects handed to it.
//!
//! # Why this cannot be read off the signature
//!
//! Every object crossing the C ABI is an `OtioNode`, whatever the call means
//! to do with it, and every handle is only meaningful inside the document
//! that issued it. A binding that hides the document — which upstream's own
//! APIs do, so ours do too — therefore has to decide, for each object
//! argument, one of two things: move this object into the receiver's
//! document first, or insist it is already there.
//!
//! Neither default is safe, and both fail quietly.
//!
//! Moving an object that was only going to be *named* swallows the timeline
//! it came from: `v1.detachChild(clipFromAnotherTimeline)` absorbs that whole
//! timeline and then deletes the clip out of it, reporting success. Refusing
//! an object that was going to be *placed* breaks appending, which is the
//! one thing every binding has to be able to do.
//!
//! # Why it is per parameter
//!
//! There is no convention in the naming to read it from. `child` is placed
//! by `otio_composition_append_child` and only named by
//! `otio_composition_detach_child`; `item` is placed by `otio_edit_insert`
//! and only named by `otio_edit_trim`. And one call can want both:
//! `otio_edit_insert` places `item` and `fill_template` into a `composition`
//! that has to be there already.
//!
//! So it is declared here, once, and every backend reads
//! [`Param::placement`](crate::model::Param::placement) rather than deciding
//! for itself. A backend that still exposes the document — Zig, deliberately
//! — can ignore it and refuse everything, which is what having no choice to
//! make looks like.
//!
//! # Which document the call works in
//!
//! Hiding the document raises a second question the same backends would
//! otherwise each answer for themselves: the C call wants a document and the
//! caller no longer supplies one, so it has to come off one of the objects.
//! Which one is not free choice. An object the call `Require`s cannot move,
//! so the call has to happen where *it* already is; an object the call
//! `Adopt`s moves, so anchoring on it would ask every other object to come
//! to the newcomer instead — and `track.insert(newClip)` would refuse the
//! track, which is the one thing hiding the document was for.
//!
//! So the anchor is the receiver where there is one, and otherwise the first
//! object the call requires to be present already.
//! [`Param::anchor`](crate::model::Param::anchor) marks it.

use crate::model::{Param, ParamRole, Placement, Type};
use crate::scan::{ScanError, Scanned};

/// What each editing call does with the objects handed to it, by entry point
/// and C parameter name.
///
/// A call that only asks questions places nothing, so it needs no entry.
const PLACEMENTS: &[(&str, &str, Placement)] = &[
    ("otio_algorithm_flatten_stack", "stack", Placement::Require),
    (
        "otio_algorithm_flatten_tracks",
        "tracks",
        Placement::Require,
    ),
    (
        "otio_algorithm_track_trimmed_to_range",
        "track",
        Placement::Require,
    ),
    (
        "otio_clip_set_media_reference",
        "reference",
        Placement::Adopt,
    ),
    ("otio_composition_append_child", "child", Placement::Adopt),
    ("otio_composition_detach_child", "child", Placement::Require),
    ("otio_composition_insert_child", "child", Placement::Adopt),
    ("otio_composition_neighbors_of", "child", Placement::Require),
    ("otio_document_deep_clone", "node", Placement::Require),
    ("otio_document_remove", "node", Placement::Require),
    ("otio_document_remove_recursive", "node", Placement::Require),
    ("otio_document_set_root", "node", Placement::Adopt),
    ("otio_edit_fill", "item", Placement::Adopt),
    ("otio_edit_fill", "track", Placement::Require),
    ("otio_edit_insert", "composition", Placement::Require),
    ("otio_edit_insert", "fill_template", Placement::Adopt),
    ("otio_edit_insert", "item", Placement::Adopt),
    ("otio_edit_overwrite", "composition", Placement::Require),
    ("otio_edit_overwrite", "fill_template", Placement::Adopt),
    ("otio_edit_overwrite", "item", Placement::Adopt),
    ("otio_edit_remove", "composition", Placement::Require),
    ("otio_edit_remove", "fill_template", Placement::Adopt),
    ("otio_edit_ripple", "item", Placement::Require),
    ("otio_edit_roll", "item", Placement::Require),
    ("otio_edit_slice", "composition", Placement::Require),
    ("otio_edit_slide", "item", Placement::Require),
    ("otio_edit_slip", "item", Placement::Require),
    ("otio_edit_trim", "fill_template", Placement::Adopt),
    ("otio_edit_trim", "item", Placement::Require),
    ("otio_item_append_effect", "effect_handle", Placement::Adopt),
    ("otio_item_append_marker", "marker_handle", Placement::Adopt),
    ("otio_metadata_set_object", "value", Placement::Adopt),
    ("otio_timeline_set_tracks", "tracks", Placement::Adopt),
];

/// Works out what one call does with one object argument.
///
/// Two answers need no table. A call holding the document as `*const` cannot
/// put anything into it, so there is nothing to decide. And the object a
/// call is *about* — the receiver — is never the object being placed:
/// `otio_item_append_effect` puts the effect in the item, not the item in
/// anything. That is a convention across the whole interface rather than a
/// hundred near-identical rows.
fn placement_of(symbol: &str, param: &Param, mutates: bool) -> Option<Placement> {
    if !mutates || param.role == ParamRole::Receiver {
        return Some(Placement::Require);
    }
    PLACEMENTS
        .iter()
        .find(|(function, name, _)| *function == symbol && *name == param.name)
        .map(|(_, _, placement)| *placement)
}

/// Whether a parameter is an object the caller hands over.
fn is_object(param: &Param) -> bool {
    matches!(param.role, ParamRole::Input | ParamRole::Receiver)
        && match &param.ty {
            Type::Node => true,
            Type::List(inner) => **inner == Type::Node,
            _ => false,
        }
}

/// Fills in the placement of every object argument of one call.
///
/// # Errors
///
/// Fails for an editing call with an object argument that [`PLACEMENTS`] does
/// not mention. That is deliberate: both defaults are wrong in a way nothing
/// downstream would catch, so a function added to the C ABI in this shape
/// stops the build rather than reaching five SDKs with a guess in it.
pub fn annotate(symbol: &str, params: &mut [Param]) -> Scanned<()> {
    let mutates = params
        .iter()
        .any(|param| param.role == ParamRole::DocumentMut);
    for param in params.iter_mut() {
        if !is_object(param) {
            continue;
        }
        param.placement = Some(
            placement_of(symbol, param, mutates).ok_or_else(|| ScanError {
                location: "crates/otio-sdk-model/src/placement.rs".to_string(),
                message: format!(
                    "`{symbol}` edits the document and takes an object as `{}`, and PLACEMENTS \
                     does not say what it does with it. Add an entry: Adopt if the call puts the \
                     object in the document, Require if the object has to be there already. \
                     Guessing is not safe — adopting where the call meant to name swallows \
                     another timeline, and naming where it meant to place breaks appending.",
                    param.name
                ),
            })?,
        );
    }
    if let Some(index) = anchor_of(params) {
        params[index].anchor = true;
    }
    Ok(())
}

/// Which object argument's document the call is made in.
///
/// The receiver, when the call has one: `track.append_child(clip)` happens
/// where the track is. Otherwise the first object the call requires to be
/// there already, because that is the one that cannot be moved to meet the
/// others. `otio_edit_insert` is the case that makes the rule worth writing
/// down: it takes an `item` it adopts and a `composition` it requires, and
/// anchoring on the item would put the call in the new item's own document
/// and then refuse the composition for being somewhere else.
///
/// A call taking no object at all has no anchor. Those are the constructors
/// and the whole-document calls, and a binding that hides the document makes
/// or holds one for them rather than reading it off an argument.
fn anchor_of(params: &[Param]) -> Option<usize> {
    // A single object the call accepts as absent cannot say where the call
    // happens, since it may not be there. A *list* marked the same way can:
    // that mark means the pointer may be null, and the call is still about
    // the tracks it is given.
    let usable = |param: &Param| !param.optional || matches!(param.ty, Type::List(_));
    let objects = || {
        params
            .iter()
            .enumerate()
            .filter(|(_, param)| is_object(param) && usable(param))
    };
    objects()
        .find(|(_, param)| param.role == ParamRole::Receiver)
        .or_else(|| objects().find(|(_, param)| param.placement == Some(Placement::Require)))
        .or_else(|| objects().next())
        .map(|(index, _)| index)
}

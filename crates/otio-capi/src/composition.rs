//! Where things sit in time, and the tree they sit in.
//!
//! # Lists
//!
//! Several of these calls answer with a list of objects whose length is not
//! known in advance. They all work the same way: pass a buffer and its
//! capacity, and `out_count` is set to how many there really are, whether or
//! not they fit. A caller that does not know the size calls once with a
//! capacity of zero and a null buffer, allocates, and calls again.

use otio_core::{Node, NodeId};

use crate::buffer::OtioBuffer;
use crate::handle::{OtioDocument, OtioNode, document, document_mut, write_out};
use crate::node::{OtioNodeKind, kind_of};
use crate::status::{Fault, OtioStatus, Outcome, guard};
use crate::time::{OtioRationalTime, OtioTimeRange};

/// What to do about a transition at the very start or end of a track when
/// asking for its neighbours.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioNeighborGapPolicy {
    /// Report no neighbour, which is the literal truth.
    Never = 0,
    /// Report a gap the length of the transition's overhang.
    AroundTransitions = 1,
}

impl From<OtioNeighborGapPolicy> for otio_core::NeighborGapPolicy {
    fn from(policy: OtioNeighborGapPolicy) -> Self {
        match policy {
            OtioNeighborGapPolicy::Never => Self::Never,
            OtioNeighborGapPolicy::AroundTransitions => Self::AroundTransitions,
        }
    }
}

/// How far a transition reaches on each side, where it reaches at all.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioHandles {
    /// Whether there is a handle before the child.
    pub has_before: bool,
    /// How much media there is before the child's start.
    pub before: OtioRationalTime,
    /// Whether there is a handle after the child.
    pub has_after: bool,
    /// How much media there is after the child's end.
    pub after: OtioRationalTime,
}

/// Fills a caller's buffer with as many handles as fit, and reports the total.
unsafe fn deliver(
    found: &[NodeId],
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
) -> Outcome<()> {
    if !out_nodes.is_null() {
        for (index, id) in found.iter().take(capacity).enumerate() {
            unsafe { out_nodes.add(index).write(OtioNode::from_id(*id)) };
        }
    } else if capacity != 0 {
        return Err(Fault::null("out_nodes, when capacity is not zero"));
    }
    unsafe { write_out(out_count, found.len(), "out_count") }
}

// ---------------------------------------------------------------------------
// The tree
// ---------------------------------------------------------------------------

/// Returns how many children an object holds.
///
/// Tracks, stacks, bare compositions and serializable collections hold
/// children; anything else reports `OTIO_STATUS_CORE_ERROR`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_child_count(
    source: *const OtioDocument,
    parent: OtioNode,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let count = source.children_of(parent.to_id())?.len();
        unsafe { write_out(out_count, count, "out_count") }
    })
}

/// Returns one of an object's children.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_child_at(
    source: *const OtioDocument,
    parent: OtioNode,
    index: usize,
    out_child: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let children = source.children_of(parent.to_id())?;
        let child = children
            .get(index)
            .ok_or_else(|| Fault::invalid(format!("no child at index {index}")))?;
        unsafe { write_out(out_child, OtioNode::from_id(*child), "out_child") }
    })
}

/// Returns an object's children.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_children(
    source: *const OtioDocument,
    parent: OtioNode,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let children = source.children_of(parent.to_id())?;
        unsafe { deliver(&children, out_nodes, capacity, out_count) }
    })
}

/// Adds a child to a composition at an index.
///
/// A negative index counts from the end, as upstream's Python does. The child
/// must not already be in a composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_insert_child(
    target: *mut OtioDocument,
    parent: OtioNode,
    index: i64,
    child: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        target.insert_child(parent.to_id(), index, child.to_id())?;
        Ok(())
    })
}

/// Adds a child to the end of a composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_append_child(
    target: *mut OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        target.append_child(parent.to_id(), child.to_id())?;
        Ok(())
    })
}

/// Removes a child by index, and returns it.
///
/// The child stays in the document with no parent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_remove_child(
    target: *mut OtioDocument,
    parent: OtioNode,
    index: i64,
    out_child: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        let removed = target.remove_child(parent.to_id(), index)?;
        unsafe { write_out(out_child, OtioNode::from_id(removed), "out_child") }
    })
}

/// Removes a child by handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_detach_child(
    target: *mut OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        target.detach_child(parent.to_id(), child.to_id())?;
        Ok(())
    })
}

/// Removes every child of a composition, and returns them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_clear_children(
    target: *mut OtioDocument,
    parent: OtioNode,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        let removed = target.clear_children(parent.to_id())?;
        unsafe { deliver(&removed, out_nodes, capacity, out_count) }
    })
}

/// Returns where a child sits in its composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_index_of_child(
    source: *const OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_index: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let index = source.index_of_child(parent.to_id(), child.to_id())?;
        unsafe { write_out(out_index, index, "out_index") }
    })
}

/// Returns whether a composition holds an object directly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_has_child(
    source: *const OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_has: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let has = source.has_child(parent.to_id(), child.to_id())?;
        unsafe { write_out(out_has, has, "out_has") }
    })
}

/// Returns whether an object descends from a composition at any depth.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_is_parent_of(
    source: *const OtioDocument,
    parent: OtioNode,
    other: OtioNode,
    out_is: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let is = source.is_parent_of(parent.to_id(), other.to_id())?;
        unsafe { write_out(out_is, is, "out_is") }
    })
}

/// Returns the outermost object above this one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_highest_ancestor(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_ancestor: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let ancestor = source.highest_ancestor(node_handle.to_id())?;
        unsafe { write_out(out_ancestor, OtioNode::from_id(ancestor), "out_ancestor") }
    })
}

/// Returns every clip at or below an object, in order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_find_clips(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let found = source.find_clips(node_handle.to_id())?;
        unsafe { deliver(&found, out_nodes, capacity, out_count) }
    })
}

/// Returns every object of a kind at or below a composition.
///
/// A null `search_range` searches all of it. `shallow` stops the walk at the
/// composition's own children.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_find_children_of_kind(
    source: *const OtioDocument,
    parent: OtioNode,
    kind: OtioNodeKind,
    search_range: *const OtioTimeRange,
    shallow: bool,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let range = if search_range.is_null() {
            None
        } else {
            Some(opentime::TimeRange::from(unsafe { *search_range }))
        };
        let source = unsafe { document(source) }?;
        let wanted = kind;
        let found = source.find_children(parent.to_id(), range, shallow, &|node: &Node| {
            kind_of(node) == wanted
        })?;
        unsafe { deliver(&found, out_nodes, capacity, out_count) }
    })
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// Returns how long an object occupies its parent's timeline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_duration(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_duration: *mut OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let duration = source.duration(node_handle.to_id())?;
        unsafe { write_out(out_duration, duration.into(), "out_duration") }
    })
}

/// Returns the span of media an object could draw on, before trimming.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_available_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.available_range(node_handle.to_id())?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns the span of media an object uses, in its own clock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_trimmed_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.trimmed_range(node_handle.to_id())?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns the span of media an object shows, including what its transitions
/// reach into.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_visible_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.visible_range(node_handle.to_id())?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where an object sits in its parent's clock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_range_in_parent(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.range_in_parent(node_handle.to_id())?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where an object sits in its parent's clock, after the parent's own
/// trim.
///
/// Reports `OTIO_STATUS_NO_VALUE` when the parent's trim excludes the object
/// entirely.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_trimmed_range_in_parent(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source
            .trimmed_range_in_parent(node_handle.to_id())?
            .ok_or_else(|| Fault::no_value("the trimmed range in the parent"))?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where the child at an index sits in its composition's clock.
///
/// A negative index counts from the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_range_of_child_at_index(
    source: *const OtioDocument,
    parent: OtioNode,
    index: i64,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.range_of_child_at_index(parent.to_id(), index)?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where the child at an index sits, after the composition's own trim.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_trimmed_range_of_child_at_index(
    source: *const OtioDocument,
    parent: OtioNode,
    index: i64,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.trimmed_range_of_child_at_index(parent.to_id(), index)?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where a child sits in a composition's clock, at any depth.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_range_of_child(
    source: *const OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source.range_of_child(parent.to_id(), child.to_id())?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where a child sits after the composition's trim, at any depth.
///
/// Reports `OTIO_STATUS_NO_VALUE` when the trim excludes it entirely.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_trimmed_range_of_child(
    source: *const OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source
            .trimmed_range_of_child(parent.to_id(), child.to_id())?
            .ok_or_else(|| Fault::no_value("the trimmed range of the child"))?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Clips a range to a composition's own trim.
///
/// Reports `OTIO_STATUS_NO_VALUE` when nothing of it is left.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_trim_child_range(
    source: *const OtioDocument,
    parent: OtioNode,
    child_range: OtioTimeRange,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let range = source
            .trim_child_range(parent.to_id(), child_range.into())?
            .ok_or_else(|| Fault::no_value("what is left of the range"))?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Returns where every child of a composition sits, in one pass.
///
/// `out_nodes` and `out_ranges` are filled in step, so entry `i` of one goes
/// with entry `i` of the other. Either may be null when only the count is
/// wanted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_ranges_of_children(
    source: *const OtioDocument,
    parent: OtioNode,
    out_nodes: *mut OtioNode,
    out_ranges: *mut OtioTimeRange,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let ranges = source.range_of_all_children(parent.to_id())?;
        if capacity != 0 && (out_nodes.is_null() || out_ranges.is_null()) {
            return Err(Fault::null(
                "out_nodes and out_ranges, when capacity is not zero",
            ));
        }
        for (index, (id, range)) in ranges.iter().take(capacity).enumerate() {
            unsafe {
                out_nodes.add(index).write(OtioNode::from_id(*id));
                out_ranges.add(index).write((*range).into());
            }
        }
        unsafe { write_out(out_count, ranges.len(), "out_count") }
    })
}

/// Returns the child of a composition that covers an instant.
///
/// `shallow` stops at the composition's own children rather than descending
/// into nested ones. Reports `OTIO_STATUS_NO_VALUE` when nothing covers it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_child_at_time(
    source: *const OtioDocument,
    parent: OtioNode,
    time: OtioRationalTime,
    shallow: bool,
    out_child: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let child = source
            .child_at_time(parent.to_id(), time.into(), shallow)?
            .ok_or_else(|| Fault::no_value("a child at that time"))?;
        unsafe { write_out(out_child, OtioNode::from_id(child), "out_child") }
    })
}

/// Returns the children of a composition that touch a span.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_children_in_range(
    source: *const OtioDocument,
    parent: OtioNode,
    search_range: OtioTimeRange,
    out_nodes: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let found = source.children_in_range(parent.to_id(), search_range.into())?;
        unsafe { deliver(&found, out_nodes, capacity, out_count) }
    })
}

/// Returns how much unused media a child has on each side.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_handles_of_child(
    source: *const OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    out_handles: *mut OtioHandles,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let (before, after) = source.handles_of_child(parent.to_id(), child.to_id())?;
        let zero = OtioRationalTime {
            value: 0.0,
            rate: 1.0,
        };
        let handles = OtioHandles {
            has_before: before.is_some(),
            before: before.map_or(zero, Into::into),
            has_after: after.is_some(),
            after: after.map_or(zero, Into::into),
        };
        unsafe { write_out(out_handles, handles, "out_handles") }
    })
}

/// Returns the children on either side of one, or
/// `otio_node_none` where there is none.
///
/// With `OTIO_NEIGHBOR_GAP_AROUND_TRANSITIONS`, a transition at the head or
/// tail of a track gets a gap materialized for its overhang, which is why this
/// takes a document it may edit.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_neighbors_of(
    target: *mut OtioDocument,
    parent: OtioNode,
    child: OtioNode,
    policy: OtioNeighborGapPolicy,
    out_before: *mut OtioNode,
    out_after: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        let (before, after) =
            target.neighbors_of_mut(parent.to_id(), child.to_id(), policy.into())?;
        unsafe {
            write_out(
                out_before,
                before.map_or(OtioNode::NONE, OtioNode::from_id),
                "out_before",
            )?;
            write_out(
                out_after,
                after.map_or(OtioNode::NONE, OtioNode::from_id),
                "out_after",
            )
        }
    })
}

/// Restates an instant from one object's clock in another's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_transformed_time(
    source: *const OtioDocument,
    time: OtioRationalTime,
    from: OtioNode,
    to: OtioNode,
    out_time: *mut OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let transformed = source.transformed_time(time.into(), from.to_id(), to.to_id())?;
        unsafe { write_out(out_time, transformed.into(), "out_time") }
    })
}

/// Restates a span from one object's clock in another's.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_transformed_time_range(
    source: *const OtioDocument,
    range: OtioTimeRange,
    from: OtioNode,
    to: OtioNode,
    out_range: *mut OtioTimeRange,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let transformed = source.transformed_time_range(range.into(), from.to_id(), to.to_id())?;
        unsafe { write_out(out_range, transformed.into(), "out_range") }
    })
}

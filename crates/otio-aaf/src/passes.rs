//! The passes upstream runs over a transcribed file.
//!
//! Transcription gives AAF's structure in OTIO's objects. Three passes then
//! reshape it into what an OTIO reader expects, always in this order:
//!
//! 1. [`fix_transitions`] moves each transition's reach onto its neighbours.
//!    AAF counts a transition's length in both clips; OTIO counts it in
//!    neither, and says how far it reaches instead.
//! 2. [`Transcriber::attach_markers`] moves each marker from the slot that
//!    carries it onto the item it points at. It runs second because AAF
//!    counts marker positions without the transition offsets.
//! 3. [`crate::simplify`] collapses the nesting AAF has and OTIO does not
//!    need.
//!
//! Only the first always runs; the other two are options, on by default.

use std::io::{Read, Seek};

use opentime::TimeRange;
use otio_core::{Document, Error as OtioError, Node, NodeId};

use crate::Transcriber;
use crate::error::Result;
use crate::transcribe::int_of;

/// Upstream's `_fix_transitions`: trims the items either side of every
/// transition by the distance the transition reaches into them.
///
/// # Errors
///
/// Returns an error if an item next to a transition has no range to trim.
pub(crate) fn fix_transitions(document: &mut Document, id: NodeId) -> Result<()> {
    let node = document.try_get(id)?;
    if let Node::Timeline(timeline) = node {
        if let Some(tracks) = timeline.tracks {
            fix_transitions(document, tracks)?;
        }
        return Ok(());
    }
    let Some(children) = node.children().map(<[NodeId]>::to_vec) else {
        return Ok(());
    };
    if matches!(node, Node::Track(_)) {
        for (index, &child) in children.iter().enumerate() {
            if document.try_get(child)?.item().is_none() {
                continue;
            }
            if index > 0 {
                if let Node::Transition(before) = document.try_get(children[index - 1])? {
                    let in_offset = before.in_offset;
                    let range = document.trimmed_range(child)?;
                    set_range(
                        document,
                        child,
                        TimeRange::new(
                            range.start_time() + in_offset,
                            range.duration() - in_offset,
                        ),
                    );
                }
            }
            if index + 1 < children.len() {
                if let Node::Transition(after) = document.try_get(children[index + 1])? {
                    let out_offset = after.out_offset;
                    let range = document.trimmed_range(child)?;
                    set_range(
                        document,
                        child,
                        TimeRange::new(range.start_time(), range.duration() - out_offset),
                    );
                }
            }
        }
    }
    for child in children {
        fix_transitions(document, child)?;
    }
    Ok(())
}

/// Sets an item's source range.
fn set_range(document: &mut Document, id: NodeId, range: TimeRange) {
    if let Some(item) = document.get_mut(id).and_then(Node::item_mut) {
        item.source_range = Some(range);
    }
}

/// The timelines a pass looks at: the object itself if it is one, else those
/// in the collection, and in collections inside it.
pub(crate) fn timelines_in(document: &Document, id: NodeId) -> Vec<NodeId> {
    match document.get(id) {
        Some(Node::Timeline(_)) => vec![id],
        Some(Node::SerializableCollection(collection)) => {
            let mut out = Vec::new();
            for &child in &collection.children {
                match document.get(child) {
                    Some(Node::Timeline(_)) => out.push(child),
                    Some(Node::SerializableCollection(_)) => {
                        out.extend(timelines_in(document, child));
                    }
                    _ => {}
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Every track under a timeline, depth first.
fn tracks_under(document: &Document, timeline: NodeId) -> Result<Vec<NodeId>> {
    let Some(Node::Timeline(found)) = document.get(timeline) else {
        return Ok(Vec::new());
    };
    let Some(stack) = found.tracks else {
        return Ok(Vec::new());
    };
    Ok(document.find_children(stack, None, false, &|node| matches!(node, Node::Track(_)))?)
}

impl<R: Read + Seek> Transcriber<R> {
    /// Upstream's `_attach_markers`: moves each marker from the track that
    /// carries it to the item it points at.
    ///
    /// A marker says which slot and which physical track it belongs to.
    /// That names a track, and the marker's position names the item on it.
    /// Where the track cannot be found — Avid writes wrong track numbers
    /// when exporting only selected tracks — the marker goes on the
    /// timeline's stack, which is where DaVinci Resolve puts such markers.
    /// Where the item cannot hold a marker, a transition, or its extent
    /// cannot be worked out, the marker stays on the track.
    ///
    /// # Errors
    ///
    /// Returns an error if a marker's position cannot be restated in the
    /// coordinates of the item it moves to.
    pub(crate) fn attach_markers(&mut self, root: NodeId) -> Result<()> {
        for timeline in timelines_in(&self.document, root) {
            let tracks = tracks_under(&self.document, timeline)?;
            let mut by_slot = std::collections::HashMap::new();
            for &track in &tracks {
                let slot_id = self.meta_int(track, "SlotID");
                let track_number = self.meta_int(track, "PhysicalTrackNumber");
                if let (Some(slot_id), Some(track_number)) = (slot_id, track_number) {
                    by_slot.insert((slot_id, track_number), track);
                }
            }
            let stack = match self.document.get(timeline) {
                Some(Node::Timeline(found)) => found.tracks,
                _ => None,
            };

            for &current in &tracks {
                for marker in self.markers_of(current) {
                    let aaf = self.aaf_of(marker);
                    let key = (
                        int_of(aaf.get("AttachedSlotID")),
                        int_of(aaf.get("AttachedPhysicalTrackNumber")),
                    );
                    let target_track = match key {
                        (Some(slot), Some(track)) => by_slot.get(&(slot, track)).copied(),
                        _ => None,
                    };
                    if let Some(item) = self.document.get_mut(current).and_then(Node::item_mut) {
                        item.markers.retain(|found| *found != marker);
                    }
                    let range = self.marked_range(marker);

                    let target = match target_track {
                        None => {
                            let Some(stack) = stack else { continue };
                            let start = self.document.transformed_time(
                                range.start_time(),
                                current,
                                stack,
                            )?;
                            self.set_marked_range(marker, TimeRange::new(start, range.duration()));
                            stack
                        }
                        Some(target_track) => {
                            match self.find_child_at_time(target_track, range.start_time()) {
                                Err(error) if is_unknown_extent(&error) => target_track,
                                Err(error) => return Err(error),
                                Ok(found) => {
                                    let target = match found {
                                        Some(found) if self.can_hold_markers(found) => found,
                                        _ => target_track,
                                    };
                                    match self.document.transformed_time(
                                        range.start_time(),
                                        current,
                                        target,
                                    ) {
                                        Ok(start) => {
                                            self.set_marked_range(
                                                marker,
                                                TimeRange::new(start, range.duration()),
                                            );
                                            target
                                        }
                                        Err(OtioError::NoAvailableRange { .. }) => target_track,
                                        Err(error) => return Err(error.into()),
                                    }
                                }
                            }
                        }
                    };
                    if let Some(item) = self.document.get_mut(target).and_then(Node::item_mut) {
                        item.markers.push(marker);
                    }
                }
            }
        }
        Ok(())
    }

    /// Upstream's `_find_child_at_time`: the item at a time, looking through
    /// a transition to the item on whichever side the time falls.
    fn find_child_at_time(
        &self,
        track: NodeId,
        time: opentime::RationalTime,
    ) -> Result<Option<NodeId>> {
        let Some(found) = self.document.child_at_time(track, time, false)? else {
            return Ok(None);
        };
        if !matches!(self.document.get(found), Some(Node::Transition(_))) {
            return Ok(Some(found));
        }
        let parent = self.document.parent_of(found)?;
        let index = self.document.index_of_child(parent, found)?;
        let siblings = self.document.children_of(parent)?;
        let local = self.document.transformed_time(time, track, parent)?;
        let before = index
            .checked_sub(1)
            .and_then(|i| siblings.get(i))
            .copied()
            .ok_or(crate::Error::Malformed(
                "a transition has nothing before it",
            ))?;
        let target = if self.document.range_in_parent(before)?.contains_time(local) {
            before
        } else {
            siblings
                .get(index + 1)
                .copied()
                .ok_or(crate::Error::Malformed("a transition has nothing after it"))?
        };
        if self.document.try_get(target)?.children().is_some() {
            // Upstream restates the original time rather than the local one
            // here; kept, so markers land where they land there.
            let inner = self.document.transformed_time(time, parent, target)?;
            return self.find_child_at_time(target, inner);
        }
        Ok(Some(target))
    }

    /// Whether an object has a list of markers of its own.
    fn can_hold_markers(&self, id: NodeId) -> bool {
        self.document.get(id).and_then(Node::item).is_some()
    }

    /// A marker's range.
    pub(crate) fn marked_range(&self, marker: NodeId) -> TimeRange {
        match self.document.get(marker) {
            Some(Node::Marker(marker)) => marker.marked_range,
            _ => TimeRange::default(),
        }
    }

    /// Sets a marker's range.
    pub(crate) fn set_marked_range(&mut self, marker: NodeId, range: TimeRange) {
        if let Some(Node::Marker(marker)) = self.document.get_mut(marker) {
            marker.marked_range = range;
        }
    }
}

/// Whether an error is upstream's `CannotComputeAvailableRangeError`: a clip
/// whose media does not say how long it is.
fn is_unknown_extent(error: &crate::Error) -> bool {
    matches!(
        error,
        crate::Error::Otio(OtioError::NoAvailableRange { .. })
    )
}

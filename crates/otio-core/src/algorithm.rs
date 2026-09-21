//! Algorithms over whole compositions.
//!
//! These are upstream's `stackAlgorithm` and `trackAlgorithm`: collapsing a
//! stack of tracks into the single track a viewer would actually see, and
//! cutting a track down to a span of time.
//!
//! They take the document by mutable reference because a result is a new
//! object in it, not a value returned by copy. Anything an algorithm builds
//! along the way is removed again before it returns, so the only thing left
//! behind is the answer.

use std::collections::HashMap;

use opentime::{DEFAULT_EPSILON_S, RationalTime, TimeRange, max};

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::schema::{Gap, ItemData, Node, Track};

/// The name upstream gives the track that flattening produces.
pub const FLATTENED_TRACK_NAME: &str = "Flattened";

/// Cuts a track down to `trim_range`, returning a new track.
///
/// Children entirely outside the range are dropped, and a child straddling an
/// edge has its `source_range` pulled in to fit. The original is left
/// untouched.
///
/// # Errors
///
/// Returns [`Error::CannotTrimTransition`] if an edge falls inside a
/// transition, which has no meaning: a transition is defined by how far it
/// reaches into its neighbours.
pub fn track_trimmed_to_range(
    document: &mut Document,
    track: NodeId,
    trim_range: TimeRange,
) -> Result<NodeId> {
    let new_track = document.deep_clone(track)?;
    let ranges = document.range_of_all_children(new_track)?;
    let children = document.children_of(new_track)?;

    // Back to front, so that removing a child does not shift the index of one
    // not yet looked at.
    for (index, child) in children.iter().enumerate().rev() {
        let child_range = *ranges.get(child).ok_or(Error::InvalidTimeRange)?;

        if !trim_range.intersects(child_range, DEFAULT_EPSILON_S) {
            let removed = document.remove_child(new_track, index as i64)?;
            document.remove_recursive(removed)?;
            continue;
        }

        if trim_range.contains_range(child_range, DEFAULT_EPSILON_S) {
            continue;
        }

        if matches!(document.try_get(*child)?, Node::Transition(_)) {
            return Err(Error::CannotTrimTransition);
        }

        let mut source_range = document.trimmed_range(*child)?;
        if trim_range.start_time() > child_range.start_time() {
            let amount = trim_range.start_time() - child_range.start_time();
            source_range = TimeRange::new(
                source_range.start_time() + amount,
                source_range.duration() - amount,
            );
        }

        let trim_end = trim_range.end_time_exclusive();
        let child_end = child_range.end_time_exclusive();
        if trim_end < child_end {
            let amount = child_end - trim_end;
            source_range =
                TimeRange::new(source_range.start_time(), source_range.duration() - amount);
        }

        document
            .try_get_mut(*child)?
            .item_mut()
            .ok_or_else(|| Error::UnexpectedChild {
                schema: "Transition".to_string(),
                parent: "Track".to_string(),
            })?
            .source_range = Some(source_range);
    }

    Ok(new_track)
}

/// Collapses a stack of tracks into the single track a viewer would see.
///
/// Higher tracks cover lower ones, except where a gap or a disabled item lets
/// what is beneath show through.
///
/// # Errors
///
/// Returns [`Error::UnexpectedChild`] if the stack holds anything but tracks.
pub fn flatten_stack(document: &mut Document, stack: NodeId) -> Result<NodeId> {
    let children = document.children_of(stack)?;

    let mut tracks = Vec::with_capacity(children.len());
    for child in children {
        let node = document.try_get(child)?;
        match node {
            Node::Track(track) => {
                // A disabled track contributes nothing, so it is not even a
                // layer for the ones below to show through.
                if track.item.enabled {
                    tracks.push(child);
                }
            }
            node => {
                return Err(Error::UnexpectedChild {
                    schema: node.schema_name().to_string(),
                    parent: "Stack".to_string(),
                });
            }
        }
    }

    flatten_tracks(document, &tracks)
}

/// Collapses a list of tracks into one, lowest first.
///
/// # Errors
///
/// Propagates whatever the ranges along the way report.
pub fn flatten_tracks(document: &mut Document, tracks: &[NodeId]) -> Result<NodeId> {
    let (tracks, scratch) = normalize_track_lengths(document, tracks)?;

    let flat_track = document.insert(Node::Track(Track {
        item: ItemData {
            base: crate::schema::Base {
                name: FLATTENED_TRACK_NAME.to_string(),
                ..crate::schema::Base::default()
            },
            ..ItemData::new()
        },
        children: Vec::new(),
        kind: String::new(),
    }));

    let mut ranges = HashMap::new();
    let top = i64::try_from(tracks.len()).unwrap_or(i64::MAX) - 1;
    flatten_next_item(document, &mut ranges, flat_track, &tracks, top, None)?;

    for id in scratch {
        document.remove_recursive(id)?;
    }
    Ok(flat_track)
}

/// Pads every track out to the length of the longest.
///
/// Returns the tracks to flatten and the scratch copies to clean up
/// afterwards. A track that is already long enough is used as it is.
fn normalize_track_lengths(
    document: &mut Document,
    tracks: &[NodeId],
) -> Result<(Vec<NodeId>, Vec<NodeId>)> {
    let mut longest = RationalTime::default();
    for track in tracks {
        longest = max(longest, document.duration(*track)?);
    }

    let mut normalized = Vec::with_capacity(tracks.len());
    let mut scratch = Vec::new();
    for track in tracks {
        let duration = document.duration(*track)?;
        if duration >= longest {
            normalized.push(*track);
            continue;
        }

        // The original must not grow a gap, so pad a copy.
        let padded = document.deep_clone(*track)?;
        let gap = document.insert(Node::Gap(Gap {
            item: ItemData {
                source_range: Some(TimeRange::new(
                    RationalTime::new(0.0, (longest - duration).rate()),
                    longest - duration,
                )),
                ..ItemData::new()
            },
        }));
        document.append_child(padded, gap)?;
        normalized.push(padded);
        scratch.push(padded);
    }
    Ok((normalized, scratch))
}

/// Walks one track, copying what is visible and recursing into the track below
/// wherever something is not.
///
/// `track_index` counts down from the top track to the bottom. `trim_range`
/// is the span of the track below that a hole above has exposed.
fn flatten_next_item(
    document: &mut Document,
    ranges: &mut HashMap<NodeId, HashMap<NodeId, TimeRange>>,
    flat_track: NodeId,
    tracks: &[NodeId],
    track_index: i64,
    trim_range: Option<TimeRange>,
) -> Result<()> {
    if track_index < 0 {
        return Ok(());
    }
    let Ok(index) = usize::try_from(track_index) else {
        return Ok(());
    };
    let Some(&original) = tracks.get(index) else {
        return Ok(());
    };

    // A hole above exposes only part of this track, so work on a copy cut to
    // that part. The copy is dropped before returning; its handle going stale
    // is exactly what stops a later object reusing the slot from being
    // mistaken for it.
    let (track, scratch) = match trim_range {
        Some(range) => {
            let trimmed = track_trimmed_to_range(document, original, range)?;
            (trimmed, Some(trimmed))
        }
        None => (original, None),
    };

    if let std::collections::hash_map::Entry::Vacant(entry) = ranges.entry(track) {
        entry.insert(
            document
                .range_of_all_children(track)?
                .into_iter()
                .collect::<HashMap<_, _>>(),
        );
    }

    let children = document.children_of(track)?;

    for child in children {
        let node = document.try_get(child)?;
        let is_item = node.item().is_some();
        let is_transition = matches!(node, Node::Transition(_));
        if !is_item && !is_transition {
            return Err(Error::UnexpectedChild {
                schema: node.schema_name().to_string(),
                parent: "Track".to_string(),
            });
        }

        // A transition, anything visible, and everything on the bottom track
        // lands on the flattened track as it is. Only a hole — a gap, or a
        // disabled item — sends the search down a layer.
        if !is_item || node.visible() || track_index == 0 {
            let copy = document.deep_clone(child)?;
            document.append_child(flat_track, copy)?;
            continue;
        }

        let mut hole = *ranges
            .get(&track)
            .and_then(|map| map.get(&child))
            .ok_or(Error::InvalidTimeRange)?;
        if let Some(range) = trim_range {
            // The hole's range is stated in the trimmed copy's timeline;
            // the track below knows only the untrimmed one.
            hole = TimeRange::new(hole.start_time() + range.start_time(), hole.duration());
            if let Some(map) = ranges.get_mut(&track) {
                map.insert(child, hole);
            }
        }

        flatten_next_item(
            document,
            ranges,
            flat_track,
            tracks,
            track_index - 1,
            Some(hole),
        )?;
    }

    if let Some(scratch) = scratch {
        ranges.remove(&scratch);
        document.remove_recursive(scratch)?;
    }
    Ok(())
}

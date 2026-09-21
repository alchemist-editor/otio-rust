//! Builders for the timelines the algorithm tests work on.
//!
//! These mirror the fixtures upstream's `test_track_algo.py` and
//! `test_stack_algo.py` build, so a failure here can be read against the test
//! it was ported from.

#![allow(dead_code)]

use otio_core::schema::{Base, Clip, Gap, ItemData, Node, Stack, Track, Transition};
use otio_core::{Document, NodeId};

use opentime::{RationalTime, TimeRange};

/// The rate every fixture is stated at.
pub const RATE: f64 = 24.0;

/// Builds a time range at [`RATE`].
#[must_use]
pub fn range(start: f64, duration: f64) -> TimeRange {
    TimeRange::new(
        RationalTime::new(start, RATE),
        RationalTime::new(duration, RATE),
    )
}

/// Builds a time at [`RATE`].
#[must_use]
pub fn time(value: f64) -> RationalTime {
    RationalTime::new(value, RATE)
}

/// Adds a named clip trimmed to `start` and `duration`.
pub fn clip(document: &mut Document, name: &str, start: f64, duration: f64) -> NodeId {
    document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base {
                name: name.to_string(),
                ..Base::default()
            },
            source_range: Some(range(start, duration)),
            ..ItemData::new()
        },
        ..Clip::default()
    }))
}

/// Adds a clip that does not contribute when its stack is flattened.
pub fn disabled_clip(document: &mut Document, name: &str, start: f64, duration: f64) -> NodeId {
    let id = clip(document, name, start, duration);
    document
        .try_get_mut(id)
        .expect("just inserted")
        .item_mut()
        .expect("a clip is an item")
        .enabled = false;
    id
}

/// Adds a gap of `duration`.
pub fn gap(document: &mut Document, duration: f64) -> NodeId {
    gap_at(document, "", 0.0, duration)
}

/// Adds a named gap holding `start` to `start + duration` of its own media.
///
/// A gap shows nothing, so where its range starts never reaches the screen.
/// It still matters to the edit operations, which read it when deciding how
/// much of a clip dropped into the gap fits.
pub fn gap_at(document: &mut Document, name: &str, start: f64, duration: f64) -> NodeId {
    document.insert(Node::Gap(Gap {
        item: ItemData {
            base: Base {
                name: name.to_string(),
                ..Base::default()
            },
            source_range: Some(range(start, duration)),
            ..ItemData::new()
        },
    }))
}

/// Adds a transition reaching `in_offset` back and `out_offset` forward.
pub fn transition(document: &mut Document, in_offset: f64, out_offset: f64) -> NodeId {
    document.insert(Node::Transition(Transition {
        in_offset: time(in_offset),
        out_offset: time(out_offset),
        enabled: true,
        ..Transition::default()
    }))
}

/// Adds a named track holding `children`, in order.
pub fn track(document: &mut Document, name: &str, children: &[NodeId]) -> NodeId {
    let id = document.insert(Node::Track(Track {
        item: ItemData {
            base: Base {
                name: name.to_string(),
                ..Base::default()
            },
            ..ItemData::new()
        },
        children: Vec::new(),
        kind: "Video".to_string(),
    }));
    for child in children {
        document
            .append_child(id, *child)
            .expect("a track holds items");
    }
    id
}

/// Adds a stack holding `children`, lowest first.
pub fn stack(document: &mut Document, children: &[NodeId]) -> NodeId {
    let id = document.insert(Node::Stack(Stack::default()));
    for child in children {
        document
            .append_child(id, *child)
            .expect("a stack holds tracks");
    }
    id
}

/// Returns the name and span of every child of a composition.
///
/// This is what the upstream tests compare, and it reads better in a failure
/// message than a tree of handles. The span is stated in the child's own
/// terms, so that it does not move when the child does: for an item it is the
/// trimmed range, the part of its media in use; for a transition it is the
/// reach either side of the cut it sits on, so a 12/20 dissolve is
/// `(-12, 32)`.
#[must_use]
pub fn summarize(document: &Document, composition: NodeId) -> Vec<(String, TimeRange)> {
    document
        .children_of(composition)
        .expect("a composition")
        .into_iter()
        .map(|child| {
            let node = document.try_get(child).expect("live");
            let span = match node {
                Node::Transition(transition) => TimeRange::new(
                    -transition.in_offset,
                    transition.in_offset + transition.out_offset,
                ),
                _ => document.trimmed_range(child).expect("a trimmed item"),
            };
            (node.name().to_string(), span)
        })
        .collect()
}

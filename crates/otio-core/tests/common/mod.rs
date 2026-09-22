//! Builders for the timelines the algorithm tests work on.
//!
//! These mirror the fixtures upstream's `test_track_algo.py` and
//! `test_stack_algo.py` build, so a failure here can be read against the test
//! it was ported from.

#![allow(dead_code)]

use otio_core::schema::{
    Base, Clip, EffectData, ExternalReference, Gap, ItemData, MediaReferenceData, Node, Stack,
    Track, Transition,
};
use otio_core::{Any, Document, NodeId};

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

/// What [`hold_twice`] made a clip hold twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldTwice {
    /// The object under both metadata keys `a` and `b`.
    pub object: NodeId,
    /// The effect listed twice.
    pub effect: NodeId,
    /// The media reference under both keys `one` and `two`.
    pub reference: NodeId,
}

/// Makes a clip hold one object in each of the places an object can be held
/// twice: one object under two metadata keys, one effect listed twice, and one
/// media reference under two keys.
pub fn hold_twice(document: &mut Document, clip: NodeId) -> HeldTwice {
    let object = document.insert(Node::SerializableObjectWithMetadata(Base {
        name: "held".to_string(),
        ..Base::default()
    }));
    let effect = document.insert(Node::Effect(EffectData {
        base: Base {
            name: "fx".to_string(),
            ..Base::default()
        },
        ..EffectData::new()
    }));
    let reference = document.insert(Node::ExternalReference(ExternalReference {
        media: MediaReferenceData::default(),
        target_url: "file:///media.mov".to_string(),
    }));
    let Node::Clip(node) = document.try_get_mut(clip).expect("live") else {
        panic!("hold_twice takes a clip");
    };
    node.item
        .base
        .metadata
        .insert("a".to_string(), Any::Object(object));
    node.item
        .base
        .metadata
        .insert("b".to_string(), Any::Object(object));
    node.item.effects = vec![effect, effect];
    node.media_references = [
        ("one".to_string(), reference),
        ("two".to_string(), reference),
    ]
    .into_iter()
    .collect();
    node.active_media_reference_key = "one".to_string();
    HeldTwice {
        object,
        effect,
        reference,
    }
}

/// What a clip holds under metadata `key`.
#[must_use]
pub fn held_in_metadata(document: &Document, clip: NodeId, key: &str) -> NodeId {
    match document
        .try_get(clip)
        .expect("live")
        .base()
        .expect("has metadata")
        .metadata
        .get(key)
    {
        Some(Any::Object(id)) => *id,
        other => panic!("expected an object under {key:?}, got {other:?}"),
    }
}

/// The two holders of each thing [`hold_twice`] set up, as `clip` now has
/// them: metadata `a` and `b`, the two effects, and media references `one`
/// and `two`.
#[must_use]
pub fn held_pairs(document: &Document, clip: NodeId) -> [(NodeId, NodeId); 3] {
    let Node::Clip(node) = document.try_get(clip).expect("live") else {
        panic!("held_pairs takes a clip");
    };
    [
        (
            held_in_metadata(document, clip, "a"),
            held_in_metadata(document, clip, "b"),
        ),
        (node.item.effects[0], node.item.effects[1]),
        (node.media_references["one"], node.media_references["two"]),
    ]
}

/// Asserts that `clip` still holds each of `held` twice, as it did.
pub fn assert_held_twice(document: &Document, clip: NodeId, held: HeldTwice) {
    assert_eq!(
        held_pairs(document, clip),
        [
            (held.object, held.object),
            (held.effect, held.effect),
            (held.reference, held.reference),
        ]
    );
}

/// Asserts that `copy`, a copy of a clip [`hold_twice`] set up, holds two
/// separate copies of each thing the original held twice, as upstream's
/// `clone()` makes them, and none of the originals.
pub fn assert_copied_apart(document: &Document, copy: NodeId, original: HeldTwice) {
    let pairs = held_pairs(document, copy);
    for (first, second) in pairs {
        assert_ne!(
            first, second,
            "one object held twice should be two in the copy"
        );
    }
    let originals = [original.object, original.effect, original.reference];
    for ((first, second), original) in pairs.into_iter().zip(originals) {
        assert_ne!(first, original);
        assert_ne!(second, original);
        assert_eq!(
            document.try_get(first).unwrap().name(),
            document.try_get(original).unwrap().name()
        );
    }
}

/// How many objects `id` and everything it owns come to.
#[must_use]
pub fn owned_count(document: &Document, id: NodeId) -> usize {
    let mut pending = vec![id];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if seen.insert(id) {
            document
                .try_get(id)
                .expect("live")
                .visit_owned(&mut |owned| pending.push(owned));
        }
    }
    seen.len()
}

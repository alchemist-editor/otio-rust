// SPDX-License-Identifier: Apache-2.0
// Copyright Contributors to the OpenTimelineIO project

//! What area of picture sits behind an item.

use std::collections::BTreeMap;

use otio_core::schema::{Clip, ExternalReference, ItemData, MediaReferenceData, Stack, Track};
use otio_core::{Box2d, Document, Node, NodeId, V2d};

/// Adds a clip whose media covers `size` by `size`, or no media at all.
fn clip(document: &mut Document, size: Option<f64>) -> NodeId {
    let mut references = BTreeMap::new();
    let reference = document.insert(Node::ExternalReference(ExternalReference {
        media: MediaReferenceData {
            available_image_bounds: size
                .map(|size| Box2d::new(V2d::new(0.0, 0.0), V2d::new(size, size))),
            ..MediaReferenceData::default()
        },
        target_url: "file:///a.mov".to_string(),
    }));
    references.insert(otio_core::DEFAULT_MEDIA_KEY.to_string(), reference);
    document.insert(Node::Clip(Clip {
        item: ItemData::new(),
        media_references: references,
        active_media_reference_key: otio_core::DEFAULT_MEDIA_KEY.to_string(),
    }))
}

#[test]
fn a_clip_reports_the_bounds_of_its_active_media() {
    let mut document = Document::new();
    let clip = clip(&mut document, Some(2.0));
    assert_eq!(
        document.available_image_bounds(clip).unwrap(),
        Some(Box2d::new(V2d::new(0.0, 0.0), V2d::new(2.0, 2.0)))
    );
}

#[test]
fn a_clip_whose_media_says_nothing_reports_nothing() {
    let mut document = Document::new();
    let clip = clip(&mut document, None);
    assert_eq!(document.available_image_bounds(clip).unwrap(), None);
}

#[test]
fn a_track_unions_the_clips_sitting_on_it() {
    let mut document = Document::new();
    let track = document.insert(Node::Track(Track::default()));
    let small = clip(&mut document, Some(1.0));
    let large = clip(&mut document, Some(3.0));
    document.append_child(track, small).unwrap();
    document.append_child(track, large).unwrap();

    assert_eq!(
        document.available_image_bounds(track).unwrap(),
        Some(Box2d::new(V2d::new(0.0, 0.0), V2d::new(3.0, 3.0)))
    );
}

#[test]
fn a_track_does_not_reach_into_a_nested_track_but_a_stack_does() {
    // This is upstream's difference, not a simplification here: `Track`
    // walks its own children and `Stack` walks every clip below it.
    let mut document = Document::new();
    let outer = document.insert(Node::Track(Track::default()));
    let inner = document.insert(Node::Track(Track::default()));
    let hidden = clip(&mut document, Some(3.0));
    document.append_child(inner, hidden).unwrap();
    document.append_child(outer, inner).unwrap();

    assert_eq!(document.available_image_bounds(outer).unwrap(), None);

    let stack = document.insert(Node::Stack(Stack::default()));
    let inner = document.insert(Node::Track(Track::default()));
    let hidden = clip(&mut document, Some(3.0));
    document.append_child(inner, hidden).unwrap();
    document.append_child(stack, inner).unwrap();

    assert_eq!(
        document.available_image_bounds(stack).unwrap(),
        Some(Box2d::new(V2d::new(0.0, 0.0), V2d::new(3.0, 3.0)))
    );
}

#[test]
fn an_object_that_has_no_media_at_all_says_so() {
    let mut document = Document::new();
    let gap = document.insert(Node::Gap(otio_core::schema::Gap::default()));
    let error = document.available_image_bounds(gap).unwrap_err();
    assert!(
        error.to_string().contains("available_image_bounds"),
        "{error}"
    );
}

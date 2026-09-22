//! Where things sit in time.
//!
//! These check the range machinery against the behaviour upstream's C++
//! defines: a track lays its children end to end, a stack lays them on top of
//! one another, a transition sits over its neighbours rather than beside
//! them, and a composition's own trim clips what its children report.

mod common;

use common::{clip, gap, range, stack, time, track, transition};

use opentime::{RationalTime, TimeRange};
use otio_core::schema::{Base, SerializableCollection, Timeline};
use otio_core::{Any, Document, Error, Node, NodeId};

#[test]
fn a_track_lays_its_children_end_to_end() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let c = clip(&mut document, "C", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    assert_eq!(document.range_in_parent(a).unwrap(), range(0.0, 50.0));
    assert_eq!(document.range_in_parent(b).unwrap(), range(50.0, 50.0));
    assert_eq!(document.range_in_parent(c).unwrap(), range(100.0, 50.0));
    assert_eq!(document.duration(sequence).unwrap(), time(150.0));
}

#[test]
fn a_stack_lays_its_children_on_top_of_one_another() {
    let mut document = Document::new();
    let short = clip(&mut document, "short", 0.0, 50.0);
    let long = clip(&mut document, "long", 0.0, 150.0);
    let lower = track(&mut document, "lower", &[short]);
    let upper = track(&mut document, "upper", &[long]);
    let layers = stack(&mut document, &[lower, upper]);

    assert_eq!(
        document.range_of_child(layers, lower).unwrap(),
        range(0.0, 50.0)
    );
    assert_eq!(
        document.range_of_child(layers, upper).unwrap(),
        range(0.0, 150.0)
    );
    // A stack is as long as its longest layer.
    assert_eq!(document.duration(layers).unwrap(), time(150.0));
}

#[test]
fn a_transition_does_not_advance_the_playhead() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let dissolve = transition(&mut document, 12.0, 20.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, dissolve, b]);

    // B still starts at 50: the transition sits over the cut rather than
    // taking time of its own.
    assert_eq!(document.range_in_parent(b).unwrap(), range(50.0, 50.0));
    assert_eq!(document.duration(sequence).unwrap(), time(100.0));

    // The transition itself starts where it begins to bite into A.
    assert_eq!(
        document.range_in_parent(dissolve).unwrap(),
        range(38.0, 32.0)
    );
}

#[test]
fn a_transition_widens_the_visible_range_of_its_neighbours() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let dissolve = transition(&mut document, 12.0, 20.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    track(&mut document, "Sequence1", &[a, dissolve, b]);

    // A is on screen for its own range plus the transition's tail.
    assert_eq!(document.trimmed_range(a).unwrap(), range(0.0, 50.0));
    assert_eq!(document.visible_range(a).unwrap(), range(0.0, 70.0));

    // B is on screen from before its own start, by the transition's head.
    assert_eq!(document.visible_range(b).unwrap(), range(-12.0, 62.0));
}

#[test]
fn a_composition_trim_clips_what_its_children_report() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let c = clip(&mut document, "C", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    document
        .try_get_mut(sequence)
        .unwrap()
        .item_mut()
        .unwrap()
        .source_range = Some(range(25.0, 50.0));

    // A is half outside the trim, so only its second half is reported.
    assert_eq!(
        document.trimmed_range_in_parent(a).unwrap(),
        Some(range(25.0, 25.0))
    );
    // C is entirely outside it. Upstream raises here rather than reporting an
    // empty range, and its own test_composition.py pins that, so this does
    // too: a caller that silently got a zero-length range would place the clip
    // at the head of the track.
    assert_eq!(
        document.trimmed_range_in_parent(c),
        Err(Error::InvalidTimeRange)
    );
    // B is entirely inside.
    assert_eq!(
        document.trimmed_range_in_parent(b).unwrap(),
        Some(range(50.0, 25.0))
    );
}

#[test]
fn ranges_accumulate_down_a_nested_composition() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let inner = track(&mut document, "inner", &[a, b]);
    let lead_in = gap(&mut document, 10.0);
    let outer = track(&mut document, "outer", &[lead_in, inner]);

    // The inner track starts at 10 on the outer one, so B, which starts at 50
    // inside it, lands at 60.
    assert_eq!(
        document.range_of_child(outer, inner).unwrap(),
        range(10.0, 100.0)
    );
    assert_eq!(
        document.range_of_child(outer, b).unwrap(),
        range(60.0, 50.0)
    );
}

#[test]
fn a_time_can_be_restated_in_another_items_clock() {
    let mut document = Document::new();
    // A clip that starts 100 frames into its media.
    let a = clip(&mut document, "A", 100.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);

    // Frame 110 of A's media is frame 10 of the track.
    assert_eq!(
        document.transformed_time(time(110.0), a, sequence).unwrap(),
        time(10.0)
    );
    // And back again.
    assert_eq!(
        document.transformed_time(time(10.0), sequence, a).unwrap(),
        time(110.0)
    );
}

#[test]
fn child_at_time_finds_the_clip_under_the_playhead() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let c = clip(&mut document, "C", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    assert_eq!(
        document.child_at_time(sequence, time(0.0), true).unwrap(),
        Some(a)
    );
    assert_eq!(
        document.child_at_time(sequence, time(49.0), true).unwrap(),
        Some(a)
    );
    assert_eq!(
        document.child_at_time(sequence, time(50.0), true).unwrap(),
        Some(b)
    );
    assert_eq!(
        document.child_at_time(sequence, time(149.0), true).unwrap(),
        Some(c)
    );
    assert_eq!(
        document.child_at_time(sequence, time(150.0), true).unwrap(),
        None
    );
}

#[test]
fn child_at_time_descends_into_a_nested_track() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let inner = track(&mut document, "inner", &[a, b]);
    let outer = track(&mut document, "outer", &[inner]);

    assert_eq!(
        document.child_at_time(outer, time(60.0), true).unwrap(),
        Some(inner)
    );
    assert_eq!(
        document.child_at_time(outer, time(60.0), false).unwrap(),
        Some(b)
    );
}

// Upstream's `test_find_clips` and `test_child_at_time_with_children`
// search with a range of zero duration. That intersects nothing, but a track
// bisects its children rather than intersecting them, so it still finds the
// clip under the point.
#[test]
fn a_track_finds_the_child_under_a_zero_duration_range() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 100.0, 50.0);
    let b = clip(&mut document, "B", 101.0, 50.0);
    let c = clip(&mut document, "C", 102.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    let at = |value: f64| TimeRange::new(time(value), time(0.0));
    assert_eq!(document.children_in_range(sequence, at(-1.0)).unwrap(), []);
    assert_eq!(document.children_in_range(sequence, at(0.0)).unwrap(), [a]);
    assert_eq!(document.children_in_range(sequence, at(49.0)).unwrap(), [a]);
    assert_eq!(document.children_in_range(sequence, at(50.0)).unwrap(), [b]);
    assert_eq!(
        document.children_in_range(sequence, at(149.0)).unwrap(),
        [c]
    );
    assert_eq!(document.children_in_range(sequence, at(150.0)).unwrap(), []);
    assert_eq!(
        document
            .children_in_range(sequence, range(40.0, 20.0))
            .unwrap(),
        [a, b]
    );
}

// A stack intersects instead, as upstream's `Stack::children_in_range` does.
#[test]
fn a_stack_intersects_its_childrens_ranges() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 20.0);
    let lower = track(&mut document, "lower", &[a]);
    let upper = track(&mut document, "upper", &[b]);
    let layers = stack(&mut document, &[lower, upper]);

    assert_eq!(
        document
            .children_in_range(layers, range(10.0, 5.0))
            .unwrap(),
        [lower, upper]
    );
    assert_eq!(
        document
            .children_in_range(layers, range(30.0, 5.0))
            .unwrap(),
        [lower]
    );
    assert_eq!(
        document
            .children_in_range(layers, TimeRange::new(time(30.0), time(0.0)))
            .unwrap(),
        [lower]
    );
    assert_eq!(
        document
            .children_in_range(layers, range(60.0, 5.0))
            .unwrap(),
        []
    );
}

#[test]
fn neighbours_are_the_children_on_either_side() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let c = clip(&mut document, "C", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    assert_eq!(
        document.neighbors_of(sequence, b).unwrap(),
        (Some(a), Some(c))
    );
    assert_eq!(document.neighbors_of(sequence, a).unwrap(), (None, Some(b)));
    assert_eq!(document.neighbors_of(sequence, c).unwrap(), (Some(b), None));
}

#[test]
fn a_track_is_as_long_as_its_children_plus_any_overhanging_transitions() {
    let mut document = Document::new();
    let lead = transition(&mut document, 5.0, 3.0);
    let a = clip(&mut document, "A", 0.0, 50.0);
    let tail = transition(&mut document, 4.0, 7.0);
    let sequence = track(&mut document, "Sequence1", &[lead, a, tail]);

    // 50 frames of clip, plus 5 the leading transition reaches back by and 7
    // the trailing one reaches forward by.
    assert_eq!(document.duration(sequence).unwrap(), time(62.0));
}

#[test]
fn an_item_with_nothing_to_state_its_length_is_an_error() {
    let mut document = Document::new();
    let untrimmed = document.insert(otio_core::Node::Clip(otio_core::schema::Clip::default()));
    // No source range and no media reference that says how long the media is:
    // nothing in the file answers the question, so neither does this.
    assert!(matches!(
        document.duration(untrimmed),
        Err(Error::NoActiveMediaReference { .. })
    ));
}

#[test]
fn a_marker_has_no_duration() {
    let mut document = Document::new();
    let marker = document.insert(otio_core::Node::Marker(otio_core::schema::Marker::default()));
    assert!(matches!(
        document.duration(marker),
        Err(Error::NoDuration { schema }) if schema == "Marker"
    ));
}

#[test]
fn an_object_cannot_be_in_two_compositions_at_once() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let first = track(&mut document, "first", &[a]);
    let second = track(&mut document, "second", &[]);

    assert_eq!(
        document.append_child(second, a),
        Err(Error::ChildAlreadyParented)
    );

    // Taking it out of the first track makes the move legal.
    document.detach_child(first, a).unwrap();
    document.append_child(second, a).unwrap();
    assert_eq!(document.parent_of(a).unwrap(), second);
}

#[test]
fn a_deep_clone_shares_nothing_with_the_original() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let original = track(&mut document, "Sequence1", &[a, b]);

    let copy = document.deep_clone(original).unwrap();
    assert_ne!(copy, original);
    assert_eq!(document.try_get(copy).unwrap().parent(), None);

    let copied_children = document.children_of(copy).unwrap();
    assert_eq!(copied_children.len(), 2);
    for (copied, source) in copied_children.iter().zip([a, b]) {
        assert_ne!(*copied, source);
        assert_eq!(document.parent_of(*copied).unwrap(), copy);
    }

    // Editing the copy must not reach the original.
    document
        .try_get_mut(copied_children[0])
        .unwrap()
        .item_mut()
        .unwrap()
        .source_range = Some(range(10.0, 10.0));
    assert_eq!(document.trimmed_range(a).unwrap(), range(0.0, 50.0));
}

#[test]
fn a_deep_clone_copies_an_object_held_in_metadata() {
    // Metadata may hold a whole object. Copying the handle rather than what
    // it points at would hand back two objects that write to the same marker.
    let mut document = Document::new();
    let held = document.insert(Node::Marker(otio_core::schema::Marker {
        base: Base {
            name: "note".to_string(),
            ..Base::default()
        },
        ..otio_core::schema::Marker::default()
    }));
    let original = clip(&mut document, "A", 0.0, 50.0);
    document
        .try_get_mut(original)
        .unwrap()
        .base_mut()
        .unwrap()
        .metadata
        .insert("held".to_string(), Any::Object(held));

    let copy = document.deep_clone(original).unwrap();
    let copied_held = match document
        .try_get(copy)
        .unwrap()
        .base()
        .unwrap()
        .metadata
        .get("held")
    {
        Some(Any::Object(id)) => *id,
        other => panic!("expected an object in metadata, got {other:?}"),
    };
    assert_ne!(copied_held, held);

    document
        .try_get_mut(copied_held)
        .unwrap()
        .base_mut()
        .unwrap()
        .name = "renamed".to_string();
    assert_eq!(document.try_get(held).unwrap().name(), "note");
}

#[test]
fn removing_a_subtree_leaves_nothing_behind() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);
    assert_eq!(document.len(), 3);

    document.remove_recursive(sequence).unwrap();
    assert_eq!(document.len(), 0);
    assert!(!document.contains(a));
    assert!(!document.contains(b));
}

#[test]
fn a_negative_child_index_counts_from_the_end() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);

    assert_eq!(
        document.range_of_child_at_index(sequence, -1).unwrap(),
        range(50.0, 50.0)
    );
    assert!(matches!(
        document.range_of_child_at_index(sequence, 2),
        Err(Error::IllegalIndex { .. })
    ));
}

#[test]
fn laying_out_mixed_rates_resolves_to_the_higher_one() {
    let mut document = Document::new();
    // A 25fps clip followed by a 24fps one. Adding two times resolves to the
    // higher rate, so the accumulated start time comes out at 25 — the same
    // answer upstream gives, and the reason the seconds still agree.
    let a = document.insert(otio_core::Node::Clip(otio_core::schema::Clip {
        item: otio_core::schema::ItemData {
            source_range: Some(TimeRange::new(
                RationalTime::new(0.0, 25.0),
                RationalTime::new(25.0, 25.0),
            )),
            ..otio_core::schema::ItemData::new()
        },
        ..otio_core::schema::Clip::default()
    }));
    let b = clip(&mut document, "B", 0.0, 24.0);
    let sequence = track(&mut document, "mixed", &[a, b]);

    // One second at 25, then one second at 24: two seconds either way.
    assert_eq!(document.duration(sequence).unwrap().to_seconds(), 2.0);
    assert_eq!(
        document.range_in_parent(b).unwrap().start_time().rate(),
        25.0
    );
    assert_eq!(
        document
            .range_in_parent(b)
            .unwrap()
            .start_time()
            .to_seconds(),
        1.0
    );
}

#[test]
fn a_deep_clone_of_an_object_that_holds_itself_terminates() {
    // Metadata holds whole objects, and nothing stops one of them being the
    // object the metadata belongs to. Following that link without remembering
    // what has already been copied recurses until the process runs out of
    // stack, so the copy has to be registered before its links are followed.
    let mut document = Document::new();
    let original = clip(&mut document, "A", 0.0, 50.0);
    document
        .try_get_mut(original)
        .unwrap()
        .base_mut()
        .unwrap()
        .metadata
        .insert("self".to_string(), Any::Object(original));

    let copy = document.deep_clone(original).unwrap();
    assert_ne!(copy, original);

    // The copy holds itself, not the original: the cycle is reproduced rather
    // than broken or left pointing back at what was copied.
    let held = match document
        .try_get(copy)
        .unwrap()
        .base()
        .unwrap()
        .metadata
        .get("self")
    {
        Some(Any::Object(id)) => *id,
        other => panic!("expected an object in metadata, got {other:?}"),
    };
    assert_eq!(held, copy);
}

#[test]
fn a_deep_clone_copies_a_twice_held_object_once() {
    // The same object held under two keys is one object, and a copy that
    // silently turned it into two would not be a copy of the same graph.
    let mut document = Document::new();
    let held = document.insert(Node::Marker(otio_core::schema::Marker {
        base: Base {
            name: "note".to_string(),
            ..Base::default()
        },
        ..otio_core::schema::Marker::default()
    }));
    let original = clip(&mut document, "A", 0.0, 50.0);
    {
        let metadata = &mut document
            .try_get_mut(original)
            .unwrap()
            .base_mut()
            .unwrap()
            .metadata;
        metadata.insert("first".to_string(), Any::Object(held));
        metadata.insert("second".to_string(), Any::Object(held));
    }

    let copy = document.deep_clone(original).unwrap();
    let metadata = &document.try_get(copy).unwrap().base().unwrap().metadata;
    let at = |key: &str| match metadata.get(key) {
        Some(Any::Object(id)) => *id,
        other => panic!("expected an object in metadata, got {other:?}"),
    };
    assert_ne!(at("first"), held);
    assert_eq!(at("first"), at("second"));
}

/// A collection holding one timeline of three 24-frame clips, and the clips.
fn collected_timeline(document: &mut Document) -> (NodeId, [NodeId; 3]) {
    let clips = [
        clip(document, "A", 0.0, 24.0),
        clip(document, "B", 0.0, 24.0),
        clip(document, "C", 0.0, 24.0),
    ];
    let v1 = track(document, "V1", &clips);
    let tracks = stack(document, &[v1]);
    let timeline = document.insert(Node::Timeline(Timeline {
        tracks: Some(tracks),
        ..Timeline::default()
    }));
    let collection = document.insert(Node::SerializableCollection(
        SerializableCollection::default(),
    ));
    document.append_child(collection, timeline).unwrap();
    (collection, clips)
}

fn is_clip(node: &Node) -> bool {
    matches!(node, Node::Clip(_))
}

// Upstream's `test_find_children`: a collection is searched through the
// timeline it holds, although a timeline has no children of its own.
#[test]
fn a_collection_is_searched_through_the_timelines_it_holds() {
    let mut document = Document::new();
    let (collection, clips) = collected_timeline(&mut document);
    let found = document
        .find_children(collection, None, false, &is_clip)
        .unwrap();
    assert_eq!(found, clips);
}

// Upstream's `test_find_children_search_range`: the range reaches the track
// unchanged, and only the first clip sits in the first second.
#[test]
fn a_collection_hands_its_search_range_to_what_it_holds() {
    let mut document = Document::new();
    let (collection, clips) = collected_timeline(&mut document);
    let found = document
        .find_children(collection, Some(range(0.0, 24.0)), false, &is_clip)
        .unwrap();
    assert_eq!(found, [clips[0]]);
}

// Upstream's `test_find_children_shallow_search`: a shallow search of a
// collection looks at its own children and nothing below them.
#[test]
fn a_shallow_search_of_a_collection_stays_at_its_own_children() {
    let mut document = Document::new();
    let (collection, _) = collected_timeline(&mut document);
    let found = document
        .find_children(collection, None, true, &is_clip)
        .unwrap();
    assert!(found.is_empty());
    // The stack a timeline holds is how it is searched, not a child of it.
    let stacks = document
        .find_children(collection, None, false, &|node| {
            matches!(node, Node::Stack(_))
        })
        .unwrap();
    assert!(stacks.is_empty());
}

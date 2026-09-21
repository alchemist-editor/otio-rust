//! Where things sit in time.
//!
//! These check the range machinery against the behaviour upstream's C++
//! defines: a track lays its children end to end, a stack lays them on top of
//! one another, a transition sits over its neighbours rather than beside
//! them, and a composition's own trim clips what its children report.

mod common;

use common::{clip, gap, range, stack, time, track, transition};

use opentime::{RationalTime, TimeRange};
use otio_core::{Document, Error};

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

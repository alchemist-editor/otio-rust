//! The edit operations.
//!
//! Every case here is ported from upstream's `tests/test_editAlgorithm.cpp`,
//! with the same clip ranges and the same expectations, so a failure can be
//! read against the test it came from.
//!
//! Two kinds of range matter and they are easy to confuse: an item's *clip
//! range* is the part of its media in use, and its *track range* is where it
//! sits on the track. Upstream asserts on both, and so does this.

mod common;

use common::{
    assert_copied_apart, assert_held_twice, clip, gap, gap_at, hold_twice, range, time, track,
    transition,
};

use opentime::{RationalTime, TimeRange};
use otio_core::edit::{
    ReferencePoint, fill, insert, overwrite, remove, ripple, roll, slice, slide, slip, trim,
};
use otio_core::schema::{ItemData, MediaReferenceData, MissingReference, Node};
use otio_core::{Any, Document, Error, NodeId};

/// Where each child sits on the track.
fn track_ranges(document: &Document, sequence: NodeId) -> Vec<TimeRange> {
    document
        .children_of(sequence)
        .expect("a composition")
        .into_iter()
        .map(|child| {
            document
                .trimmed_range_of_child(sequence, child)
                .expect("a child range")
                .expect("not trimmed out")
        })
        .collect()
}

/// The part of its media each child is using.
fn clip_ranges(document: &Document, sequence: NodeId) -> Vec<TimeRange> {
    document
        .children_of(sequence)
        .expect("a composition")
        .into_iter()
        .map(|child| document.trimmed_range(child).expect("a trimmed item"))
        .collect()
}

/// The names of a track's children, in order.
fn names(document: &Document, sequence: NodeId) -> Vec<String> {
    document
        .children_of(sequence)
        .expect("a composition")
        .into_iter()
        .map(|child| document.try_get(child).expect("live").name().to_string())
        .collect()
}

/// Builds a time range at an arbitrary rate.
fn range_at(rate: f64, start: f64, duration: f64) -> TimeRange {
    TimeRange::new(
        RationalTime::new(start, rate),
        RationalTime::new(duration, rate),
    )
}

/// Adds a named clip trimmed to `start` and `duration` at an arbitrary rate.
fn clip_at(document: &mut Document, name: &str, rate: f64, start: f64, duration: f64) -> NodeId {
    let id = clip(document, name, 0.0, 0.0);
    set_source_range(document, id, range_at(rate, start, duration));
    id
}

/// Restates the part of an item's media in use.
fn set_source_range(document: &mut Document, id: NodeId, source_range: TimeRange) {
    document
        .try_get_mut(id)
        .expect("live")
        .item_mut()
        .expect("an item")
        .source_range = Some(source_range);
}

/// Adds a clip whose media runs for `available`, so that clamping applies.
fn clip_with_media(
    document: &mut Document,
    name: &str,
    source: TimeRange,
    available: TimeRange,
) -> NodeId {
    let reference = document.insert(Node::MissingReference(MissingReference {
        media: MediaReferenceData {
            available_range: Some(available),
            ..MediaReferenceData::default()
        },
    }));
    let id = clip(
        document,
        name,
        source.start_time().value(),
        source.duration().value(),
    );
    let Node::Clip(node) = document.try_get_mut(id).expect("just inserted") else {
        panic!("clip() makes a clip");
    };
    node.media_references
        .insert("DEFAULT_MEDIA".to_string(), reference);
    node.active_media_reference_key = "DEFAULT_MEDIA".to_string();
    id
}

// ---------------------------------------------------------------- slice ----

#[test]
fn slicing_in_the_middle_gives_two_halves() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a]);

    slice(&mut document, sequence, time(12.0), true).unwrap();
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(12.0, 12.0)]
    );
}

#[test]
fn slicing_on_an_edge_does_nothing() {
    // Upstream leaves the track alone at either edge, but gets there by two
    // different routes, and its own test cannot tell them apart because it
    // passes no error status. At the head the cut lands on the clip and the
    // piece before it is empty, so there is nothing to do. At the tail the cut
    // is past the last frame, so no child is found at all and the operation
    // reports that rather than doing nothing quietly. Both are pinned here.
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a]);

    slice(&mut document, sequence, time(0.0), true).unwrap();
    assert_eq!(track_ranges(&document, sequence), vec![range(0.0, 24.0)]);

    assert_eq!(
        slice(&mut document, sequence, time(24.0), true),
        Err(Error::NotAnItem)
    );
    assert_eq!(track_ranges(&document, sequence), vec![range(0.0, 24.0)]);
}

#[test]
fn slicing_one_frame_in_still_gives_two_pieces() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a]);

    slice(&mut document, sequence, time(1.0), true).unwrap();
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 1.0), range(1.0, 23.0)]
    );
}

#[test]
fn slicing_inside_a_transition_is_refused_unless_it_may_be_removed() {
    // Upstream's test_edit_slice_transitions_1, cut down to the one transition
    // the cut actually reaches.
    let fixture = |document: &mut Document| {
        let a = clip(document, "clip_0", 0.0, 24.0);
        let dissolve = transition(document, 5.0, 3.0);
        let b = clip(document, "clip_1", 0.0, 50.0);
        track(document, "Sequence1", &[a, dissolve, b])
    };

    // The dissolve straddles the cut at 24, reaching from 19 to 27, so a slice
    // at 22 lands on clip_0 but inside the transition's reach.
    let mut document = Document::new();
    let sequence = fixture(&mut document);
    assert_eq!(
        slice(&mut document, sequence, time(22.0), false),
        Err(Error::CannotTrimTransition)
    );
    assert_eq!(names(&document, sequence), vec!["clip_0", "", "clip_1"]);

    let mut document = Document::new();
    let sequence = fixture(&mut document);
    slice(&mut document, sequence, time(22.0), true).unwrap();
    assert_eq!(
        names(&document, sequence),
        vec!["clip_0", "clip_0", "clip_1"]
    );
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 22.0), range(22.0, 2.0), range(24.0, 50.0)]
    );
}

#[test]
fn slicing_where_a_transition_is_the_child_found_reports_no_item() {
    // A cut at 25 falls inside the dissolve's 19-to-27 reach and after the end
    // of clip_0, so the search finds the transition first. Upstream refuses it
    // as "not an item" rather than reaching past it to clip_1.
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let dissolve = transition(&mut document, 5.0, 3.0);
    let b = clip(&mut document, "clip_1", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, dissolve, b]);

    assert_eq!(
        slice(&mut document, sequence, time(25.0), true),
        Err(Error::NotAnItem)
    );
    assert_eq!(names(&document, sequence), vec!["clip_0", "", "clip_1"]);
}

#[test]
fn slicing_a_track_of_mixed_rates_states_each_piece_in_its_own_clock() {
    // Upstream's test_edit_slice_2: two 23.98 clips and one at 30, sliced at
    // times stated in 30. Every range here is the one upstream asserts, and
    // comparing two times rescales one to the other's rate, so a piece stated
    // at 30 still matches a piece upstream states at 23.98.
    let mut document = Document::new();
    let a = clip_at(&mut document, "clip_0", 23.98, 0.0, 71.94);
    let b = clip_at(&mut document, "clip_1", 23.98, 0.0, 71.94);
    let c = clip_at(&mut document, "clip_2", 30.0, 90.0, 90.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    slice(
        &mut document,
        sequence,
        RationalTime::new(121.0, 30.0),
        true,
    )
    .unwrap();
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![
            range_at(23.98, 0.0, 71.94),
            range_at(30.0, 0.0, 31.0),
            range_at(30.0, 31.0, 59.0),
            range_at(30.0, 90.0, 90.0),
        ]
    );
    assert_eq!(
        track_ranges(&document, sequence),
        vec![
            range_at(23.98, 0.0, 71.94),
            range_at(30.0, 90.0, 31.0),
            range_at(30.0, 121.0, 59.0),
            range_at(30.0, 180.0, 90.0),
        ]
    );

    // One frame on, which cuts the piece just made.
    slice(
        &mut document,
        sequence,
        RationalTime::new(122.0, 30.0),
        true,
    )
    .unwrap();
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![
            range_at(23.98, 0.0, 71.94),
            range_at(30.0, 0.0, 31.0),
            range_at(30.0, 31.0, 1.0),
            range_at(30.0, 32.0, 58.0),
            range_at(30.0, 90.0, 90.0),
        ]
    );

    // Drop the one-frame piece and the track closes up.
    let one_frame = document.children_of(sequence).unwrap()[2];
    let index = document.index_of_child(sequence, one_frame).unwrap();
    let removed = document.remove_child(sequence, index as i64).unwrap();
    document.remove_recursive(removed).unwrap();
    assert_eq!(
        track_ranges(&document, sequence),
        vec![
            range_at(23.98, 0.0, 71.94),
            range_at(30.0, 90.0, 31.0),
            range_at(30.0, 121.0, 58.0),
            range_at(30.0, 179.0, 90.0),
        ]
    );

    // A slice back on the cut does nothing, because there is no first piece.
    slice(
        &mut document,
        sequence,
        RationalTime::new(121.0, 30.0),
        true,
    )
    .unwrap();
    assert_eq!(
        track_ranges(&document, sequence),
        vec![
            range_at(23.98, 0.0, 71.94),
            range_at(30.0, 90.0, 31.0),
            range_at(30.0, 121.0, 58.0),
            range_at(30.0, 179.0, 90.0),
        ]
    );
}

// ------------------------------------------------------------- overwrite ----

#[test]
fn overwriting_past_the_end_appends_with_a_gap_between() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a]);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);

    overwrite(&mut document, b, sequence, range(48.0, 24.0), true, None).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(children.len(), 3);
    assert!(matches!(
        document.try_get(children[1]).unwrap(),
        Node::Gap(_)
    ));
    assert_eq!(document.duration(sequence).unwrap(), time(72.0));
    assert_eq!(
        document.trimmed_range_in_parent(b).unwrap(),
        Some(range(48.0, 24.0))
    );
}

#[test]
fn overwriting_one_frame_inside_a_clip_splits_it_in_three() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 1.0, 100.0);
    let sequence = track(&mut document, "Sequence1", &[a]);
    let b = clip(&mut document, "clip_1", 1.0, 1.0);

    overwrite(&mut document, b, sequence, range(42.0, 1.0), true, None).unwrap();

    // The track does not change length: one frame in, one frame out.
    assert_eq!(document.duration(sequence).unwrap(), time(100.0));
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![range(1.0, 42.0), range(1.0, 1.0), range(44.0, 57.0)]
    );
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 42.0), range(42.0, 1.0), range(43.0, 57.0)]
    );
}

#[test]
fn overwriting_across_two_clips_shortens_both() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);
    let over = clip(&mut document, "clip_2", 0.0, 24.0);

    let before = document.duration(sequence).unwrap();
    overwrite(&mut document, over, sequence, range(12.0, 24.0), true, None).unwrap();

    assert_eq!(document.duration(sequence).unwrap(), before);
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(12.0, 24.0), range(36.0, 12.0)]
    );
}

#[test]
fn a_fully_covered_clip_is_the_one_removed() {
    // Upstream's regression test: an overwrite that partly covers one clip and
    // wholly covers the next used to delete the wrong child.
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let c = clip(&mut document, "clip_2", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);
    let over = clip(&mut document, "over", 0.0, 100.0);

    overwrite(&mut document, over, sequence, range(36.0, 36.0), true, None).unwrap();

    assert_eq!(names(&document, sequence), vec!["clip_0", "clip_1", "over"]);
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 24.0), range(24.0, 12.0), range(36.0, 36.0)]
    );
}

// ---------------------------------------------------------------- insert ----

#[test]
fn inserting_inside_a_clip_splits_it_and_lengthens_the_track() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);
    let inserted = clip(&mut document, "insert_1", 0.0, 12.0);

    insert(&mut document, inserted, sequence, time(12.0), true, None).unwrap();

    assert_eq!(document.children_of(sequence).unwrap().len(), 4);
    assert_eq!(document.duration(sequence).unwrap(), time(60.0));
    assert_eq!(
        track_ranges(&document, sequence),
        vec![
            range(0.0, 12.0),
            range(12.0, 12.0),
            range(24.0, 12.0),
            range(36.0, 24.0),
        ]
    );
}

#[test]
fn inserting_on_a_clips_start_does_not_split_it() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);
    let inserted = clip(&mut document, "insert_1", 0.0, 12.0);

    insert(&mut document, inserted, sequence, time(0.0), true, None).unwrap();

    assert_eq!(document.children_of(sequence).unwrap().len(), 3);
    assert_eq!(document.duration(sequence).unwrap(), time(60.0));
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(12.0, 24.0), range(36.0, 24.0)]
    );
}

// ------------------------------------------------------------------ slip ----

#[test]
fn slipping_moves_the_media_under_a_fixed_window() {
    let media = range(-15.0, 63.0);
    for (delta, expected) in [
        (5.0, range(5.0, 36.0)),
        (12.0, range(12.0, 36.0)),
        // Past the end of the media, so it clamps.
        (20.0, range(12.0, 36.0)),
        (-5.0, range(-5.0, 36.0)),
        (-15.0, range(-15.0, 36.0)),
        // Past the start, so it clamps the other way.
        (-30.0, range(-15.0, 36.0)),
    ] {
        let mut document = Document::new();
        let a = clip_with_media(&mut document, "clip_0", range(0.0, 36.0), media);

        slip(&mut document, a, time(delta)).unwrap();
        assert_eq!(
            document.trimmed_range(a).unwrap(),
            expected,
            "slipping by {delta}"
        );
    }
}

// ----------------------------------------------------------------- slide ----

#[test]
fn sliding_stretches_the_clip_before_it() {
    let media = range(0.0, 48.0);
    for (delta, expected) in [
        (
            0.0,
            [range(0.0, 24.0), range(24.0, 30.0), range(54.0, 40.0)],
        ),
        (
            12.0,
            [range(0.0, 36.0), range(36.0, 30.0), range(66.0, 40.0)],
        ),
        // Beyond the previous clip's media, so it clamps.
        (
            48.0,
            [range(0.0, 48.0), range(48.0, 30.0), range(78.0, 40.0)],
        ),
        (
            -10.0,
            [range(0.0, 14.0), range(14.0, 30.0), range(44.0, 40.0)],
        ),
        // Would swallow the previous clip whole, so nothing moves.
        (
            -24.0,
            [range(0.0, 24.0), range(24.0, 30.0), range(54.0, 40.0)],
        ),
    ] {
        let mut document = Document::new();
        let a = clip_with_media(&mut document, "clip_0", range(0.0, 24.0), media);
        let b = clip(&mut document, "clip_1", 0.0, 30.0);
        let c = clip(&mut document, "clip_2", 0.0, 40.0);
        let sequence = track(&mut document, "Sequence1", &[a, b, c]);

        slide(&mut document, b, time(delta)).unwrap();
        assert_eq!(
            track_ranges(&document, sequence),
            expected.to_vec(),
            "sliding by {delta}"
        );
    }
}

// ---------------------------------------------------------------- ripple ----

/// Upstream's ripple and roll fixture: a gap and two clips.
fn ripple_fixture(document: &mut Document, middle_duration: f64) -> (NodeId, NodeId) {
    let hole = gap(document, 20.0);
    let b = clip(document, "clip_1", 5.0, middle_duration);
    let c = clip(document, "clip_2", 5.0, 20.0);
    let sequence = track(document, "Sequence1", &[hole, b, c]);
    (sequence, b)
}

#[test]
fn rippling_adjusts_one_item_and_moves_what_follows() {
    for (delta_in, delta_out, expected_track, expected_clips) in [
        (
            10.0,
            0.0,
            [range(0.0, 20.0), range(20.0, 15.0), range(35.0, 20.0)],
            [range(0.0, 20.0), range(15.0, 15.0), range(5.0, 20.0)],
        ),
        (
            -10.0,
            0.0,
            [range(0.0, 20.0), range(20.0, 30.0), range(50.0, 20.0)],
            [range(0.0, 20.0), range(0.0, 30.0), range(5.0, 20.0)],
        ),
        (
            0.0,
            10.0,
            [range(0.0, 20.0), range(20.0, 35.0), range(55.0, 20.0)],
            [range(0.0, 20.0), range(5.0, 35.0), range(5.0, 20.0)],
        ),
        (
            0.0,
            -10.0,
            [range(0.0, 20.0), range(20.0, 15.0), range(35.0, 20.0)],
            [range(0.0, 20.0), range(5.0, 15.0), range(5.0, 20.0)],
        ),
    ] {
        let mut document = Document::new();
        let (sequence, b) = ripple_fixture(&mut document, 25.0);

        ripple(&mut document, b, time(delta_in), time(delta_out)).unwrap();
        assert_eq!(
            track_ranges(&document, sequence),
            expected_track.to_vec(),
            "rippling by {delta_in}/{delta_out}"
        );
        assert_eq!(
            clip_ranges(&document, sequence),
            expected_clips.to_vec(),
            "rippling by {delta_in}/{delta_out}"
        );
    }
}

// ------------------------------------------------------------------ roll ----

#[test]
fn rolling_moves_a_cut_without_changing_the_track_length() {
    for (delta_in, delta_out, expected_track, expected_clips) in [
        (
            10.0,
            0.0,
            [range(0.0, 30.0), range(30.0, 20.0), range(50.0, 20.0)],
            [range(0.0, 30.0), range(15.0, 20.0), range(5.0, 20.0)],
        ),
        (
            -10.0,
            0.0,
            [range(0.0, 15.0), range(15.0, 35.0), range(50.0, 20.0)],
            [range(0.0, 15.0), range(0.0, 35.0), range(5.0, 20.0)],
        ),
        (
            0.0,
            10.0,
            [range(0.0, 20.0), range(20.0, 40.0), range(60.0, 20.0)],
            [range(0.0, 20.0), range(5.0, 40.0), range(15.0, 20.0)],
        ),
        (
            0.0,
            -10.0,
            [range(0.0, 20.0), range(20.0, 25.0), range(45.0, 20.0)],
            [range(0.0, 20.0), range(5.0, 25.0), range(0.0, 20.0)],
        ),
    ] {
        let mut document = Document::new();
        let (sequence, b) = ripple_fixture(&mut document, 30.0);

        roll(&mut document, b, time(delta_in), time(delta_out)).unwrap();
        assert_eq!(
            track_ranges(&document, sequence),
            expected_track.to_vec(),
            "rolling by {delta_in}/{delta_out}"
        );
        assert_eq!(
            clip_ranges(&document, sequence),
            expected_clips.to_vec(),
            "rolling by {delta_in}/{delta_out}"
        );
    }
}

// ------------------------------------------------------------------ trim ----

#[test]
fn trimming_the_head_stretches_the_gap_before_it() {
    let mut document = Document::new();
    let hole = gap(&mut document, 20.0);
    let b = clip(&mut document, "clip_1", 5.0, 50.0);
    let c = clip(&mut document, "clip_2", 5.0, 20.0);
    let sequence = track(&mut document, "Sequence1", &[hole, b, c]);

    let before = document.duration(sequence).unwrap();
    trim(&mut document, b, time(10.0), time(0.0), None).unwrap();

    // The clip gives up ten frames at the head and the gap takes them, so
    // nothing after it moves.
    assert_eq!(document.duration(sequence).unwrap(), before);
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 30.0), range(30.0, 40.0), range(70.0, 20.0)]
    );
}

#[test]
fn trimming_the_tail_leaves_a_gap_behind() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 5.0, 50.0);
    let b = clip(&mut document, "clip_1", 5.0, 20.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);

    let before = document.duration(sequence).unwrap();
    trim(&mut document, a, time(0.0), time(-10.0), None).unwrap();

    assert_eq!(document.duration(sequence).unwrap(), before);
    let children = document.children_of(sequence).unwrap();
    assert_eq!(children.len(), 3);
    assert!(matches!(
        document.try_get(children[1]).unwrap(),
        Node::Gap(_)
    ));
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 40.0), range(40.0, 10.0), range(50.0, 20.0)]
    );
}

// ------------------------------------------------------------------ fill ----

/// Runs upstream's `test_edit_fill` fixture.
///
/// The track is a clip, a gap and a clip; `filler` states the part of its
/// media a fourth clip is holding, and that clip is what gets dropped into the
/// gap.
fn check_fill(
    filler: TimeRange,
    track_time: RationalTime,
    reference_point: ReferencePoint,
    expected_track_ranges: &[TimeRange],
    expected_clip_ranges: &[TimeRange],
) {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 20.0);
    let hole = gap_at(&mut document, "gap_0", 5.0, 30.0);
    let c = clip(&mut document, "clip_2", 5.0, 20.0);
    let sequence = track(&mut document, "Sequence1", &[a, hole, c]);
    let item = clip(
        &mut document,
        "fill_0",
        filler.start_time().value(),
        filler.duration().value(),
    );

    let before = document.duration(sequence).unwrap();
    fill(&mut document, item, sequence, track_time, reference_point).unwrap();

    if reference_point == ReferencePoint::Sequence {
        assert_eq!(
            document.duration(sequence).unwrap(),
            before,
            "filling by sequence trims the media to the gap, so the track keeps its length"
        );
    }
    assert_eq!(track_ranges(&document, sequence), expected_track_ranges);
    assert_eq!(clip_ranges(&document, sequence), expected_clip_ranges);
}

#[test]
fn fitting_a_longer_clip_keeps_its_length_and_stretches_the_track() {
    // Upstream's test_edit_fill_1. Fitting hangs a time warp off the item
    // rather than trimming it, and nothing in the track layout reads that
    // warp, so 35 frames of media still occupy 35 frames of track even though
    // they are being asked to play over the gap's 30.
    check_fill(
        range(0.0, 35.0),
        time(20.0),
        ReferencePoint::Fit,
        &[range(0.0, 20.0), range(20.0, 35.0), range(55.0, 20.0)],
        &[range(0.0, 20.0), range(0.0, 35.0), range(5.0, 20.0)],
    );
}

#[test]
fn fitting_hangs_a_time_warp_at_the_ratio_of_gap_to_media() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 20.0);
    let hole = gap_at(&mut document, "gap_0", 5.0, 30.0);
    let c = clip(&mut document, "clip_2", 5.0, 20.0);
    let sequence = track(&mut document, "Sequence1", &[a, hole, c]);
    let item = clip(&mut document, "fill_0", 0.0, 35.0);

    fill(
        &mut document,
        item,
        sequence,
        time(20.0),
        ReferencePoint::Fit,
    )
    .unwrap();

    // The fitted child is a bare item, not a clip: upstream builds one to
    // carry the original's range and effects alongside the new warp.
    let fitted = document.children_of(sequence).unwrap()[1];
    assert!(matches!(
        document.try_get(fitted).unwrap(),
        Node::Item(_) | Node::Clip(_)
    ));

    let effects = document
        .try_get(fitted)
        .unwrap()
        .item()
        .unwrap()
        .effects
        .clone();
    assert_eq!(effects.len(), 1);
    let Node::LinearTimeWarp { time_scalar, .. } = document.try_get(effects[0]).unwrap() else {
        panic!("fitting should hang a linear time warp off the item");
    };
    assert!(
        (time_scalar - 30.0 / 35.0).abs() < 1e-9,
        "30 frames of gap over 35 of media, got {time_scalar}"
    );
}

#[test]
fn filling_a_longer_clip_by_source_takes_the_media_as_it_is() {
    // Upstream's test_edit_fill_2. The clip goes in whole and eats into what
    // is left of the gap.
    check_fill(
        range(0.0, 35.0),
        time(20.0),
        ReferencePoint::Source,
        &[range(0.0, 20.0), range(20.0, 35.0), range(55.0, 5.0)],
        &[range(0.0, 20.0), range(0.0, 35.0), range(20.0, 5.0)],
    );
}

#[test]
fn filling_a_clip_that_matches_the_gap_by_source_replaces_it() {
    // Upstream's test_edit_fill_3.
    check_fill(
        range(0.0, 30.0),
        time(20.0),
        ReferencePoint::Source,
        &[range(0.0, 20.0), range(20.0, 30.0), range(50.0, 20.0)],
        &[range(0.0, 20.0), range(0.0, 30.0), range(5.0, 20.0)],
    );
}

#[test]
fn filling_a_shorter_clip_by_source_leaves_the_rest_of_the_gap() {
    // Upstream's test_edit_fill_4.
    check_fill(
        range(0.0, 5.0),
        time(20.0),
        ReferencePoint::Source,
        &[
            range(0.0, 20.0),
            range(20.0, 5.0),
            range(25.0, 25.0),
            range(50.0, 20.0),
        ],
        &[
            range(0.0, 20.0),
            range(0.0, 5.0),
            range(10.0, 25.0),
            range(5.0, 20.0),
        ],
    );
}

#[test]
fn filling_by_sequence_trims_the_media_to_the_gap() {
    // Upstream's test_edit_fill_5. Thirty-five frames of media, but only the
    // thirty the gap covers survive.
    check_fill(
        range(0.0, 35.0),
        time(20.0),
        ReferencePoint::Sequence,
        &[range(0.0, 20.0), range(20.0, 30.0), range(50.0, 20.0)],
        &[range(0.0, 20.0), range(5.0, 30.0), range(5.0, 20.0)],
    );
}

#[test]
fn filling_by_sequence_lines_the_media_up_with_the_gaps_own_clock() {
    // Upstream's test_edit_fill_6. The media starts ten frames before the gap
    // does, so the first ten are dropped and half the gap is left.
    check_fill(
        range(-10.0, 30.0),
        time(20.0),
        ReferencePoint::Sequence,
        &[
            range(0.0, 20.0),
            range(20.0, 15.0),
            range(35.0, 15.0),
            range(50.0, 20.0),
        ],
        &[
            range(0.0, 20.0),
            range(5.0, 15.0),
            range(20.0, 15.0),
            range(5.0, 20.0),
        ],
    );
}

#[test]
fn filling_a_shorter_clip_by_sequence_leaves_the_rest_of_the_gap() {
    // Upstream's test_edit_fill_7.
    check_fill(
        range(10.0, 5.0),
        time(20.0),
        ReferencePoint::Sequence,
        &[
            range(0.0, 20.0),
            range(20.0, 5.0),
            range(25.0, 25.0),
            range(50.0, 20.0),
        ],
        &[
            range(0.0, 20.0),
            range(10.0, 5.0),
            range(10.0, 25.0),
            range(5.0, 20.0),
        ],
    );
}

#[test]
fn filling_somewhere_that_is_not_a_gap_is_an_error() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 20.0);
    let sequence = track(&mut document, "Sequence1", &[a]);
    let filler = clip(&mut document, "fill_0", 0.0, 10.0);

    assert_eq!(
        fill(
            &mut document,
            filler,
            sequence,
            time(5.0),
            ReferencePoint::Source
        ),
        Err(Error::NotAGap)
    );
}

// ---------------------------------------------------------------- remove ----

#[test]
fn removing_leaves_a_gap_of_the_same_length() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let c = clip(&mut document, "clip_2", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    let before = document.duration(sequence).unwrap();
    remove(&mut document, sequence, time(30.0), true, None).unwrap();

    assert_eq!(document.duration(sequence).unwrap(), before);
    let children = document.children_of(sequence).unwrap();
    assert!(matches!(
        document.try_get(children[1]).unwrap(),
        Node::Gap(_)
    ));
    assert_eq!(names(&document, sequence), vec!["clip_0", "", "clip_2"]);
}

#[test]
fn removing_without_a_fill_closes_the_hole() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let c = clip(&mut document, "clip_2", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b, c]);

    remove(&mut document, sequence, time(30.0), false, None).unwrap();

    assert_eq!(names(&document, sequence), vec!["clip_0", "clip_2"]);
    assert_eq!(document.duration(sequence).unwrap(), time(48.0));
}

#[test]
fn removing_where_there_is_nothing_is_an_error() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a]);

    assert_eq!(
        remove(&mut document, sequence, time(100.0), true, None),
        Err(Error::NotAnItem)
    );
}

// -------------------------------------------------------------- sparing ----

#[test]
fn a_removed_clip_is_dropped_unless_spared() {
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);

    remove(&mut document, sequence, time(30.0), true, None).unwrap();

    assert!(document.get(b).is_none());
    assert!(document.take_spared().is_empty());
}

#[test]
fn a_spared_clip_outlives_its_removal_without_a_parent() {
    // Upstream's objects are reference counted, so a clip an edit takes out
    // lives on while anything still holds it. Sparing is how a caller holding
    // handles from outside the document gets the same.
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    let b = clip(&mut document, "clip_1", 0.0, 24.0);
    let sequence = track(&mut document, "Sequence1", &[a, b]);
    let over = clip(&mut document, "over", 0.0, 24.0);

    document.spare([a, b]);
    overwrite(&mut document, over, sequence, range(24.0, 24.0), true, None).unwrap();
    remove(&mut document, sequence, time(30.0), true, None).unwrap();

    assert_eq!(names(&document, sequence), vec!["clip_0", ""]);
    assert_eq!(document.take_spared(), vec![b]);
    let spared = document.try_get(b).unwrap();
    assert_eq!(spared.name(), "clip_1");
    assert_eq!(spared.parent(), None);

    // Sparing ends with the call that reports it.
    remove(&mut document, sequence, time(0.0), true, None).unwrap();
    assert!(document.get(a).is_none());
    assert!(document.take_spared().is_empty());
}

// ------------------------------------------------------------ bare items ----

#[test]
fn a_bare_item_round_trips_through_json() {
    // `Item` is a schema in its own right upstream, and `fill` builds one.
    let json = r#"{
    "OTIO_SCHEMA": "Item.1",
    "name": "bare",
    "source_range": {
        "OTIO_SCHEMA": "TimeRange.1",
        "duration": { "OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 10 },
        "start_time": { "OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 0 }
    }
}"#;
    let document = otio_core::from_str(json).expect("parses");
    let root = document.root().expect("has a root");
    assert!(matches!(
        document.try_get(root).unwrap(),
        Node::Item(ItemData { .. })
    ));
    assert_eq!(document.trimmed_range(root).unwrap(), range(0.0, 10.0));

    let written = otio_core::to_string(&document).expect("writes");
    assert!(written.contains("\"OTIO_SCHEMA\": \"Item.1\""));
}

#[test]
fn the_misspelled_collection_alias_reads_as_the_real_thing() {
    // An old release wrote `SerializeableCollection`; upstream still maps it.
    let json = r#"{ "OTIO_SCHEMA": "SerializeableCollection.1", "children": [] }"#;
    let document = otio_core::from_str(json).expect("parses");
    let root = document.root().expect("has a root");
    assert!(matches!(
        document.try_get(root).unwrap(),
        Node::SerializableCollection(_)
    ));

    let written = otio_core::to_string(&document).expect("writes");
    assert!(written.contains("\"SerializableCollection.1\""));
}

#[test]
fn an_unstated_media_length_leaves_slipping_unclamped() {
    // A clip with no available range cannot be clamped against anything, and
    // upstream treats that as "no clamp" rather than an error.
    let mut document = Document::new();
    let a = clip(&mut document, "clip_0", 0.0, 24.0);
    slip(&mut document, a, RationalTime::new(1000.0, 24.0)).unwrap();
    assert_eq!(document.trimmed_range(a).unwrap(), range(1000.0, 24.0));
}

// ------------------------------------------------- metadata that cycles ----

/// Makes an item's metadata hold the item itself.
fn hold_itself(document: &mut Document, id: NodeId) {
    document
        .try_get_mut(id)
        .unwrap()
        .base_mut()
        .unwrap()
        .metadata
        .insert("cycle".to_string(), Any::Object(id));
}

/// What an item's metadata holds under `cycle`.
fn held(document: &Document, id: NodeId) -> NodeId {
    match document
        .try_get(id)
        .unwrap()
        .base()
        .unwrap()
        .metadata
        .get("cycle")
    {
        Some(Any::Object(held)) => *held,
        other => panic!("expected an object under 'cycle', got {other:?}"),
    }
}

// Upstream's four "regression: ... fails gracefully" tests put a clip in its
// own metadata and expect slice, overwrite, insert and fill to fail. Here they
// succeed, on purpose.
//
// Upstream copies the piece a split leaves over with `clone()`, and `clone()`
// works by writing the object out and reading it back, which cannot carry a
// cycle. Its C++ tests fail even earlier: they store a `Retainer<Clip>`,
// which the writer has no entry for, so the outcome they check,
// TYPE_MISMATCH, comes from that and not from the cycle; the same edits fail
// on a clip whose metadata merely holds another clip. From Python, where a
// clip in metadata is stored the way the writer knows, the edit fails with
// OBJECT_CYCLE instead. Slice, overwrite and insert get there only after
// changing the track, so upstream leaves the clip cut short and the rest of
// it gone. Fill clones first and fails cleanly.
//
// Nothing about the edit needs the copy to go through JSON. The copy here is
// made in memory and follows the cycle, so the leftover piece holds itself,
// as the original does, and the track ends up as the same edit leaves it for
// a clip with no cycle: the ranges asserted are upstream's for that edit.
// The document still cannot be written as JSON, by upstream or here; only
// the edit is allowed.

#[test]
fn slicing_a_clip_that_holds_itself_succeeds() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    hold_itself(&mut document, big);
    let sequence = track(&mut document, "", &[big]);

    slice(&mut document, sequence, time(12.0), false).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(12.0, 12.0)]
    );
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(12.0, 12.0)]
    );
    assert_eq!(held(&document, children[0]), children[0]);
    assert_eq!(held(&document, children[1]), children[1]);
}

#[test]
fn overwriting_a_clip_that_holds_itself_succeeds() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    hold_itself(&mut document, big);
    let small = clip(&mut document, "small clip", 0.0, 5.0);
    hold_itself(&mut document, small);
    let sequence = track(&mut document, "", &[big]);

    overwrite(&mut document, small, sequence, range(0.0, 12.0), true, None).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(names(&document, sequence), vec!["small clip", "big clip"]);
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 5.0), range(5.0, 12.0)]
    );
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![range(0.0, 5.0), range(12.0, 12.0)]
    );
    assert_eq!(held(&document, children[0]), small);
    assert_eq!(held(&document, children[1]), children[1]);
}

#[test]
fn inserting_into_a_clip_that_holds_itself_succeeds() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    hold_itself(&mut document, big);
    let small = clip(&mut document, "small clip", 0.0, 5.0);
    hold_itself(&mut document, small);
    let sequence = track(&mut document, "", &[big]);

    insert(&mut document, small, sequence, time(12.0), true, None).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(
        names(&document, sequence),
        vec!["big clip", "small clip", "big clip"]
    );
    assert_eq!(
        track_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(12.0, 5.0), range(17.0, 12.0)]
    );
    // Upstream's arithmetic for the second piece, reproduced: it starts 17
    // frames into the media, not 12.
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![range(0.0, 12.0), range(0.0, 5.0), range(17.0, 12.0)]
    );
    assert_eq!(held(&document, children[0]), big);
    assert_eq!(held(&document, children[2]), children[2]);
}

#[test]
fn filling_with_a_clip_that_holds_itself_succeeds() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    hold_itself(&mut document, big);
    let small = clip(&mut document, "small clip", 0.0, 5.0);
    hold_itself(&mut document, small);
    let small2 = clip(&mut document, "small clip 2", 0.0, 5.0);
    hold_itself(&mut document, small2);
    let hole = gap_at(&mut document, "gap", 0.0, 20.0);
    let sequence = track(&mut document, "", &[small, hole, small2]);

    fill(
        &mut document,
        big,
        sequence,
        time(12.0),
        ReferencePoint::Sequence,
    )
    .unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(
        names(&document, sequence),
        vec!["small clip", "gap", "big clip", "small clip 2"]
    );
    assert_eq!(
        track_ranges(&document, sequence),
        vec![
            range(0.0, 5.0),
            range(5.0, 7.0),
            range(12.0, 13.0),
            range(25.0, 5.0)
        ]
    );
    assert_eq!(
        clip_ranges(&document, sequence),
        vec![
            range(0.0, 5.0),
            range(0.0, 7.0),
            range(0.0, 13.0),
            range(0.0, 5.0)
        ]
    );
    // The track holds a copy of the clip, and the copy holds itself.
    assert_ne!(children[2], big);
    assert_eq!(held(&document, children[2]), children[2]);
    assert_eq!(held(&document, big), big);
}

// ------------------------------------------ what the copied piece holds ----

// Upstream makes each of these copies with `clone()`, which copies by writing
// the item out and reading it back. Its writer can mark an object it meets a
// second time and refer back to it (`OTIO_REF_ID`), but only when built with
// `OTIO_INSTANCING_SUPPORT`, which its build never defines; without it, an
// object is forgotten once written and a second holder writes it out again.
// Run against an upstream build, the copy each edit makes holds two objects
// where the item held one twice — metadata, effects and media references
// alike — and none of the originals. The item left in place keeps its own.

#[test]
fn slicing_copies_what_the_item_holds_twice_into_two() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    let held = hold_twice(&mut document, big);
    let sequence = track(&mut document, "", &[big]);

    slice(&mut document, sequence, time(12.0), true).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(children[0], big);
    assert_held_twice(&document, big, held);
    assert_copied_apart(&document, children[1], held);
}

#[test]
fn overwriting_inside_an_item_copies_what_it_holds_twice_into_two() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    let held = hold_twice(&mut document, big);
    let sequence = track(&mut document, "", &[big]);
    let small = clip(&mut document, "small clip", 0.0, 4.0);

    overwrite(&mut document, small, sequence, range(8.0, 4.0), true, None).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(
        names(&document, sequence),
        ["big clip", "small clip", "big clip"]
    );
    assert_held_twice(&document, big, held);
    assert_copied_apart(&document, children[2], held);
}

#[test]
fn inserting_inside_an_item_copies_what_it_holds_twice_into_two() {
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    let held = hold_twice(&mut document, big);
    let sequence = track(&mut document, "", &[big]);
    let small = clip(&mut document, "small clip", 0.0, 4.0);

    insert(&mut document, small, sequence, time(12.0), true, None).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(
        names(&document, sequence),
        ["big clip", "small clip", "big clip"]
    );
    assert_held_twice(&document, big, held);
    assert_copied_apart(&document, children[2], held);
}

#[test]
fn filling_by_sequence_copies_what_the_clip_holds_twice_into_two() {
    let mut document = Document::new();
    let before = clip(&mut document, "before", 0.0, 5.0);
    let hole = gap_at(&mut document, "gap", 0.0, 20.0);
    let after = clip(&mut document, "after", 0.0, 5.0);
    let sequence = track(&mut document, "", &[before, hole, after]);
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    let held = hold_twice(&mut document, big);

    fill(
        &mut document,
        big,
        sequence,
        time(12.0),
        ReferencePoint::Sequence,
    )
    .unwrap();

    // The track gets a copy of the clip; the clip itself stays out of it.
    let children = document.children_of(sequence).unwrap();
    assert_eq!(
        names(&document, sequence),
        ["before", "gap", "big clip", "after"]
    );
    assert!(document.parent_of(big).is_err());
    assert_held_twice(&document, big, held);
    assert_copied_apart(&document, children[2], held);
}

#[test]
fn a_copied_piece_that_holds_itself_still_copies_what_it_holds_twice_into_two() {
    // Following a cycle, which upstream cannot copy, is the one thing the
    // edits' copy does differently; an object held twice beside the cycle is
    // still copied twice.
    let mut document = Document::new();
    let big = clip(&mut document, "big clip", 0.0, 24.0);
    let twice = hold_twice(&mut document, big);
    hold_itself(&mut document, big);
    let sequence = track(&mut document, "", &[big]);

    slice(&mut document, sequence, time(12.0), true).unwrap();

    let children = document.children_of(sequence).unwrap();
    assert_eq!(held(&document, children[1]), children[1]);
    assert_copied_apart(&document, children[1], twice);
}

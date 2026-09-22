//! Flattening a stack, and trimming a track.
//!
//! The fixtures and expectations here are ported from upstream's
//! `tests/test_stack_algo.py` and `tests/test_track_algo.py`. Where a name
//! appears — `trackABC`, `trackDgE` — it is upstream's name for the same
//! arrangement, so a failure can be read against the test it came from.

mod common;

use common::{
    assert_copied_apart, assert_held_twice, clip, disabled_clip, gap, held_in_metadata, hold_twice,
    owned_count, range, stack, summarize, time, track, transition,
};

use otio_core::algorithm::{flatten_stack, flatten_tracks, track_trimmed_to_range};
use otio_core::{Any, Document, Error, NodeId};

/// Three 50-frame clips laid end to end: upstream's `trackABC`.
fn track_abc(document: &mut Document) -> otio_core::NodeId {
    let a = clip(document, "A", 0.0, 50.0);
    let b = clip(document, "B", 0.0, 50.0);
    let c = clip(document, "C", 0.0, 50.0);
    track(document, "Sequence1", &[a, b, c])
}

/// One 150-frame clip: upstream's `trackZ`.
fn track_z(document: &mut Document) -> otio_core::NodeId {
    let z = clip(document, "Z", 0.0, 150.0);
    track(document, "Sequence2", &[z])
}

/// A clip, a gap, a clip: upstream's `trackDgE`.
fn track_dge(document: &mut Document) -> otio_core::NodeId {
    let d = clip(document, "D", 0.0, 50.0);
    let hole = gap(document, 50.0);
    let e = clip(document, "E", 0.0, 50.0);
    track(document, "Sequence3", &[d, hole, e])
}

/// A gap, a clip, a gap: upstream's `trackgFg`.
fn track_gfg(document: &mut Document) -> otio_core::NodeId {
    let first = gap(document, 50.0);
    let f = clip(document, "F", 0.0, 50.0);
    let last = gap(document, 50.0);
    track(document, "Sequence4", &[first, f, last])
}

#[test]
fn flattening_one_track_copies_it() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let layers = stack(&mut document, &[abc]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(summarize(&document, flat), summarize(&document, abc));

    // Equivalent, but not the same objects: the original must be left alone.
    let originals = document.children_of(abc).unwrap();
    for (copied, original) in document.children_of(flat).unwrap().iter().zip(originals) {
        assert_ne!(*copied, original);
    }
}

// Upstream builds the result with `new Track`, so it is a video track named
// "Flattened"; `test_flatten_example_code` compares it with one read from a
// file.
#[test]
fn the_flattened_track_is_a_video_track() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let layers = stack(&mut document, &[abc]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    let otio_core::Node::Track(track) = document.try_get(flat).unwrap() else {
        panic!("flattening makes a track");
    };
    assert_eq!(track.kind, otio_core::TRACK_KIND_VIDEO);
    assert_eq!(track.item.base.name, "Flattened");
}

#[test]
fn a_higher_track_obscures_a_lower_one() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let z = track_z(&mut document);
    let layers = stack(&mut document, &[abc, z]);

    // Z is on top and covers everything, so only Z survives.
    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(summarize(&document, flat), summarize(&document, z));
}

#[test]
fn order_decides_which_track_wins() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let z = track_z(&mut document);
    // Lowest first, so ABC is now on top.
    let layers = stack(&mut document, &[z, abc]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(summarize(&document, flat), summarize(&document, abc));
}

#[test]
fn a_disabled_clip_lets_the_track_below_show_through() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let z = disabled_clip(&mut document, "Z", 0.0, 150.0);
    let top = track(&mut document, "Sequence2", &[z]);
    let layers = stack(&mut document, &[abc, top]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(summarize(&document, flat), summarize(&document, abc));
}

#[test]
fn a_disabled_track_contributes_nothing() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let z = track_z(&mut document);
    document.try_get_mut(z).unwrap().item_mut().unwrap().enabled = false;
    let layers = stack(&mut document, &[abc, z]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(summarize(&document, flat), summarize(&document, abc));
}

#[test]
fn a_gap_lets_the_track_below_show_through() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let dge = track_dge(&mut document);
    let layers = stack(&mut document, &[abc, dge]);

    // D and E come from the top track; the hole between them shows B.
    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("D".to_string(), range(0.0, 50.0)),
            ("B".to_string(), range(0.0, 50.0)),
            ("E".to_string(), range(0.0, 50.0)),
        ]
    );
}

#[test]
fn gaps_at_either_end_show_the_track_below() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let gfg = track_gfg(&mut document);
    let layers = stack(&mut document, &[abc, gfg]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("A".to_string(), range(0.0, 50.0)),
            ("F".to_string(), range(0.0, 50.0)),
            ("C".to_string(), range(0.0, 50.0)),
        ]
    );
}

#[test]
fn showing_through_trims_the_clip_beneath_to_the_hole() {
    let mut document = Document::new();
    let z = track_z(&mut document);
    let dge = track_dge(&mut document);
    let layers = stack(&mut document, &[z, dge]);

    // The hole runs from 50 to 100, so the single long clip beneath appears
    // trimmed to exactly that span of its own media.
    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("D".to_string(), range(0.0, 50.0)),
            ("Z".to_string(), range(50.0, 50.0)),
            ("E".to_string(), range(0.0, 50.0)),
        ]
    );
}

#[test]
fn a_hole_at_each_end_trims_the_clip_beneath_twice() {
    let mut document = Document::new();
    let z = track_z(&mut document);
    let gfg = track_gfg(&mut document);
    let layers = stack(&mut document, &[z, gfg]);

    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("Z".to_string(), range(0.0, 50.0)),
            ("F".to_string(), range(0.0, 50.0)),
            ("Z".to_string(), range(100.0, 50.0)),
        ]
    );
}

#[test]
fn a_bare_list_of_tracks_flattens_the_same_way() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let dge = track_dge(&mut document);

    let flat = flatten_tracks(&mut document, &[abc, dge]).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("D".to_string(), range(0.0, 50.0)),
            ("B".to_string(), range(0.0, 50.0)),
            ("E".to_string(), range(0.0, 50.0)),
        ]
    );
}

#[test]
fn a_short_track_is_padded_rather_than_left_ragged() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    let short = clip(&mut document, "S", 0.0, 50.0);
    let top = track(&mut document, "short", &[short]);
    let layers = stack(&mut document, &[abc, top]);

    // The top track only covers the first 50 frames; the rest of the
    // flattened track has to come from below, not stop short.
    let flat = flatten_stack(&mut document, layers).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("S".to_string(), range(0.0, 50.0)),
            ("B".to_string(), range(0.0, 50.0)),
            ("C".to_string(), range(0.0, 50.0)),
        ]
    );
    // Padding happens on a copy, so the original is untouched.
    assert_eq!(document.children_of(top).unwrap().len(), 1);
}

#[test]
fn flattening_a_stack_of_something_other_than_tracks_is_an_error() {
    let mut document = Document::new();
    let inner = stack(&mut document, &[]);
    let layers = stack(&mut document, &[inner]);

    assert!(matches!(
        flatten_stack(&mut document, layers),
        Err(Error::UnexpectedChild { .. })
    ));
}

#[test]
fn trimming_to_the_range_a_track_already_has_changes_nothing() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);
    assert_eq!(document.trimmed_range(abc).unwrap(), range(0.0, 150.0));

    let trimmed = track_trimmed_to_range(&mut document, abc, range(0.0, 150.0)).unwrap();
    assert_eq!(summarize(&document, trimmed), summarize(&document, abc));
}

#[test]
fn trimming_to_a_longer_range_changes_nothing() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);

    let trimmed = track_trimmed_to_range(&mut document, abc, range(-10.0, 160.0)).unwrap();
    assert_eq!(summarize(&document, trimmed), summarize(&document, abc));
}

#[test]
fn trimming_the_front_drops_and_shortens() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);

    // Cuts off A entirely and the first 10 frames of B.
    let trimmed = track_trimmed_to_range(&mut document, abc, range(60.0, 90.0)).unwrap();
    assert_eq!(
        summarize(&document, trimmed),
        vec![
            ("B".to_string(), range(10.0, 40.0)),
            ("C".to_string(), range(0.0, 50.0)),
        ]
    );
    assert_eq!(document.trimmed_range(trimmed).unwrap(), range(0.0, 90.0));
}

#[test]
fn trimming_the_end_drops_and_shortens() {
    let mut document = Document::new();
    let abc = track_abc(&mut document);

    let trimmed = track_trimmed_to_range(&mut document, abc, range(0.0, 90.0)).unwrap();
    assert_eq!(
        summarize(&document, trimmed),
        vec![
            ("A".to_string(), range(0.0, 50.0)),
            ("B".to_string(), range(0.0, 40.0)),
        ]
    );
    assert_eq!(document.trimmed_range(trimmed).unwrap(), range(0.0, 90.0));
}

#[test]
fn a_trim_cannot_cut_a_transition_in_half() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let dissolve = transition(&mut document, 12.0, 20.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let c = clip(&mut document, "C", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, dissolve, b, c]);

    // A transition runs 38 to 70 here, so either edge landing inside it has
    // no meaning.
    assert_eq!(
        track_trimmed_to_range(&mut document, sequence, range(5.0, 50.0)),
        Err(Error::CannotTrimTransition)
    );
    assert_eq!(
        track_trimmed_to_range(&mut document, sequence, range(45.0, 50.0)),
        Err(Error::CannotTrimTransition)
    );

    // A trim that contains the whole transition is fine.
    let trimmed = track_trimmed_to_range(&mut document, sequence, range(25.0, 50.0)).unwrap();
    assert_eq!(
        summarize(&document, trimmed),
        vec![
            ("A".to_string(), range(25.0, 25.0)),
            (String::new(), range(-12.0, 32.0)),
            ("B".to_string(), range(0.0, 25.0)),
        ]
    );
}

#[test]
fn a_transition_keeps_its_offsets_through_a_trim() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let dissolve = transition(&mut document, 12.0, 20.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, dissolve, b]);

    let trimmed = track_trimmed_to_range(&mut document, sequence, range(25.0, 50.0)).unwrap();
    let children = document.children_of(trimmed).unwrap();
    let otio_core::Node::Transition(copied) = document.try_get(children[1]).unwrap() else {
        panic!("the transition should have survived the trim");
    };
    assert_eq!(copied.in_offset, time(12.0));
    assert_eq!(copied.out_offset, time(20.0));
}

#[test]
fn an_algorithm_leaves_no_scratch_objects_behind() {
    let mut document = Document::new();
    let z = track_z(&mut document);
    let dge = track_dge(&mut document);
    let layers = stack(&mut document, &[z, dge]);
    let before = document.len();

    let flat = flatten_stack(&mut document, layers).unwrap();

    // Only the flattened track and its three children are new; every copy the
    // algorithm made along the way is gone.
    let added = document.len() - before;
    assert_eq!(added, 1 + document.children_of(flat).unwrap().len());
}

// ------------------------------------------------- what the copies hold ----

// Upstream copies with `clone()` in both algorithms: the whole track to trim
// it, a short track to pad it, and each child that lands on the flattened
// track. `clone()` writes the object out and reads it back, and upstream's
// writer, built as it always is without `OTIO_INSTANCING_SUPPORT`, forgets an
// object once written, so a second holder writes it out again. Run against an
// upstream build, a copied clip holds two objects where the original held one
// twice, two clips of a copied track holding one object hold one each, and an
// object that holds itself is refused as a cycle.

/// Two clips, the first holding objects twice as [`hold_twice`] sets up and
/// the second holding the first's metadata object as well, in a track.
fn track_sharing_one_object(document: &mut Document) -> (NodeId, NodeId, common::HeldTwice) {
    let first = clip(document, "A", 0.0, 24.0);
    let held = hold_twice(document, first);
    let second = clip(document, "B", 0.0, 24.0);
    document
        .try_get_mut(second)
        .unwrap()
        .base_mut()
        .unwrap()
        .metadata
        .insert("a".to_string(), Any::Object(held.object));
    let sequence = track(document, "Sequence", &[first, second]);
    (sequence, first, held)
}

/// Makes an item's metadata hold the item itself.
fn hold_itself(document: &mut Document, id: NodeId) {
    document
        .try_get_mut(id)
        .unwrap()
        .base_mut()
        .unwrap()
        .metadata
        .insert("self".to_string(), Any::Object(id));
}

#[test]
fn trimming_copies_an_object_held_twice_into_two() {
    let mut document = Document::new();
    let (sequence, first, held) = track_sharing_one_object(&mut document);

    let trimmed = track_trimmed_to_range(&mut document, sequence, range(0.0, 48.0)).unwrap();

    let children = document.children_of(trimmed).unwrap();
    assert_copied_apart(&document, children[0], held);
    // Held by two clips of the track, the object comes out as one per clip.
    assert_ne!(
        held_in_metadata(&document, children[0], "a"),
        held_in_metadata(&document, children[1], "a")
    );
    assert_ne!(held_in_metadata(&document, children[1], "a"), held.object);
    assert_held_twice(&document, first, held);
}

#[test]
fn flattening_copies_an_object_held_twice_into_two() {
    let mut document = Document::new();
    let (sequence, first, held) = track_sharing_one_object(&mut document);
    let layers = stack(&mut document, &[sequence]);

    let flat = flatten_stack(&mut document, layers).unwrap();

    let children = document.children_of(flat).unwrap();
    assert_copied_apart(&document, children[0], held);
    assert_ne!(
        held_in_metadata(&document, children[0], "a"),
        held_in_metadata(&document, children[1], "a")
    );
    assert_held_twice(&document, first, held);
}

#[test]
fn flattening_through_a_hole_copies_an_object_held_twice_into_two() {
    // The upper track is short, so it is padded by a copy, and its gap sends
    // the search down to a trimmed copy of the track below; what lands on the
    // flattened track is a copy of those copies.
    let mut document = Document::new();
    let below = clip(&mut document, "below", 0.0, 48.0);
    let below_held = hold_twice(&mut document, below);
    let lower = track(&mut document, "lower", &[below]);
    let hole = gap(&mut document, 12.0);
    let above = clip(&mut document, "above", 0.0, 12.0);
    let above_held = hold_twice(&mut document, above);
    let upper = track(&mut document, "upper", &[hole, above]);
    let layers = stack(&mut document, &[lower, upper]);
    let before = document.len();

    let flat = flatten_stack(&mut document, layers).unwrap();

    let children = document.children_of(flat).unwrap();
    assert_eq!(
        summarize(&document, flat),
        vec![
            ("below".to_string(), range(0.0, 12.0)),
            ("above".to_string(), range(0.0, 12.0)),
            ("below".to_string(), range(24.0, 24.0)),
        ]
    );
    assert_copied_apart(&document, children[0], below_held);
    assert_copied_apart(&document, children[1], above_held);
    assert_copied_apart(&document, children[2], below_held);
    // And the scratch copies went with everything they held.
    assert_eq!(document.len() - before, owned_count(&document, flat));
}

#[test]
fn trimming_a_track_holding_a_cycle_is_refused_and_leaves_nothing_behind() {
    // Upstream's `clone()` cannot copy a cycle and reports OBJECT_CYCLE; its
    // `track_trimmed_to_range` passes that on, having changed nothing.
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 24.0);
    hold_itself(&mut document, a);
    let sequence = track(&mut document, "Sequence", &[a]);
    let before = document.len();

    assert_eq!(
        track_trimmed_to_range(&mut document, sequence, range(0.0, 12.0)),
        Err(Error::ObjectCycle {
            schema: "Clip".to_string()
        })
    );
    assert_eq!(document.len(), before);
}

#[test]
fn flattening_a_track_holding_a_cycle_is_refused_and_leaves_nothing_behind() {
    // Upstream's `flatten_stack` sets OBJECT_CYCLE when it fails to copy the
    // clip and then appends the copy it did not get, which crashes. The
    // error it had set is what is reported here.
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 24.0);
    hold_itself(&mut document, a);
    let sequence = track(&mut document, "Sequence", &[a]);
    let layers = stack(&mut document, &[sequence]);
    let before = document.len();

    assert_eq!(
        flatten_stack(&mut document, layers),
        Err(Error::ObjectCycle {
            schema: "Clip".to_string()
        })
    );
    assert_eq!(document.len(), before);
}

#[test]
fn padding_a_track_holding_a_cycle_is_refused_and_leaves_nothing_behind() {
    // A short track is padded by a copy, which upstream refuses cleanly with
    // OBJECT_CYCLE before anything is flattened.
    let mut document = Document::new();
    let long = clip(&mut document, "long", 0.0, 48.0);
    let lower = track(&mut document, "lower", &[long]);
    let a = clip(&mut document, "A", 0.0, 24.0);
    hold_itself(&mut document, a);
    let upper = track(&mut document, "upper", &[a]);
    let before = document.len();

    assert_eq!(
        flatten_tracks(&mut document, &[lower, upper]),
        Err(Error::ObjectCycle {
            schema: "Clip".to_string()
        })
    );
    assert_eq!(document.len(), before);
}

#[test]
fn a_cycle_met_partway_through_flattening_leaves_nothing_behind() {
    // The top track's first clip is copied onto the flattened track before
    // the gap after it sends the search down to the track holding the cycle.
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 48.0);
    hold_itself(&mut document, a);
    let lower = track(&mut document, "lower", &[a]);
    let top_clip = clip(&mut document, "top", 0.0, 24.0);
    let hole = gap(&mut document, 24.0);
    let upper = track(&mut document, "upper", &[top_clip, hole]);
    let before = document.len();

    assert!(matches!(
        flatten_tracks(&mut document, &[lower, upper]),
        Err(Error::ObjectCycle { .. })
    ));
    assert_eq!(document.len(), before);
}

#[test]
fn trimming_through_a_transition_leaves_nothing_behind() {
    let mut document = Document::new();
    let a = clip(&mut document, "A", 0.0, 50.0);
    let dissolve = transition(&mut document, 12.0, 20.0);
    let b = clip(&mut document, "B", 0.0, 50.0);
    let sequence = track(&mut document, "Sequence1", &[a, dissolve, b]);
    let before = document.len();

    assert_eq!(
        track_trimmed_to_range(&mut document, sequence, range(45.0, 50.0)),
        Err(Error::CannotTrimTransition)
    );
    assert_eq!(document.len(), before);
}

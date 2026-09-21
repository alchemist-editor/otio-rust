//! Flattening a stack, and trimming a track.
//!
//! The fixtures and expectations here are ported from upstream's
//! `tests/test_stack_algo.py` and `tests/test_track_algo.py`. Where a name
//! appears — `trackABC`, `trackDgE` — it is upstream's name for the same
//! arrangement, so a failure can be read against the test it came from.

mod common;

use common::{clip, disabled_clip, gap, range, stack, summarize, time, track, transition};

use otio_core::algorithm::{flatten_stack, flatten_tracks, track_trimmed_to_range};
use otio_core::{Document, Error};

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

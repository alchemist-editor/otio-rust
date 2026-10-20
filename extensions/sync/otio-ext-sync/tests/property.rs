//! Furnace's `property_test.go`, as seeded deterministic loops in place of
//! `rapid`: random edits to random timelines diff and apply back to the
//! edited timeline, and injected problems fall back for the right reason.

mod common;

use std::collections::BTreeMap;

use common::{J, Rng, SyntheticState, SyntheticTrack, render_synthetic_timeline};
use otio_ext_sync::{Op, Options, Snapshot, Stats, apply, diff, diff_with_options, json_eq};

const CASES: u64 = 400;

fn draw_synthetic_state(rng: &mut Rng) -> SyntheticState {
    let track_count = rng.range(1, 4);
    let mut state = SyntheticState {
        next_track_id: track_count as i64 + 1,
        next_clip_id: 1,
        leading_gap_on: rng.bool(),
        inter_gaps_on: rng.bool(),
        ..SyntheticState::default()
    };
    for track_index in 0..track_count {
        let mut track = SyntheticTrack {
            id: track_index as i64 + 1,
            clips: Vec::new(),
        };
        for _ in 0..rng.range(0, 6) {
            let id = state.next_clip_id;
            state.next_clip_id += 1;
            track.clips.push(id);
            state.names.insert(id, format!("clip-{id}"));
        }
        state.tracks.push(track);
    }
    state
}

/// A random clip, as (track index, clip index), if any track has one.
fn draw_existing_clip(rng: &mut Rng, state: &SyntheticState) -> Option<(usize, usize)> {
    let nonempty: Vec<usize> = (0..state.tracks.len())
        .filter(|&i| !state.tracks[i].clips.is_empty())
        .collect();
    if nonempty.is_empty() {
        return None;
    }
    let track = nonempty[rng.range(0, nonempty.len() - 1)];
    let clip = rng.range(0, state.tracks[track].clips.len() - 1);
    Some((track, clip))
}

fn apply_random_edit(rng: &mut Rng, state: &mut SyntheticState, edit_number: i64) {
    match rng.range(0, 7) {
        0 => {
            // Insert a clip.
            let track = rng.range(0, state.tracks.len() - 1);
            let at = rng.range(0, state.tracks[track].clips.len());
            let id = state.next_clip_id;
            state.next_clip_id += 1;
            state.names.insert(id, format!("clip-{id}"));
            state.tracks[track].clips.insert(at, id);
        }
        1 => {
            // Remove a clip.
            if let Some((track, clip)) = draw_existing_clip(rng, state) {
                let id = state.tracks[track].clips.remove(clip);
                state.names.remove(&id);
                state.metadata.remove(&id);
            }
        }
        2 => {
            // Move a clip within or across tracks.
            if let Some((track, clip)) = draw_existing_clip(rng, state) {
                let id = state.tracks[track].clips.remove(clip);
                let destination = rng.range(0, state.tracks.len() - 1);
                let at = rng.range(0, state.tracks[destination].clips.len());
                state.tracks[destination].clips.insert(at, id);
            }
        }
        3 => {
            // Insert a track, sometimes adopting an existing clip.
            let mut track = SyntheticTrack {
                id: state.next_track_id,
                clips: Vec::new(),
            };
            state.next_track_id += 1;
            if let Some((source, clip)) = draw_existing_clip(rng, state) {
                if rng.bool() {
                    track.clips = vec![state.tracks[source].clips.remove(clip)];
                }
            }
            let at = rng.range(0, state.tracks.len());
            state.tracks.insert(at, track);
        }
        4 => {
            // Remove a track.
            if state.tracks.len() > 1 {
                let at = rng.range(0, state.tracks.len() - 1);
                for id in state.tracks.remove(at).clips {
                    state.names.remove(&id);
                    state.metadata.remove(&id);
                }
            }
        }
        5 => {
            // Rename a clip.
            if let Some((track, clip)) = draw_existing_clip(rng, state) {
                let id = state.tracks[track].clips[clip];
                state
                    .names
                    .insert(id, format!("renamed-{id}-{edit_number}"));
            }
        }
        6 => {
            // Add or change a metadata namespace, with an integer past what
            // a double holds exactly.
            if let Some((track, clip)) = draw_existing_clip(rng, state) {
                let id = state.tracks[track].clips[clip];
                state.metadata.entry(id).or_default().insert(
                    format!("namespace{}", edit_number % 3),
                    J::object([
                        ("edit", J::Int(edit_number)),
                        ("large", J::Int(9_007_199_254_740_993)),
                    ]),
                );
            }
        }
        _ => {
            // Remove a metadata namespace.
            if let Some((track, clip)) = draw_existing_clip(rng, state) {
                let id = state.tracks[track].clips[clip];
                if let Some(namespaces) = state.metadata.get_mut(&id) {
                    if let Some(key) = namespaces.keys().next().cloned() {
                        namespaces.remove(&key);
                    }
                }
            }
        }
    }
}

fn assert_fallback_reason(stats: &Stats, contains: &str, case: u64) {
    assert!(
        stats.fell_back && stats.fallback_reason.contains(contains),
        "case {case}: expected fallback reason containing {contains:?}, got {stats:?}"
    );
}

#[test]
fn diff_apply_property() {
    const INJECTIONS: [&str; 7] = [
        "normal",
        "normal",
        "normal",
        "generation",
        "sequence",
        "guidless",
        "duplicate",
    ];
    let mut patched = 0;
    for case in 0..CASES {
        let mut rng = Rng::new(case);
        let old_state = draw_synthetic_state(&mut rng);
        let mut new_state = old_state.clone();
        for edit in 0..rng.range(1, 10) {
            apply_random_edit(&mut rng, &mut new_state, edit as i64);
        }
        let injection = INJECTIONS[rng.range(0, INJECTIONS.len() - 1)];

        let old_timeline = render_synthetic_timeline(&old_state, "main", "");
        let new_sequence = if injection == "sequence" {
            "other"
        } else {
            "main"
        };
        let new_generation = if injection == "generation" { 1 } else { 2 };
        let new_timeline = render_synthetic_timeline(&new_state, new_sequence, injection);
        let old = Snapshot {
            sequence_id: "main".into(),
            generation: 1,
            timeline: old_timeline.clone(),
        };
        let new = Snapshot {
            sequence_id: new_sequence.into(),
            generation: new_generation,
            timeline: new_timeline.clone(),
        };

        let (patches, stats) = diff(&old, &new)
            .unwrap_or_else(|error| panic!("case {case}: diff failed on valid JSON: {error}"));
        let applied = apply(&old_timeline, &patches).unwrap_or_else(|error| {
            panic!("case {case}: apply failed: {error}\npatches: {patches:?}")
        });
        assert!(
            json_eq(&applied, &new_timeline).unwrap(),
            "case {case}: round trip mismatch\ninjection={injection}\npatches={patches:?}\nold={old_timeline}\nnew={new_timeline}\ngot={applied}"
        );
        if !stats.fell_back {
            patched += 1;
        }

        match injection {
            "generation" => assert_fallback_reason(&stats, "generation regression", case),
            "sequence" => assert_fallback_reason(&stats, "sequenceId changed", case),
            "guidless" => assert_fallback_reason(&stats, "guidless", case),
            "duplicate" => assert_fallback_reason(&stats, "duplicate guid", case),
            _ => {}
        }
    }
    // Most normal cases should make patches rather than fall back, or the
    // loop is not testing the diff.
    assert!(
        patched > CASES / 5,
        "only {patched} of {CASES} cases made patches"
    );
}

#[test]
fn op_count_threshold_property() {
    for case in 0..CASES {
        let mut rng = Rng::new(case);
        let clip_count = rng.range(2, 12) as i64;
        let mut state = SyntheticState {
            tracks: vec![SyntheticTrack {
                id: 1,
                clips: (1..=clip_count).collect(),
            }],
            names: (1..=clip_count)
                .map(|id| (id, format!("clip-{id}")))
                .collect(),
            metadata: BTreeMap::new(),
            next_track_id: 2,
            next_clip_id: clip_count + 1,
            ..SyntheticState::default()
        };
        let old_timeline = render_synthetic_timeline(&state, "main", "");
        for id in 1..=clip_count {
            state.names.insert(id, format!("changed-{id}"));
        }
        let new_timeline = render_synthetic_timeline(&state, "main", "");
        let (patches, stats) = diff_with_options(
            &Snapshot {
                sequence_id: "main".into(),
                generation: 1,
                timeline: old_timeline.clone(),
            },
            &Snapshot {
                sequence_id: "main".into(),
                generation: 2,
                timeline: new_timeline.clone(),
            },
            &Options {
                max_ops: 1,
                max_ops_fraction: 100.0,
            },
        )
        .unwrap();
        assert_fallback_reason(&stats, "op count threshold exceeded", case);
        assert!(
            patches.len() == 1 && patches[0].op == Op::ReplaceTimeline,
            "case {case}: expected sole replacement patch, got {patches:?}"
        );
        let applied = apply(&old_timeline, &patches).unwrap();
        assert!(
            json_eq(&applied, &new_timeline).unwrap(),
            "case {case}: threshold fallback did not round trip"
        );
    }
}

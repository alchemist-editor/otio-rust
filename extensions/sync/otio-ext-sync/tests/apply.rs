//! Furnace's `apply_test.go`: malformed patches are refused with a clear
//! message, null differs from removal, and moves cost what they should.

mod common;

use std::collections::BTreeMap;

use common::{
    J, SyntheticState, SyntheticTrack, count_ops, render_benchmark_timeline,
    render_synthetic_timeline, synthetic_node,
};
use otio_core::json::{self, Value};
use otio_ext_sync::{Op, Options, Patch, Snapshot, apply, diff, diff_with_options, json_eq};

fn state(
    tracks: Vec<SyntheticTrack>,
    names: &[(i64, &str)],
    next_track_id: i64,
    next_clip_id: i64,
) -> SyntheticState {
    SyntheticState {
        tracks,
        names: names
            .iter()
            .map(|(id, name)| (*id, (*name).to_owned()))
            .collect(),
        metadata: BTreeMap::new(),
        next_track_id,
        next_clip_id,
        ..SyntheticState::default()
    }
}

fn track(id: i64, clips: &[i64]) -> SyntheticTrack {
    SyntheticTrack {
        id,
        clips: clips.to_vec(),
    }
}

fn valid_test_clip(guid: &str) -> String {
    synthetic_node("Clip.2", guid, [("name", J::str("inserted"))]).to_json()
}

fn patch(op: Op) -> Patch {
    Patch {
        op,
        ..Patch::default()
    }
}

fn set(guid: &str, path: &str, value: Option<&str>) -> Patch {
    Patch {
        op: Op::SetProperty,
        guid: guid.into(),
        path: path.into(),
        value: value.map(str::to_owned),
        ..Patch::default()
    }
}

#[test]
fn apply_rejects_malformed_patches() {
    let timeline = render_synthetic_timeline(
        &state(vec![track(1, &[1, 2])], &[(1, "one"), (2, "two")], 2, 3),
        "main",
        "",
    );
    let cases: Vec<(&str, Vec<Patch>, &str)> = vec![
        (
            "unknown op",
            vec![patch(Op::Other("wat".into()))],
            "unknown patch op",
        ),
        (
            "insert missing fields",
            vec![patch(Op::InsertChild)],
            "requires",
        ),
        (
            "insert missing parent",
            vec![Patch {
                parent_guid: "missing".into(),
                index: Some(0),
                node: Some("{}".into()),
                ..patch(Op::InsertChild)
            }],
            "does not exist",
        ),
        (
            "insert out of range",
            vec![Patch {
                parent_guid: "main/trk-1".into(),
                index: Some(99),
                node: Some(valid_test_clip("main/clip-3")),
                ..patch(Op::InsertChild)
            }],
            "out of range",
        ),
        (
            "move missing guid",
            vec![Patch {
                parent_guid: "main/trk-1".into(),
                guid: "missing".into(),
                index: Some(0),
                ..patch(Op::MoveChild)
            }],
            "does not exist",
        ),
        (
            "move out of range",
            vec![Patch {
                parent_guid: "main/trk-1".into(),
                guid: "main/clip-1".into(),
                index: Some(99),
                ..patch(Op::MoveChild)
            }],
            "out of range",
        ),
        (
            "remove missing guid",
            vec![Patch {
                parent_guid: "main/trk-1".into(),
                guid: "missing".into(),
                ..patch(Op::RemoveChild)
            }],
            "does not exist",
        ),
        (
            "remove wrong parent",
            vec![Patch {
                parent_guid: "main/stack".into(),
                guid: "main/clip-1".into(),
                ..patch(Op::RemoveChild)
            }],
            "is not a child",
        ),
        (
            "set missing guid",
            vec![set("missing", "/name", Some("\"x\""))],
            "does not exist",
        ),
        (
            "set missing path",
            vec![set("main/clip-1", "", Some("\"x\""))],
            "requires",
        ),
        (
            "set deep path",
            vec![set(
                "main/clip-1",
                "/source_range/duration/value",
                Some("1"),
            )],
            "unsupported",
        ),
        (
            "set malformed escape",
            vec![set("main/clip-1", "/bad~2key", Some("1"))],
            "invalid",
        ),
        (
            "set metadata sync",
            vec![set("main/clip-1", "/metadata/sync", Some("{}"))],
            "cannot be patched",
        ),
        (
            "set missing value",
            vec![set("main/clip-1", "/name", None)],
            "valid value",
        ),
        (
            "set remove with value",
            vec![Patch {
                removed: true,
                ..set("main/clip-1", "/name", Some("null"))
            }],
            "must omit value",
        ),
        (
            "replace alongside op",
            vec![
                Patch {
                    timeline: Some(timeline.clone()),
                    ..patch(Op::ReplaceTimeline)
                },
                set("main/clip-1", "/name", Some("\"x\"")),
            ],
            "sole patch",
        ),
        (
            "replace invalid timeline",
            vec![Patch {
                timeline: Some("{".into()),
                ..patch(Op::ReplaceTimeline)
            }],
            "valid timeline",
        ),
    ];
    for (name, patches, want) in cases {
        match apply(&timeline, &patches) {
            Err(error) => assert!(
                error.to_string().contains(want),
                "{name}: expected error containing {want:?}, got {error}"
            ),
            Ok(_) => panic!("{name}: expected error containing {want:?}, got success"),
        }
    }
}

/// The first clip of the first track of a timeline.
fn first_clip(timeline: &str) -> Value {
    let value = json::parse(timeline).unwrap();
    let track = &value
        .get("tracks")
        .unwrap()
        .get("children")
        .unwrap()
        .as_array()
        .unwrap()[0];
    track.get("children").unwrap().as_array().unwrap()[0].clone()
}

#[test]
fn apply_set_property_null_and_removal_differ() {
    let timeline = render_synthetic_timeline(
        &state(vec![track(1, &[1])], &[(1, "one")], 2, 2),
        "main",
        "",
    );
    let applied = apply(&timeline, &[set("main/clip-1", "/name", Some("null"))]).unwrap();
    assert_eq!(
        first_clip(&applied).get("name"),
        Some(&Value::Null),
        "JSON null was not retained as a present value"
    );
    let removal = Patch {
        removed: true,
        ..set("main/clip-1", "/name", None)
    };
    let applied = apply(&applied, &[removal]).unwrap();
    assert_eq!(
        first_clip(&applied).get("name"),
        None,
        "removed property is still present"
    );
}

#[test]
fn apply_same_parent_move_accepts_current_length_as_end() {
    let timeline = render_benchmark_timeline(&[1, 2, 3]);
    let applied = apply(
        &timeline,
        &[Patch {
            parent_guid: "main/trk-1".into(),
            guid: "main/clip-1".into(),
            index: Some(3),
            ..patch(Op::MoveChild)
        }],
    )
    .unwrap();
    let want = render_benchmark_timeline(&[2, 3, 1]);
    assert!(
        json_eq(&applied, &want).unwrap(),
        "move-to-end mismatch\nwant={want}\ngot={applied}"
    );
}

#[test]
fn diff_malformed_json_is_only_hard_error() {
    let valid = render_synthetic_timeline(&state(vec![track(1, &[])], &[], 0, 0), "main", "");
    let snapshot = |generation, timeline: &str| Snapshot {
        sequence_id: "main".into(),
        generation,
        timeline: timeline.into(),
    };
    assert!(
        diff(&snapshot(1, "{"), &snapshot(2, &valid)).is_err(),
        "invalid old JSON did not error"
    );
    assert!(
        diff(&snapshot(1, &valid), &snapshot(2, "{")).is_err(),
        "invalid new JSON did not error"
    );
    let (patches, stats) = diff(&snapshot(1, &valid), &snapshot(2, "null")).unwrap();
    assert!(
        stats.fell_back && patches.len() == 1 && patches[0].op == Op::ReplaceTimeline,
        "well-formed but unsupported JSON should fall back: {patches:?} {stats:?}"
    );
}

#[test]
fn single_displacement_uses_one_move() {
    const CHILD_COUNT: i64 = 20;
    let old_order: Vec<i64> = (1..=CHILD_COUNT).collect();
    let old_timeline = render_benchmark_timeline(&old_order);
    for source in 0..old_order.len() {
        for destination in 0..old_order.len() {
            if source == destination {
                continue;
            }
            let mut permutation = old_order.clone();
            let moved = permutation.remove(source);
            permutation.insert(destination, moved);
            let new_timeline = render_benchmark_timeline(&permutation);
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
                    max_ops: 100,
                    max_ops_fraction: 100.0,
                },
            )
            .unwrap();
            assert!(
                !stats.fell_back,
                "permutation {permutation:?} did not diff: {stats:?}"
            );
            assert!(
                count_ops(&patches, &Op::MoveChild) == 1 && patches.len() == 1,
                "move {source} to {destination}: want one move, got {patches:?}"
            );
            let applied = apply(&old_timeline, &patches).unwrap();
            assert!(
                json_eq(&applied, &new_timeline).unwrap(),
                "permutation {permutation:?} did not round trip"
            );
        }
    }
}

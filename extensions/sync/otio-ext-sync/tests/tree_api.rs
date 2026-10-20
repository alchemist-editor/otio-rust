//! Furnace's `tree_api_test.go`: a parsed [`Tree`] can be diffed again and
//! again, from many threads, and keeps its snapshot's sequence id and
//! generation.

mod common;

use common::render_benchmark_timeline;
use otio_ext_sync::{Error, Op, Options, Snapshot, Tree, diff_trees, diff_with_options};

fn single_move_snapshots() -> (Snapshot, Snapshot) {
    const CLIP_COUNT: i64 = 5000;
    let old_order: Vec<i64> = (1..=CLIP_COUNT).collect();
    let mut new_order = vec![CLIP_COUNT];
    new_order.extend_from_slice(&old_order[..old_order.len() - 1]);
    (
        Snapshot {
            sequence_id: "main".into(),
            generation: 1,
            timeline: render_benchmark_timeline(&old_order),
        },
        Snapshot {
            sequence_id: "main".into(),
            generation: 2,
            timeline: render_benchmark_timeline(&new_order),
        },
    )
}

#[test]
fn diff_trees_reusable_and_non_mutating() {
    let (old_snapshot, new_snapshot) = single_move_snapshots();
    let old_tree = Tree::parse(&old_snapshot).unwrap();
    let new_tree = Tree::parse(&new_snapshot).unwrap();
    let (want_patches, want_stats) =
        diff_with_options(&old_snapshot, &new_snapshot, &Options::default()).unwrap();
    assert_eq!(want_patches.len(), 1);
    assert_eq!(want_patches[0].op, Op::MoveChild);

    std::thread::scope(|scope| {
        let calls: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| diff_trees(&old_tree, &new_tree, &Options::default())))
            .collect();
        for call in calls {
            let (patches, stats) = call.join().unwrap();
            assert_eq!(patches, want_patches);
            assert_eq!(stats, want_stats);
        }
    });
    // A diff borrows its trees immutably, so neither can have changed; check
    // the snapshot they keep all the same.
    assert_eq!(old_tree.timeline(), old_snapshot.timeline);
    assert_eq!(new_tree.timeline(), new_snapshot.timeline);
}

#[test]
fn diff_trees_retains_snapshot_metadata() {
    let (old_snapshot, mut new_snapshot) = single_move_snapshots();
    let old_tree = Tree::parse(&old_snapshot).unwrap();
    new_snapshot.sequence_id = "other".into();
    let new_tree = Tree::parse(&new_snapshot).unwrap();
    assert_eq!(new_tree.sequence_id(), "other");
    assert_eq!(new_tree.generation(), 2);

    let (patches, stats) = diff_trees(&old_tree, &new_tree, &Options::default());
    assert!(
        stats.fell_back
            && stats.fallback_reason == "sequenceId changed"
            && patches.len() == 1
            && patches[0].op == Op::ReplaceTimeline,
        "unexpected fallback: {stats:?} {patches:?}"
    );
}

#[test]
fn parse_reports_invalid_and_unaddressable_timelines() {
    let snapshot = |timeline: &str| Snapshot {
        sequence_id: "main".into(),
        generation: 1,
        timeline: timeline.into(),
    };
    assert_eq!(
        Tree::parse(&snapshot("{")).unwrap_err(),
        Error::InvalidJson("timeline")
    );
    assert_eq!(
        Tree::parse(&snapshot(r#"{"OTIO_SCHEMA":"Timeline.1"}"#))
            .unwrap_err()
            .to_string(),
        "guidless node at Timeline.1"
    );
}

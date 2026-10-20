//! Furnace's otiodiff golden corpus, as its `golden_test.go` runs it: the
//! diff of each case's snapshots must write `patches.json` byte for byte, the
//! stats must say what Furnace's do, and applying the patches to the old
//! timeline must give the new one.

use std::fs;
use std::path::{Path, PathBuf};

use otio_core::json::{self, Value};
use otio_ext_sync::{Op, Snapshot, apply, diff, json_eq, patch};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/otiodiff")
}

/// Reads a fixture snapshot, keeping `otioJson` as the text it is in the
/// file, as Go's `json.RawMessage` keeps it.
fn read_snapshot(path: &Path) -> Snapshot {
    let text = fs::read_to_string(path).unwrap();
    let value = json::parse(&text).unwrap();
    let sequence_id = value.get("sequenceId").and_then(Value::as_str).unwrap();
    let generation = match value.get("generation") {
        Some(Value::Number(json::Number::Int(generation))) => *generation,
        other => panic!("{}: generation is {other:?}", path.display()),
    };
    // `otioJson` is the last member, so its text runs from after its key to
    // the object's closing brace.
    let after_key = text.find("\"otioJson\":").unwrap() + "\"otioJson\":".len();
    let end = text.rfind('}').unwrap();
    let timeline = text[after_key..end].trim().to_owned();
    assert!(
        text[end + 1..].trim().is_empty()
            && json::parse(&timeline).ok().as_ref() == value.get("otioJson"),
        "{}: otioJson is not the last member",
        path.display()
    );
    let snapshot = Snapshot {
        sequence_id: sequence_id.to_owned(),
        generation,
        timeline,
    };
    // The library reads the file the same way, and writes back what it read.
    assert_eq!(Snapshot::from_json(&text).unwrap(), snapshot);
    let written = Snapshot::from_json(&snapshot.to_json()).unwrap();
    assert_eq!(
        (&written.sequence_id, written.generation),
        (&snapshot.sequence_id, snapshot.generation)
    );
    assert!(json_eq(&written.timeline, &snapshot.timeline).unwrap());
    snapshot
}

#[test]
fn golden_corpus() {
    let mut case_count = 0;
    let mut directories: Vec<PathBuf> = fs::read_dir(fixtures())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    for directory in directories {
        case_count += 1;
        let name = directory.file_name().unwrap().to_str().unwrap().to_owned();
        let old = read_snapshot(&directory.join("old.json"));
        let new = read_snapshot(&directory.join("new.json"));
        let want_patches = fs::read_to_string(directory.join("patches.json")).unwrap();

        let (patches, stats) =
            diff(&old, &new).unwrap_or_else(|error| panic!("{name}: diff: {error}"));
        let got_patches = patch::to_json_pretty(&patches) + "\n";
        assert_eq!(got_patches, want_patches, "{name}: patch golden differs");

        // Reading the golden back gives patches that write the same bytes.
        let read_back = patch::from_json(&want_patches).unwrap();
        assert_eq!(
            patch::to_json(&read_back),
            patch::to_json(&patches),
            "{name}: read back"
        );

        let applied =
            apply(&old.timeline, &patches).unwrap_or_else(|error| panic!("{name}: apply: {error}"));
        assert!(
            json_eq(&applied, &new.timeline).unwrap(),
            "{name}: apply result differs semantically\nwant: {}\ngot: {applied}",
            new.timeline
        );

        match name.as_str() {
            "guidless_transition" => assert!(
                stats.fell_back
                    && (stats.fallback_reason.contains("guidless")
                        || stats.fallback_reason.contains("Transition.1")),
                "{name}: expected clear guidless fallback, got {stats:?}"
            ),
            "generation_regression" => assert!(
                stats.fell_back && stats.fallback_reason == "generation regression",
                "{name}: unexpected fallback stats: {stats:?}"
            ),
            "sequence_changed" => assert!(
                stats.fell_back && stats.fallback_reason == "sequenceId changed",
                "{name}: unexpected fallback stats: {stats:?}"
            ),
            "metadata_sync_threshold_fallback" => assert!(
                stats.fell_back
                    && stats
                        .fallback_reason
                        .contains("op count threshold exceeded"),
                "{name}: unexpected fallback stats: {stats:?}"
            ),
            _ => assert!(
                !stats.fell_back,
                "{name}: unexpected fallback: {}",
                stats.fallback_reason
            ),
        }
        if !stats.fell_back {
            assert_eq!(stats.total_ops, patches.len(), "{name}: total ops");
            assert_eq!(
                stats.insert_ops + stats.move_ops + stats.remove_ops + stats.set_property_ops,
                patches.len(),
                "{name}: ops by kind"
            );
        }

        if name == "metadata_namespaces" {
            assert!(
                patches.iter().all(|patch| patch.path != "/metadata/sync"),
                "diff emitted forbidden /metadata/sync patch"
            );
        }
        if name == "adopted_subtree" {
            assert!(
                !patches
                    .iter()
                    .any(|patch| patch.op == Op::MoveChild && patch.guid == "main/clip-10"),
                "adopted clip must be carried by the inserted Track"
            );
        }
    }
    assert_eq!(case_count, 11, "expected 11 golden cases");
}

/// The exact stats of the cases that make patches, so a change in how they
/// are counted shows.
#[test]
fn golden_stats() {
    let cases = [
        // name, insert, move, remove, set_property, node count
        ("simple_insert", 1, 0, 0, 0, 6),
        ("simple_remove", 0, 0, 1, 0, 6),
        ("single_move", 0, 1, 0, 0, 11),
        ("property_change", 0, 0, 0, 1, 6),
        ("metadata_namespaces", 0, 0, 0, 3, 9),
    ];
    for (name, insert, moves, remove, set_property, node_count) in cases {
        let directory = fixtures().join(name);
        let old = read_snapshot(&directory.join("old.json"));
        let new = read_snapshot(&directory.join("new.json"));
        let (_, stats) = diff(&old, &new).unwrap();
        assert_eq!(
            (
                stats.insert_ops,
                stats.move_ops,
                stats.remove_ops,
                stats.set_property_ops,
                stats.node_count
            ),
            (insert, moves, remove, set_property, node_count),
            "{name}: {stats:?}"
        );
    }
}

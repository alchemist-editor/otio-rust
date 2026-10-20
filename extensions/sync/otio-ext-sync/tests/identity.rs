//! GUIDs, and the `document_changed` hook.

use std::sync::Mutex;

use otio_core::{Any, Document, NodeId};
use otio_ext_sync::guid::{self, GuidIndex};
use otio_ext_sync::journal;

const TIMELINE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "identity",
    "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "name": "tracks",
        "children": [
            {
                "OTIO_SCHEMA": "Track.1",
                "name": "V1",
                "kind": "Video",
                "metadata": {"sync": {"guid": "track-1", "lease": "kept"}},
                "children": [
                    {"OTIO_SCHEMA": "Gap.1", "name": "first", "metadata": {"sync": {"guid": "gap"}}},
                    {"OTIO_SCHEMA": "Gap.1", "name": "copy", "metadata": {"sync": {"guid": "gap"}}},
                    {"OTIO_SCHEMA": "Gap.1", "name": "bare"}
                ]
            }
        ]
    }
}"#;

fn read() -> (Document, NodeId) {
    let document = otio_core::from_str(TIMELINE).unwrap();
    let root = document.root().unwrap();
    (document, root)
}

#[test]
fn stamping_fills_in_missing_and_repeated_guids_and_keeps_the_rest() {
    let (mut document, root) = read();
    let objects = guid::synced_objects(&document, root);
    // The timeline, its stack, the track and three gaps, parents first.
    assert_eq!(objects.len(), 6);
    let [timeline, stack, track, first, copy, bare] = objects[..] else {
        unreachable!()
    };

    let stamped = guid::stamp(&mut document, root).unwrap();
    assert_eq!(stamped, vec![timeline, stack, copy, bare]);
    assert_eq!(guid::guid(&document, track), Some("track-1"));
    assert_eq!(guid::guid(&document, first), Some("gap"));
    assert_ne!(guid::guid(&document, copy), Some("gap"));

    // Other fields in the namespace survive a new GUID.
    guid::set_guid(&mut document, track, "track-2").unwrap();
    let text = otio_core::to_string(&document).unwrap();
    assert!(
        text.contains("\"lease\"") && text.contains("\"kept\""),
        "{text}"
    );

    // Once stamped, every object is found by its GUID, and stamping again
    // changes nothing.
    let index = GuidIndex::build(&document, root).unwrap();
    assert_eq!(index.len(), 6);
    for id in [timeline, stack, track, first, copy, bare] {
        assert_eq!(index.get(guid::guid(&document, id).unwrap()), Some(id));
    }
    assert!(guid::stamp(&mut document, root).unwrap().is_empty());
}

#[test]
fn an_index_refuses_a_repeated_guid() {
    let (document, root) = read();
    let error = GuidIndex::build(&document, root).unwrap_err();
    assert_eq!(error.guid, "gap");
}

#[test]
fn new_guids_are_version_4_uuids_and_differ() {
    let one = guid::new_guid();
    let two = guid::new_guid();
    assert_ne!(one, two);
    assert_eq!(one.len(), 36);
    assert_eq!(&one[14..15], "4");
    assert!(matches!(&one[19..20], "8" | "9" | "a" | "b"));
}

#[test]
fn committing_runs_document_changed_with_the_touched_guids() {
    static SEEN: Mutex<Vec<Any>> = Mutex::new(Vec::new());
    otio_ext_sync::install();
    otio_ext::enable(otio_ext_sync::ID).unwrap();
    {
        let mut registry = otio_adapter::plugins::registry();
        registry.register_hook_script(
            "record",
            otio_adapter::plugins::HookScript::new(|_, target, arguments| {
                SEEN.lock()
                    .unwrap()
                    .push(arguments.get("touched").cloned().unwrap());
                Ok(target)
            }),
        );
        registry.attach_hook_script(journal::DOCUMENT_CHANGED, "record");
    }

    let (mut document, root) = read();
    guid::stamp(&mut document, root).unwrap();
    let bare = guid::synced_objects(&document, root)[5];
    let track = guid::synced_objects(&document, root)[2];

    // Nothing journalled, nothing to announce.
    assert!(journal::commit(&mut document).unwrap().is_empty());
    assert!(SEEN.lock().unwrap().is_empty());

    document.start_journal();
    let index = document
        .get(track)
        .unwrap()
        .children()
        .unwrap()
        .iter()
        .position(|child| *child == bare)
        .unwrap();
    document
        .remove_child(track, i64::try_from(index).unwrap())
        .unwrap();
    let changes = journal::commit(&mut document).unwrap();
    assert!(changes.touched.contains(&track));
    assert_eq!(
        SEEN.lock().unwrap().as_slice(),
        [Any::Vector(vec![
            Any::String("track-1".to_string()),
            Any::String(guid::guid(&document, bare).unwrap().to_string()),
        ])]
    );
}

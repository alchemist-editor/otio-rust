//! The change journal: which objects an edit touched, whichever API made it.

use otio_core::schema::{Gap, Node};
use otio_core::{Document, NodeId};

const TIMELINE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "journal",
    "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "name": "tracks",
        "children": [
            {
                "OTIO_SCHEMA": "Track.1",
                "name": "V1",
                "kind": "Video",
                "children": [
                    {"OTIO_SCHEMA": "Gap.1", "name": "first"},
                    {"OTIO_SCHEMA": "Gap.1", "name": "second"}
                ]
            }
        ]
    }
}"#;

fn track(document: &Document) -> NodeId {
    let Some(Node::Timeline(timeline)) = document.get(document.root().unwrap()) else {
        panic!("the root is a timeline");
    };
    document
        .get(timeline.tracks.unwrap())
        .unwrap()
        .children()
        .unwrap()[0]
}

#[test]
fn a_document_records_nothing_until_asked() {
    let mut document = otio_core::from_str(TIMELINE).unwrap();
    assert!(!document.is_journalling());
    let track = track(&document);
    document.remove_child(track, 0).unwrap();
    assert!(document.take_changes().is_empty());
}

#[test]
fn every_kind_of_edit_is_recorded_once() {
    let mut document = otio_core::from_str(TIMELINE).unwrap();
    let track = track(&document);
    let first = document.get(track).unwrap().children().unwrap()[0];
    document.start_journal();

    // A rename through a mutable borrow, a structural edit, a new object and
    // a removal.
    if let Some(base) = document.get_mut(first).unwrap().base_mut() {
        base.name = "renamed".to_string();
    }
    let taken = document.remove_child(track, 1).unwrap();
    document.remove(taken).unwrap();
    let added = document.insert(Node::Gap(Gap::default()));
    document.append_child(track, added).unwrap();

    let changes = document.take_changes();
    for id in [first, track, added] {
        assert_eq!(
            changes
                .touched
                .iter()
                .filter(|touched| **touched == id)
                .count(),
            1,
            "{id:?} is recorded once"
        );
    }
    assert_eq!(changes.removed, vec![taken]);
    assert!(!changes.root_changed);

    // Taking the changes starts a fresh record.
    assert!(document.take_changes().is_empty());
    document.set_root(document.root());
    assert!(document.stop_journal().root_changed);
    assert!(!document.is_journalling());
}

#[test]
fn absorbed_objects_are_recorded() {
    let mut document = Document::new();
    document.start_journal();
    let other = otio_core::from_str(TIMELINE).unwrap();
    let count = other.len();
    let moved = document.absorb(other);
    let changes = document.stop_journal();
    assert_eq!(changes.touched.len(), count);
    assert!(moved.values().all(|id| changes.touched.contains(id)));
}

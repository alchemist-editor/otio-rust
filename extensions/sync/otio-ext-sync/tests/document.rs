//! Transactions and applying patches to an open document.

use otio_core::schema::{Clip, Node};
use otio_core::{Document, NodeId};
use otio_ext_sync::document::{self, Transaction};
use otio_ext_sync::guid::{self, GuidIndex};
use otio_ext_sync::{Op, json_eq};

const TIMELINE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "transaction",
    "tracks": {
        "OTIO_SCHEMA": "Stack.1",
        "children": [
            {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
                {"OTIO_SCHEMA": "Gap.1", "name": "a"},
                {"OTIO_SCHEMA": "Gap.1", "name": "b"}
            ]},
            {"OTIO_SCHEMA": "Track.1", "name": "V2", "kind": "Video", "children": []},
            {"OTIO_SCHEMA": "Track.1", "name": "A1", "kind": "Audio", "children": [
                {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"},
                {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"}
            ]}
        ]
    }
}"#;

fn to_compact(document: &Document) -> String {
    let timeline = document.root().unwrap();
    document::snapshot(document, timeline, "", 0)
        .unwrap()
        .timeline
}

fn tracks(document: &Document) -> (NodeId, NodeId) {
    let objects = guid::synced_objects(document, document.root().unwrap());
    let tracks: Vec<NodeId> = objects
        .into_iter()
        .filter(|id| matches!(document.get(*id), Some(Node::Track(_))))
        .collect();
    (tracks[0], tracks[1])
}

#[test]
fn a_transaction_turns_edits_into_patches_another_copy_can_apply() {
    let mut editor = otio_core::from_str(TIMELINE).unwrap();
    let timeline = editor.root().unwrap();
    let transaction = Transaction::begin(&mut editor, timeline, "main", 1).unwrap();
    // Every peer starts from the same stamped timeline.
    let mut peer = otio_core::from_str(&transaction.before().timeline).unwrap();

    let (v1, v2) = tracks(&editor);
    let moved = editor.remove_child(v1, 0).unwrap();
    editor.append_child(v2, moved).unwrap();
    let clip = editor.insert(Node::Clip(Clip::default()));
    editor.append_child(v1, clip).unwrap();

    let patches = transaction.commit(&mut editor).unwrap();
    // Furnace's diff, faithfully ported, also shuffles V1 to place the new
    // clip, so there are more moves than the one edit made.
    assert!(
        patches.iter().any(|patch| patch.op == Op::InsertChild)
            && patches
                .iter()
                .any(|patch| patch.op == Op::MoveChild && patch.parent_guid != patch.guid)
            && patches.iter().all(|patch| patch.op != Op::ReplaceTimeline),
        "{patches:?}"
    );

    // Handles the peer holds before the patches arrive.
    let peer_root = peer.root().unwrap();
    let (peer_v1, peer_v2) = tracks(&peer);
    let moved_guid = guid::guid(&editor, moved).unwrap().to_string();
    let peer_moved = GuidIndex::build(&peer, peer_root)
        .unwrap()
        .get(&moved_guid)
        .unwrap();
    let count = peer.len();

    document::apply_to(&mut peer, peer_root, &patches).unwrap();

    // They still name the same objects, now where the patches put them.
    assert_eq!(peer.root(), Some(peer_root));
    assert_eq!(tracks(&peer), (peer_v1, peer_v2));
    assert_eq!(peer.get(peer_moved).unwrap().parent(), Some(peer_v2));
    assert_eq!(peer.len(), count + 1, "only the new clip was added");
    assert!(
        json_eq(
            &otio_core::to_string(&peer).unwrap(),
            &otio_core::to_string(&editor).unwrap()
        )
        .unwrap()
    );
    // The new clip has a GUID, the same on both sides.
    let new_guid = guid::guid(&editor, clip).unwrap().to_string();
    let index = GuidIndex::build(&peer, peer.root().unwrap()).unwrap();
    assert!(matches!(
        peer.get(index.get(&new_guid).unwrap()),
        Some(Node::Clip(_))
    ));
}

#[test]
fn a_transaction_with_no_change_makes_no_patches() {
    let mut document = otio_core::from_str(TIMELINE).unwrap();
    let timeline = document.root().unwrap();
    let transaction = Transaction::begin(&mut document, timeline, "main", 1).unwrap();
    assert!(transaction.commit(&mut document).unwrap().is_empty());
}

#[test]
fn rolling_back_restores_the_document() {
    let mut document = otio_core::from_str(TIMELINE).unwrap();
    let timeline = document.root().unwrap();
    let transaction = Transaction::begin(&mut document, timeline, "main", 1).unwrap();
    let before = transaction.before().timeline.clone();
    let (v1, _) = tracks(&document);
    document.remove_child(v1, 0).unwrap();
    let count = document.len();
    transaction.rollback(&mut document).unwrap();
    assert!(json_eq(&to_compact(&document), &before).unwrap());
    assert_eq!(tracks(&document).0, v1, "the track keeps its handle");
    assert_eq!(document.len(), count + 1, "the removed gap is back");
}

#[test]
fn a_failed_apply_leaves_the_document_alone() {
    let mut document = otio_core::from_str(TIMELINE).unwrap();
    let root = document.root().unwrap();
    otio_ext_sync::guid::stamp(&mut document, root).unwrap();
    let before = otio_core::to_string(&document).unwrap();
    let patches = otio_ext_sync::patch::from_json(
        r#"[{"op":"remove_child","parentGuid":"nowhere","guid":"nothing"}]"#,
    )
    .unwrap();
    assert!(document::apply_to(&mut document, root, &patches).is_err());
    assert_eq!(otio_core::to_string(&document).unwrap(), before);
}

//! Upstream's base classes, read and written as schemas in their own right.
//!
//! `Composable`, `Composition`, `MediaReference`, `SerializableObject` and
//! `SerializableObjectWithMetadata` are registered in upstream's type registry
//! alongside `Clip` and `Track`, and its Python API constructs them directly —
//! its own `test_composable.py` opens by building a bare `Composable`. So a
//! file may legitimately contain one, and reading it as an opaque blob would
//! lose the ability to ask it anything.

use otio_core::Any;
use otio_core::schema::Node;
use otio_core::{Document, Error};

/// Reads one object and returns the document and its root.
fn read(json: &str) -> (Document, otio_core::NodeId) {
    let document = otio_core::from_str(json).expect("parses");
    let root = document.root().expect("has a root");
    (document, root)
}

/// Reads, writes and reads again, returning what the second read produced.
fn round_trip(json: &str) -> (Document, otio_core::NodeId) {
    let (document, _) = read(json);
    let written = otio_core::to_string(&document).expect("writes");
    read(&written)
}

#[test]
fn a_serializable_object_carries_nothing_but_its_schema() {
    let (document, root) = round_trip(r#"{ "OTIO_SCHEMA": "SerializableObject.1" }"#);
    assert!(matches!(
        document.try_get(root).unwrap(),
        Node::SerializableObject
    ));
    let written = otio_core::to_string(&document).expect("writes");
    assert!(written.contains("\"OTIO_SCHEMA\": \"SerializableObject.1\""));
}

#[test]
fn a_composable_keeps_its_name_and_metadata() {
    let (document, root) = round_trip(
        r#"{
    "OTIO_SCHEMA": "Composable.1",
    "name": "test",
    "metadata": { "foo": "bar" }
}"#,
    );
    let node = document.try_get(root).unwrap();
    assert!(matches!(node, Node::Composable(_)));
    assert_eq!(node.name(), "test");
    assert_eq!(
        node.base().unwrap().metadata.get("foo"),
        Some(&Any::String("bar".to_string()))
    );
}

#[test]
fn an_object_with_metadata_is_not_confused_with_a_composable() {
    // The two serialize identically — upstream's `Composable::write_to` only
    // calls its parent's — so the schema label is the whole difference, and
    // it has to survive the trip.
    let (document, root) =
        round_trip(r#"{ "OTIO_SCHEMA": "SerializableObjectWithMetadata.1", "name": "m" }"#);
    assert!(matches!(
        document.try_get(root).unwrap(),
        Node::SerializableObjectWithMetadata(_)
    ));
    let written = otio_core::to_string(&document).expect("writes");
    assert!(written.contains("\"SerializableObjectWithMetadata.1\""));
}

#[test]
fn a_media_reference_keeps_its_available_range() {
    let (document, root) = round_trip(
        r#"{
    "OTIO_SCHEMA": "MediaReference.1",
    "name": "somewhere",
    "available_range": {
        "OTIO_SCHEMA": "TimeRange.1",
        "duration": { "OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 10 },
        "start_time": { "OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 0 }
    }
}"#,
    );
    let Node::MediaReference(media) = document.try_get(root).unwrap() else {
        panic!("a MediaReference");
    };
    assert_eq!(media.available_range.unwrap().duration().value(), 10.0);
}

#[test]
fn a_bare_composition_holds_children_but_will_not_place_them() {
    // Upstream's `Composition` is the base class `Track` and `Stack` derive
    // from. It holds children, so editing works, but it says nothing about
    // where they sit: upstream's own `range_of_child_at_index` on the base
    // class reports NOT_IMPLEMENTED rather than guessing.
    let (mut document, root) = round_trip(
        r#"{
    "OTIO_SCHEMA": "Composition.1",
    "name": "bag",
    "children": [
        { "OTIO_SCHEMA": "Clip.2", "name": "a", "media_references": {}, "active_media_reference_key": "DEFAULT_MEDIA" }
    ]
}"#,
    );
    assert!(matches!(
        document.try_get(root).unwrap(),
        Node::Composition(_)
    ));
    assert_eq!(document.children_of(root).unwrap().len(), 1);

    assert_eq!(document.range_of_all_children(root), Err(Error::NoLayout));

    // Editing still works, because it does not need to know where anything
    // sits.
    document
        .remove_child(root, 0)
        .expect("a bare composition can be edited");
    assert_eq!(document.children_of(root).unwrap().len(), 0);
}

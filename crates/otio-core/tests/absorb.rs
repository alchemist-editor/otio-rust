//! Moving objects from one document into another.
//!
//! Two objects built separately live in separate arenas, so putting one inside
//! the other means moving it. That is what `Document::absorb` is for, and what
//! the Python bindings need every time a caller appends one object to another.

use std::path::PathBuf;

use otio_core::schema::{Base, Marker, Node};
use otio_core::{Any, Document};

/// Returns every sample document's contents.
fn samples() -> Vec<(String, String)> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&directory)
        .expect("tests/data exists")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "otio").then_some(path)
        })
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "no sample documents found");

    entries
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            (name, std::fs::read_to_string(&path).expect("readable"))
        })
        .collect()
}

#[test]
fn every_sample_survives_the_move() {
    // The samples between them exercise every kind of link a node can hold:
    // parents, children, effects, markers, media references and a timeline's
    // tracks. Writing the moved graph out again is the strongest check
    // available that none of them were left pointing at the old arena.
    for (name, text) in samples() {
        let document = otio_core::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let root = document.root().expect("a parsed document has a root");
        let before = otio_core::to_string(&document).expect("writes");

        let mut destination = Document::new();
        let translation = destination.absorb(document);
        let moved_root = translation[&root];
        destination.set_root(Some(moved_root));

        let after = otio_core::to_string(&destination).expect("writes");
        assert_eq!(before, after, "{name} changed when it moved");
    }
}

#[test]
fn an_object_held_in_metadata_moves_with_everything_else() {
    // Metadata is the link that is easiest to forget: it is not a field of
    // the schema but an arbitrary value that may hold a whole object.
    let mut source = Document::new();
    let marker = source.insert(Node::Marker(Marker {
        base: Base {
            name: "held".to_string(),
            ..Base::default()
        },
        ..Marker::default()
    }));
    let mut base = Base::default();
    base.metadata
        .insert("inside".to_string(), Any::Object(marker));
    let holder = source.insert(Node::SerializableObjectWithMetadata(base));

    let mut destination = Document::new();
    let translation = destination.absorb(source);

    let Some(Any::Object(moved_marker)) = destination
        .try_get(translation[&holder])
        .unwrap()
        .base()
        .unwrap()
        .metadata
        .get("inside")
        .cloned()
    else {
        panic!("the metadata still holds an object");
    };
    assert_eq!(moved_marker, translation[&marker]);
    assert_eq!(destination.try_get(moved_marker).unwrap().name(), "held");
}

#[test]
fn the_destinations_own_objects_are_left_alone() {
    // Handles from the two documents can collide, since each numbers its own
    // slots from zero. The one already here must not be rewritten.
    let mut destination = Document::new();
    let resident = destination.insert(Node::Marker(Marker {
        base: Base {
            name: "resident".to_string(),
            ..Base::default()
        },
        ..Marker::default()
    }));

    let mut source = Document::new();
    let visitor = source.insert(Node::Marker(Marker {
        base: Base {
            name: "visitor".to_string(),
            ..Base::default()
        },
        ..Marker::default()
    }));
    assert_eq!(resident, visitor, "the two documents number slots alike");

    let translation = destination.absorb(source);
    assert_eq!(destination.try_get(resident).unwrap().name(), "resident");
    assert_eq!(
        destination.try_get(translation[&visitor]).unwrap().name(),
        "visitor"
    );
}

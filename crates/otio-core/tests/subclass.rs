//! Schemas registered at run time as subclasses of a built-in one.
//!
//! The registry is process-wide, as upstream's is, and tests run in parallel,
//! so every schema a test registers has a name no other test uses.

use std::sync::Arc;

use otio_core::registry::{self, SchemaKind, SchemaVersionMap};
use otio_core::schema::{Extension, Node};
use otio_core::{Any, AnyDictionary, Document, Error, NodeId, WriteOptions};

fn compact() -> WriteOptions {
    WriteOptions {
        indent: None,
        ..WriteOptions::default()
    }
}

fn written(document: &Document, id: NodeId, options: &WriteOptions) -> String {
    otio_core::to_string_with(document, &Any::Object(id), options).expect("writes")
}

fn root(document: &Document) -> NodeId {
    document.root().expect("a root")
}

/// A track holding one clip of the subclass `schema`, carrying a field of
/// its own and a marker held in another.
fn track_with_subclassed_clip(schema: &str) -> String {
    format!(
        r#"{{
    "OTIO_SCHEMA": "Track.1",
    "name": "V1",
    "kind": "Video",
    "children": [
        {{
            "OTIO_SCHEMA": "{schema}",
            "name": "shot",
            "take": 3,
            "held": {{
                "OTIO_SCHEMA": "Marker.2",
                "name": "kept",
                "marked_range": {{
                    "OTIO_SCHEMA": "TimeRange.1",
                    "start_time": {{"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24}},
                    "duration": {{"OTIO_SCHEMA": "RationalTime.1", "value": 1, "rate": 24}}
                }}
            }},
            "source_range": {{
                "OTIO_SCHEMA": "TimeRange.1",
                "start_time": {{"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24}},
                "duration": {{"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}
            }}
        }},
        {{
            "OTIO_SCHEMA": "Gap.1",
            "source_range": {{
                "OTIO_SCHEMA": "TimeRange.1",
                "start_time": {{"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24}},
                "duration": {{"OTIO_SCHEMA": "RationalTime.1", "value": 24, "rate": 24}}
            }}
        }}
    ]
}}"#
    )
}

#[test]
fn a_subclass_of_clip_reads_as_a_clip_under_its_own_name() {
    assert!(registry::register_subclass("SubclassTestClip", 1, "Clip"));
    assert_eq!(
        registry::schema_kind("SubclassTestClip"),
        Some(SchemaKind::Subclass("Clip"))
    );

    let document =
        otio_core::from_str(&track_with_subclassed_clip("SubclassTestClip.1")).expect("reads");
    let track = root(&document);
    let clip = document.children_of(track).expect("a track")[0];
    let node = document.try_get(clip).expect("live");

    let Node::Clip(inner) = node else {
        panic!("a subclass of Clip is a Clip, not a {node:?}");
    };
    assert_eq!(inner.item.base.name, "shot");
    assert_eq!(node.schema_name(), "SubclassTestClip");
    assert_eq!(node.schema_version(), 1);
    assert_eq!(node.built_in_schema_name(), "Clip");
    assert_eq!(node.built_in_schema_version(), 2);

    // Only the fields `Clip` does not read are the subclass's own.
    let extension = inner.item.base.extension.as_deref().expect("an extension");
    assert_eq!(extension.schema, Some(("SubclassTestClip".to_string(), 1)));
    assert_eq!(
        extension.fields.keys().collect::<Vec<_>>(),
        ["held", "take"]
    );
    assert_eq!(extension.fields["take"], Any::Int(3));
    let Any::Object(held) = extension.fields["held"] else {
        panic!("an object field holds the object");
    };
    assert_eq!(document.try_get(held).expect("live").name(), "kept");

    // It behaves as a clip everywhere: timing, parentage, searches.
    assert_eq!(document.parent_of(clip), Ok(track));
    assert_eq!(document.duration(track).expect("a duration").value(), 72.0);
    assert_eq!(document.find_clips(track).expect("clips"), [clip]);
}

#[test]
fn a_subclass_round_trips_its_fields_first() {
    registry::register_subclass("SubclassTestRoundTrip", 3, "Clip");
    let document =
        otio_core::from_str(&track_with_subclassed_clip("SubclassTestRoundTrip.3")).expect("reads");
    let text = written(&document, root(&document), &WriteOptions::default());

    // Reading what was written and writing it again changes nothing.
    let again = otio_core::from_str(&text).expect("reads back");
    assert_eq!(
        written(&again, root(&again), &WriteOptions::default()),
        text
    );

    // Upstream writes an object's dynamic fields straight after its schema,
    // before anything its class adds.
    let clip = document.children_of(root(&document)).expect("a track")[0];
    let clip_text = written(&document, clip, &compact());
    assert!(
        clip_text.starts_with(
            r#"{"OTIO_SCHEMA":"SubclassTestRoundTrip.3","held":{"OTIO_SCHEMA":"Marker.3","#
        ),
        "{clip_text}"
    );
    assert!(
        clip_text.contains(r#""take":3,"metadata":{},"name":"shot","source_range":"#),
        "{clip_text}"
    );
}

#[test]
fn a_subclass_nobody_registered_reads_as_an_unknown_schema() {
    // A document built by a program that knows the subclass.
    let mut document = Document::new();
    let clip = document.insert(Node::Clip(otio_core::schema::Clip::default()));
    let base = document
        .try_get_mut(clip)
        .expect("live")
        .base_mut()
        .expect("a clip has a base");
    base.extension = Some(Box::new(Extension {
        schema: Some(("SubclassTestNeverRegistered".to_string(), 2)),
        fields: AnyDictionary::from([("take".to_string(), Any::Int(1))]),
    }));
    let text = written(&document, clip, &compact());
    assert!(
        text.starts_with(r#"{"OTIO_SCHEMA":"SubclassTestNeverRegistered.2","take":1,"#),
        "{text}"
    );

    // Read by one that does not, every field is kept verbatim.
    let read = otio_core::from_str(&text).expect("reads");
    let Ok(Node::Unknown(unknown)) = read.try_get(root(&read)) else {
        panic!("an unregistered subclass is an unknown schema");
    };
    assert_eq!(unknown.original_schema_name, "SubclassTestNeverRegistered");
    assert_eq!(unknown.original_schema_version, 2);
    assert_eq!(unknown.data["take"], Any::Int(1));
    assert!(unknown.data.contains_key("media_references"));
    assert_eq!(unknown.data.len(), 10);
}

#[test]
fn a_subclass_is_held_wherever_its_built_in_may_be() {
    registry::register_subclass("SubclassTestMarker", 1, "Marker");
    registry::register_subclass("SubclassTestStack", 1, "Stack");
    registry::register_subclass("SubclassTestNotAMarker", 1, "Clip");

    let text = r#"{
        "OTIO_SCHEMA": "Timeline.1",
        "tracks": {
            "OTIO_SCHEMA": "SubclassTestStack.1",
            "children": [{
                "OTIO_SCHEMA": "Track.1",
                "markers": [{"OTIO_SCHEMA": "SubclassTestMarker.1", "note": "here"}]
            }]
        }
    }"#;
    let document = otio_core::from_str(text).expect("reads");
    let Ok(Node::Timeline(timeline)) = document.try_get(root(&document)) else {
        panic!("a timeline");
    };
    let stack = timeline.tracks.expect("tracks");
    assert!(matches!(document.try_get(stack), Ok(Node::Stack(_))));
    let track = document.children_of(stack).expect("a stack")[0];
    let marker = document.try_get(track).unwrap().item().unwrap().markers[0];
    let marker = document.try_get(marker).expect("live");
    assert!(matches!(marker, Node::Marker(_)));
    assert_eq!(marker.schema_name(), "SubclassTestMarker");

    // A clip is no more a marker for being a subclass of one.
    let text = r#"{
        "OTIO_SCHEMA": "Track.1",
        "markers": [{"OTIO_SCHEMA": "SubclassTestNotAMarker.1"}]
    }"#;
    let Err(Error::TypeMismatch { detail, .. }) = otio_core::from_str(text) else {
        panic!("a clip in a track's markers is refused");
    };
    assert!(detail.contains("4Clip"), "{detail}");
}

#[test]
fn a_subclass_upgrades_and_downgrades_under_its_own_name() {
    registry::register_subclass("SubclassTestVersions", 2, "ExternalReference");
    registry::register_upgrade_function(
        "SubclassTestVersions",
        2,
        Arc::new(|fields: &mut AnyDictionary| {
            let old = fields.remove("reel").unwrap_or(Any::Null);
            fields.insert("tape".to_string(), old);
            Ok(())
        }),
    );
    registry::register_downgrade_function(
        "SubclassTestVersions",
        2,
        Arc::new(|fields: &mut AnyDictionary| {
            let new = fields.remove("tape").unwrap_or(Any::Null);
            fields.insert("reel".to_string(), new);
            Ok(())
        }),
    );

    let text =
        r#"{"OTIO_SCHEMA": "SubclassTestVersions.1", "target_url": "a.mov", "reel": "A001"}"#;
    let document = otio_core::from_str(text).expect("reads");
    let node = document.try_get(root(&document)).expect("live");
    let Node::ExternalReference(reference) = node else {
        panic!("an external reference");
    };
    assert_eq!(reference.target_url, "a.mov");
    assert_eq!(node.schema_version(), 2);
    let fields = reference.media.base.extension_fields().expect("fields");
    assert_eq!(fields["tape"], Any::from("A001"));

    // `Clip`'s own ladder is not the subclass's, and a target under the
    // subclass's name takes it back down.
    let targets = WriteOptions {
        indent: None,
        schema_version_targets: SchemaVersionMap::from([
            ("SubclassTestVersions".to_string(), 1),
            ("ExternalReference".to_string(), 0),
        ]),
    };
    let text = written(&document, root(&document), &targets);
    assert!(
        text.starts_with(r#"{"OTIO_SCHEMA":"SubclassTestVersions.1","#),
        "{text}"
    );
    assert!(text.contains(r#""reel":"A001""#), "{text}");
    assert!(!text.contains("tape"), "{text}");
}

#[test]
fn copies_keep_the_subclass_and_copy_what_its_fields_hold() {
    registry::register_subclass("SubclassTestCopied", 1, "Clip");
    let mut document =
        otio_core::from_str(&track_with_subclassed_clip("SubclassTestCopied.1")).expect("reads");
    let clip = document.children_of(root(&document)).expect("a track")[0];
    let held = |document: &Document, id: NodeId| -> NodeId {
        let fields = document
            .try_get(id)
            .unwrap()
            .base()
            .unwrap()
            .extension_fields()
            .unwrap();
        match fields["held"] {
            Any::Object(held) => held,
            _ => panic!("an object"),
        }
    };

    let copy = document.deep_clone(clip).expect("copies");
    let node = document.try_get(copy).expect("live");
    assert_eq!(node.schema_name(), "SubclassTestCopied");
    assert!(matches!(node, Node::Clip(_)));
    // A deep copy owns a copy of the marker, not the original's.
    assert_ne!(held(&document, copy), held(&document, clip));
    assert_eq!(
        written(&document, copy, &compact()),
        written(&document, clip, &compact())
    );
}

/// Every built-in a subclass can derive from reads back every field its
/// writer writes, so that nothing the built-in owns is mistaken for one of
/// the subclass's own.
#[test]
fn a_subclass_keeps_no_field_its_built_in_writes() {
    let built_ins = [
        ("Clip", 2),
        ("Composable", 1),
        ("Composition", 1),
        ("Effect", 1),
        ("ExternalReference", 1),
        ("FreezeFrame", 1),
        ("Gap", 1),
        ("GeneratorReference", 1),
        ("ImageSequenceReference", 1),
        ("Item", 1),
        ("LinearTimeWarp", 1),
        ("Marker", 3),
        ("MediaReference", 1),
        ("MissingReference", 1),
        ("SerializableCollection", 1),
        ("Stack", 1),
        ("TimeEffect", 1),
        ("Timeline", 1),
        ("Track", 1),
        ("Transition", 1),
    ];
    for (built_in, version) in built_ins {
        let subclass = format!("SubclassTestOf{built_in}");
        assert!(
            registry::register_subclass(&subclass, 1, built_in),
            "{built_in} can be derived from"
        );
        let mut document =
            otio_core::from_str(&format!(r#"{{"OTIO_SCHEMA": "{built_in}.{version}"}}"#))
                .expect("reads");
        let id = root(&document);
        document
            .try_get_mut(id)
            .unwrap()
            .base_mut()
            .expect("a built-in with a name")
            .extension = Some(Box::new(Extension {
            schema: Some((subclass.clone(), 1)),
            fields: AnyDictionary::new(),
        }));

        let text = written(&document, id, &compact());
        let read = otio_core::from_str(&text).expect("reads back");
        let node = read.try_get(root(&read)).unwrap();
        assert_eq!(node.built_in_schema_name(), built_in);
        assert_eq!(node.schema_name(), subclass);
        assert_eq!(
            node.base().unwrap().extension_fields(),
            Some(&AnyDictionary::new()),
            "{built_in} wrote a field it does not read: {text}"
        );
    }

    // The root classes are derived from as dynamic objects instead.
    for root_class in ["SerializableObject", "SerializableObjectWithMetadata"] {
        assert!(!registry::register_subclass(
            &format!("SubclassTestOf{root_class}"),
            1,
            root_class
        ));
    }
}

#[test]
fn a_built_in_keeps_the_fields_it_does_not_read() {
    // Upstream keeps these in any object's dynamic fields and writes them
    // back, so a file written by a newer release or a plugin loses nothing.
    let text = r#"{"OTIO_SCHEMA":"Gap.1","later":[1,2],"metadata":{},"name":"g","source_range":null,"effects":[],"markers":[],"enabled":true,"color":null}"#;
    let document = otio_core::from_str(text).expect("reads");
    let node = document.try_get(root(&document)).expect("live");
    assert_eq!(node.schema_name(), "Gap");
    let extension = node.base().unwrap().extension.as_deref().expect("kept");
    assert_eq!(extension.schema, None);
    assert_eq!(
        extension.fields["later"],
        Any::Vector(vec![Any::Int(1), Any::Int(2)])
    );
    assert_eq!(written(&document, root(&document), &compact()), text);

    // One with nothing extra carries no extension at all.
    let document = otio_core::from_str(r#"{"OTIO_SCHEMA":"Gap.1"}"#).expect("reads");
    let node = document.try_get(root(&document)).expect("live");
    assert!(node.base().unwrap().extension.is_none());
}

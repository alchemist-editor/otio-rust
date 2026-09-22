//! Schema versioning: the registry, upgrades on read, downgrades on write,
//! and schemas defined at run time.
//!
//! The registry is process-wide, as upstream's is, and tests run in parallel,
//! so every schema a test registers has a name no other test uses.

use std::sync::Arc;

use otio_core::registry::{self, DynamicBase, SchemaVersionMap};
use otio_core::schema::{Base, DynamicObject, Marker, Node};
use otio_core::{Any, AnyDictionary, Color, Document, Error, WriteOptions};

fn targets(entries: &[(&str, u32)]) -> WriteOptions {
    WriteOptions {
        schema_version_targets: entries
            .iter()
            .map(|(name, version)| ((*name).to_string(), *version))
            .collect::<SchemaVersionMap>(),
        ..WriteOptions::default()
    }
}

fn written(document: &Document, options: &WriteOptions) -> String {
    let root = document.root().expect("a root");
    otio_core::to_string_with(document, &Any::Object(root), options).expect("writes")
}

fn parsed(text: &str) -> otio_core::json::Value {
    otio_core::json::parse(text).expect("valid JSON")
}

#[test]
fn a_run_time_schema_reads_as_a_dynamic_object_and_round_trips() {
    assert!(registry::register_type(
        "VersioningTestThing",
        1,
        DynamicBase::SerializableObject
    ));
    let text = r#"{"OTIO_SCHEMA": "VersioningTestThing.1", "b": 2, "a": {"x": 1}}"#;
    let document = otio_core::from_str(text).expect("reads");
    let root = document.root().expect("a root");
    let Node::Dynamic(thing) = document.try_get(root).expect("live") else {
        panic!("a registered schema reads as a dynamic object");
    };
    assert_eq!(thing.schema_name, "VersioningTestThing");
    assert_eq!(thing.schema_version, 1);
    assert!(thing.base.is_none());
    assert_eq!(thing.fields["b"], Any::Int(2));

    // Dynamic fields are written sorted, straight after the schema.
    assert_eq!(
        written(&document, &WriteOptions::default()),
        "{\n    \"OTIO_SCHEMA\": \"VersioningTestThing.1\",\n    \"a\": {\n        \"x\": 1\n    },\n    \"b\": 2\n}\n"
    );
}

#[test]
fn a_run_time_schema_with_metadata_keeps_its_name_apart() {
    registry::register_type(
        "VersioningTestNamed",
        1,
        DynamicBase::SerializableObjectWithMetadata,
    );
    let text = r#"{"OTIO_SCHEMA": "VersioningTestNamed.1", "name": "n", "metadata": {"k": 1}, "extra": true}"#;
    let document = otio_core::from_str(text).expect("reads");
    let node = document.try_get(document.root().unwrap()).unwrap();
    assert_eq!(node.name(), "n");
    assert_eq!(node.base().unwrap().metadata["k"], Any::Int(1));
    let Node::Dynamic(named) = node else {
        panic!("a dynamic object");
    };
    assert_eq!(named.fields.keys().collect::<Vec<_>>(), ["extra"]);
    assert_eq!(
        written(
            &document,
            &WriteOptions {
                indent: None,
                ..WriteOptions::default()
            }
        ),
        r#"{"OTIO_SCHEMA":"VersioningTestNamed.1","extra":true,"metadata":{"k":1},"name":"n"}"#
    );
}

#[test]
fn an_unregistered_schema_is_still_unknown() {
    let document =
        otio_core::from_str(r#"{"OTIO_SCHEMA": "VersioningTestNobody.7", "a": 1}"#).expect("reads");
    assert!(matches!(
        document.try_get(document.root().unwrap()),
        Ok(Node::Unknown(_))
    ));
}

#[test]
fn a_newer_version_than_registered_is_refused() {
    assert_eq!(
        otio_core::from_str(r#"{"OTIO_SCHEMA": "Clip.3"}"#).err(),
        Some(Error::UnsupportedSchemaVersion {
            schema: "Clip".to_string(),
            version: 3,
            highest: 2,
        })
    );
}

#[test]
fn upgrade_functions_run_in_order_from_the_objects_version() {
    registry::register_type("VersioningTestLadder", 4, DynamicBase::SerializableObject);
    let rename = |from: &'static str, to: &'static str| -> registry::VersionFunction {
        Arc::new(move |fields: &mut AnyDictionary| {
            let value = fields.remove(from).unwrap_or(Any::Null);
            fields.insert(to.to_string(), value);
            Ok(())
        })
    };
    assert!(registry::register_upgrade_function(
        "VersioningTestLadder",
        2,
        rename("foo", "foo_2")
    ));
    assert!(registry::register_upgrade_function(
        "VersioningTestLadder",
        3,
        rename("foo_2", "foo_3")
    ));

    for (version, field) in [(1, "foo"), (3, "foo_2"), (4, "foo_3")] {
        let text =
            format!(r#"{{"OTIO_SCHEMA": "VersioningTestLadder.{version}", "{field}": "bar"}}"#);
        let document = otio_core::from_str(&text).expect("reads");
        let Ok(Node::Dynamic(thing)) = document.try_get(document.root().unwrap()) else {
            panic!("a dynamic object");
        };
        assert_eq!(thing.schema_version, 4);
        assert_eq!(thing.fields.len(), 1, "from version {version}");
        assert_eq!(
            thing.fields["foo_3"],
            Any::from("bar"),
            "from version {version}"
        );
    }
}

#[test]
fn a_failing_upgrade_stops_the_read() {
    registry::register_type("VersioningTestFails", 2, DynamicBase::SerializableObject);
    registry::register_upgrade_function(
        "VersioningTestFails",
        2,
        Arc::new(|_| {
            Err(Error::VersionFunctionFailed {
                schema: "VersioningTestFails".to_string(),
                message: "no".to_string(),
            })
        }),
    );
    assert!(matches!(
        otio_core::from_str(r#"{"OTIO_SCHEMA": "VersioningTestFails.1"}"#),
        Err(Error::VersionFunctionFailed { .. })
    ));
}

#[test]
fn the_built_in_upgrades_run_through_the_registry() {
    let clip = r#"{
        "OTIO_SCHEMA": "Clip.1",
        "media_reference": {"OTIO_SCHEMA": "ExternalReference.1", "target_url": "a.mov"},
        "markers": [{"OTIO_SCHEMA": "Marker.1", "color": "green", "range": {
            "OTIO_SCHEMA": "TimeRange.1",
            "start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 1},
            "duration": {"OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 2}
        }}]
    }"#;
    let document = otio_core::from_str(clip).expect("reads");
    let Ok(Node::Clip(clip)) = document.try_get(document.root().unwrap()) else {
        panic!("a clip");
    };
    assert_eq!(clip.active_media_reference_key, "DEFAULT_MEDIA");
    let Ok(Node::ExternalReference(reference)) =
        document.try_get(clip.media_references["DEFAULT_MEDIA"])
    else {
        panic!("the reference moved under DEFAULT_MEDIA");
    };
    assert_eq!(reference.target_url, "a.mov");
    let Ok(Node::Marker(marker)) = document.try_get(clip.item.markers[0]) else {
        panic!("a marker");
    };
    assert_eq!(
        marker.color.as_ref().map(|c| c.name.as_str()),
        Some("Green")
    );
    assert_eq!(marker.marked_range.duration().value(), 2.0);
}

#[test]
fn writing_for_an_older_release_downgrades_each_object() {
    let mut document = Document::new();
    let marker = document.insert(Node::Marker(Marker {
        base: Base::default(),
        color: Some(Color::red()),
        marked_range: opentime::TimeRange::default(),
        comment: String::new(),
    }));
    document.set_root(Some(marker));

    let text = written(&document, &targets(&[("Marker", 2)]));
    let value = parsed(&text);
    assert_eq!(
        value.get("OTIO_SCHEMA").and_then(|v| v.as_str()),
        Some("Marker.2")
    );
    assert_eq!(value.get("color").and_then(|v| v.as_str()), Some("RED"));
    // A downgraded object is written from a dictionary, so its keys sort.
    assert!(text.starts_with(
        "{\n    \"OTIO_SCHEMA\": \"Marker.2\",\n    \"color\": \"RED\",\n    \"comment\""
    ));

    // A target at or above the current version changes nothing.
    assert_eq!(
        written(&document, &targets(&[("Marker", 3)])),
        written(&document, &WriteOptions::default())
    );
}

#[test]
fn objects_inside_a_downgraded_one_are_downgraded_too() {
    let clip = r#"{
        "OTIO_SCHEMA": "Clip.2",
        "media_references": {
            "DEFAULT_MEDIA": {"OTIO_SCHEMA": "MissingReference.1"},
            "high": {"OTIO_SCHEMA": "ExternalReference.1", "target_url": "hi.mov"}
        },
        "active_media_reference_key": "high",
        "markers": [{"OTIO_SCHEMA": "Marker.3", "color": {"OTIO_SCHEMA": "Color.1", "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0, "name": "Blue"}}]
    }"#;
    let document = otio_core::from_str(clip).expect("reads");
    let value = parsed(&written(&document, &targets(&[("Clip", 1), ("Marker", 2)])));
    assert_eq!(
        value.get("OTIO_SCHEMA").and_then(|v| v.as_str()),
        Some("Clip.1")
    );
    assert!(value.get("media_references").is_none());
    assert_eq!(
        value
            .get("media_reference")
            .and_then(|v| v.get("target_url"))
            .and_then(|v| v.as_str()),
        Some("hi.mov")
    );
    let marker = &value.get("markers").and_then(|v| v.as_array()).unwrap()[0];
    assert_eq!(marker.get("color").and_then(|v| v.as_str()), Some("BLUE"));

    // And a clip written for Clip.1 reads back as the same clip.
    let again = otio_core::from_str(&written(&document, &targets(&[("Clip", 1)]))).unwrap();
    let Ok(Node::Clip(clip)) = again.try_get(again.root().unwrap()) else {
        panic!("a clip");
    };
    assert_eq!(clip.media_references.len(), 1);
}

#[test]
fn a_nested_object_is_downgraded_even_when_its_holder_is_not() {
    let track = r#"{"OTIO_SCHEMA": "Track.1", "children": [
        {"OTIO_SCHEMA": "Gap.1", "markers": [{"OTIO_SCHEMA": "Marker.3"}]}
    ]}"#;
    let document = otio_core::from_str(track).unwrap();
    let text = written(&document, &targets(&[("Marker", 2)]));
    assert!(text.contains("\"OTIO_SCHEMA\": \"Track.1\""));
    assert!(text.contains("\"OTIO_SCHEMA\": \"Marker.2\""));
}

#[test]
fn a_run_time_downgrade_replaces_the_fields() {
    registry::register_type("VersioningTestDown", 2, DynamicBase::SerializableObject);
    registry::register_downgrade_function(
        "VersioningTestDown",
        2,
        Arc::new(|fields: &mut AnyDictionary| {
            let value = fields.remove("foo_2").unwrap_or(Any::Null);
            fields.clear();
            fields.insert("foo".to_string(), value);
            Ok(())
        }),
    );
    let mut document = Document::new();
    let mut fields = AnyDictionary::new();
    fields.insert("foo_2".into(), "a thing here".into());
    let root = document.insert(Node::Dynamic(DynamicObject {
        schema_name: "VersioningTestDown".to_string(),
        schema_version: 2,
        base: None,
        fields,
    }));
    document.set_root(Some(root));
    assert_eq!(
        written(
            &document,
            &WriteOptions {
                indent: None,
                ..targets(&[("VersioningTestDown", 1)])
            }
        ),
        r#"{"OTIO_SCHEMA":"VersioningTestDown.1","foo":"a thing here"}"#
    );

    // Asking for a version nothing can reach is an error, not a silent skip.
    assert_eq!(
        otio_core::to_string_with(
            &document,
            &Any::Object(root),
            &targets(&[("VersioningTestDown", 0)])
        )
        .err(),
        Some(Error::NoDowngradeFunction {
            schema: "VersioningTestDown".to_string(),
            from: 1,
            to: 0,
        })
    );
}

#[test]
fn an_object_holding_itself_cannot_be_written() {
    let mut document = Document::new();
    let root = document.insert(Node::SerializableObjectWithMetadata(Base::default()));
    document
        .get_mut(root)
        .and_then(Node::base_mut)
        .unwrap()
        .metadata
        .insert("myself".into(), Any::Object(root));
    document.set_root(Some(root));
    assert_eq!(
        otio_core::to_string(&document).err(),
        Some(Error::ObjectCycle {
            schema: "SerializableObjectWithMetadata".to_string()
        })
    );
}

#[test]
fn an_instance_is_built_from_a_schema_and_its_fields() {
    registry::register_type("VersioningTestInstance", 2, DynamicBase::SerializableObject);
    registry::register_upgrade_function(
        "VersioningTestInstance",
        2,
        Arc::new(|fields: &mut AnyDictionary| {
            fields.insert("upgraded".into(), Any::Bool(true));
            Ok(())
        }),
    );
    let mut data = AnyDictionary::new();
    data.insert("foo".into(), "bar".into());
    let empty = Document::new();

    let built = registry::instance_from_schema(&empty, "VersioningTestInstance", 1, &data).unwrap();
    let Ok(Node::Dynamic(thing)) = built.try_get(built.root().unwrap()) else {
        panic!("a dynamic object");
    };
    assert_eq!(thing.fields["foo"], Any::from("bar"));
    assert_eq!(thing.fields["upgraded"], Any::Bool(true));

    assert!(matches!(
        registry::instance_from_schema(&empty, "VersioningTestInstance", 3, &data),
        Err(Error::UnsupportedSchemaVersion { highest: 2, .. })
    ));
}

#[test]
fn a_base_class_with_extra_fields_keeps_them() {
    let text = r#"{"OTIO_SCHEMA": "SerializableObject.1", "foo": 1}"#;
    let document = otio_core::from_str(text).unwrap();
    assert!(matches!(
        document.try_get(document.root().unwrap()),
        Ok(Node::Dynamic(DynamicObject { base: None, .. }))
    ));
    assert_eq!(
        written(
            &document,
            &WriteOptions {
                indent: None,
                ..WriteOptions::default()
            }
        ),
        r#"{"OTIO_SCHEMA":"SerializableObject.1","foo":1}"#
    );
    // Without them it is the plain base class it always was.
    let plain = otio_core::from_str(r#"{"OTIO_SCHEMA": "SerializableObject.1"}"#).unwrap();
    assert!(matches!(
        plain.try_get(plain.root().unwrap()),
        Ok(Node::SerializableObject)
    ));
}

#[test]
fn any_value_can_be_read_at_the_root() {
    let (_, time) =
        otio_core::from_str_any(r#"{"OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 15}"#)
            .unwrap();
    assert_eq!(
        time,
        Any::RationalTime(opentime::RationalTime::new(15.0, 24.0))
    );

    let (document, list) = otio_core::from_str_any(r#"[{"OTIO_SCHEMA": "Gap.1"}, 3]"#).unwrap();
    let Any::Vector(items) = list else {
        panic!("a list");
    };
    let Any::Object(gap) = items[0] else {
        panic!("an object");
    };
    assert!(matches!(document.try_get(gap), Ok(Node::Gap(_))));
    assert_eq!(document.root(), None);
}

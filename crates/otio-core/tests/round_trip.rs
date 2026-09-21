//! Conformance against upstream's sample documents.
//!
//! The files in `tests/data` come from upstream OpenTimelineIO 0.19.0. They
//! are the practical specification for the format, so the bar this library has
//! to clear is stated against them:
//!
//! 1. **Every file parses.**
//! 2. **Serialization is idempotent.** Parsing, writing, and parsing again
//!    produces the same bytes, so a file does not drift each time a tool
//!    touches it.
//! 3. **Nothing is lost.** Everything the input says appears in the output.
//!    Fields move between schema versions and empty ones may disappear, but
//!    no content does.
//!
//! Note that byte-for-byte equality with the *input* is deliberately not the
//! bar, and upstream does not clear it either: its own baseline tests compare
//! parsed JSON, not bytes, because a newer release writes fields that an older
//! file does not carry (`color` on `Item`, for instance, added in 0.19.0
//! without a schema version bump). Requiring identical bytes would mean
//! refusing to ever add a field.

use std::collections::BTreeSet;
use std::path::PathBuf;

use otio_core::json::{self, Value};

/// Returns every sample document, as (file name, contents).
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
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            (name, contents)
        })
        .collect()
}

/// Legacy schema names that upstream renamed, and what they are read as now.
fn canonical_schema_name(name: &str) -> &str {
    match name {
        "Filler" => "Gap",
        "Sequence" => "Track",
        other => other,
    }
}

/// Compares an `OTIO_SCHEMA` value from the input against the output's.
///
/// The version may legitimately differ: reading a `Marker.2` and writing a
/// `Marker.3` is an upgrade, not a loss. The name may differ only by the
/// renames upstream itself performs.
fn schemas_agree(input: &str, output: &str) -> bool {
    let name_of = |schema: &str| {
        schema
            .rsplit_once('.')
            .map_or_else(|| schema.to_string(), |(name, _)| name.to_string())
    };
    canonical_schema_name(&name_of(input)) == canonical_schema_name(&name_of(output))
}

/// Where a schema upgrade moves a field to.
///
/// A `Clip.1` read and written back out as a `Clip.2` no longer carries a
/// `media_reference` key: that object now lives in `media_references`, under
/// the default key. The content survives, only its address changed. `Marker.1`
/// is the same story, with `range` becoming `marked_range`.
fn relocated<'a>(schema: Option<&str>, key: &str, output: &'a Value) -> Option<&'a Value> {
    match (schema?, key) {
        ("Clip.1", "media_reference") => output
            .get("media_references")
            .and_then(|references| references.get(otio_core::upgrade::DEFAULT_MEDIA_KEY)),
        ("Marker.1", "range") => output.get("marked_range"),
        _ => None,
    }
}

/// Whether a value says nothing — the shape a field takes when the file gave
/// it no content.
///
/// An explicit `MissingReference` counts: it is how the format spells "there
/// is no media here".
fn says_nothing(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(entries) => {
            entries.is_empty()
                || value
                    .get("OTIO_SCHEMA")
                    .and_then(Value::as_str)
                    .is_some_and(|schema| schema.starts_with("MissingReference."))
        }
        _ => false,
    }
}

/// Asserts that everything in `input` is present in `output`.
///
/// `output` may carry additional keys — fields added in a later schema
/// revision — but may not drop or alter anything that was in the input.
fn assert_no_data_lost(input: &Value, output: &Value, path: &str) {
    match (input, output) {
        (Value::Object(input_object), Value::Object(_)) => {
            let schema = input.get("OTIO_SCHEMA").and_then(Value::as_str);
            for (key, input_value) in input_object {
                // Not a field: an instancing id that this library resolves at
                // read time rather than carrying through.
                if key == "OTIO_REF_ID" {
                    continue;
                }

                let Some(output_value) = output.get(key).or_else(|| relocated(schema, key, output))
                else {
                    // A key the output does not have is only a loss if the
                    // input put something in it. `"parameters": {}` on a
                    // Transition is the clearest case: the field is a
                    // Python-era leftover that the current schema does not
                    // define, and it is empty.
                    assert!(
                        says_nothing(input_value),
                        "{path}.{key} was dropped: {input_value:?}"
                    );
                    continue;
                };

                if key == "OTIO_SCHEMA" {
                    let (Some(input_schema), Some(output_schema)) =
                        (input_value.as_str(), output_value.as_str())
                    else {
                        panic!("{path}.{key} is not a string on both sides");
                    };
                    assert!(
                        schemas_agree(input_schema, output_schema),
                        "{path}: schema changed from {input_schema} to {output_schema}"
                    );
                    continue;
                }

                assert_no_data_lost(input_value, output_value, &format!("{path}.{key}"));
            }
        }
        (Value::Array(input_items), Value::Array(output_items)) => {
            assert_eq!(
                input_items.len(),
                output_items.len(),
                "{path}: array length changed"
            );
            for (index, (input_item, output_item)) in
                input_items.iter().zip(output_items).enumerate()
            {
                assert_no_data_lost(input_item, output_item, &format!("{path}[{index}]"));
            }
        }
        // A marker colour was a bare name before Marker.3 and is a Color.1
        // object now. The name is what carried the meaning, so it is what has
        // to survive.
        (Value::String(name), Value::Object(_)) => {
            assert_eq!(
                output.get("name").and_then(Value::as_str),
                Some(name.as_str()),
                "{path}: colour name lost in the upgrade to Color.1"
            );
        }
        // `null` is the file declining to say anything. Writing the schema's
        // default in its place says the same thing; only an output carrying
        // real content there would be a change.
        (Value::Null, _) => assert!(
            says_nothing(output),
            "{path}: null was replaced with content: {output:?}"
        ),
        (Value::Number(input_number), Value::Number(output_number)) => {
            // 24 and 24.0 are the same value; the writer always emits a
            // double where the schema calls for one.
            assert_eq!(
                input_number.as_f64().to_bits(),
                output_number.as_f64().to_bits(),
                "{path}: number changed"
            );
        }
        _ => assert_eq!(input, output, "{path}: value changed"),
    }
}

#[test]
fn every_sample_parses() {
    for (name, contents) in samples() {
        let document = otio_core::from_str(&contents)
            .unwrap_or_else(|error| panic!("parsing {name}: {error}"));
        assert!(
            document.root().is_some(),
            "{name}: parsed document has no root"
        );
        assert!(!document.is_empty(), "{name}: parsed document is empty");
    }
}

#[test]
fn serialization_is_idempotent() {
    for (name, contents) in samples() {
        let first = otio_core::from_str(&contents)
            .unwrap_or_else(|error| panic!("parsing {name}: {error}"));
        let written =
            otio_core::to_string(&first).unwrap_or_else(|error| panic!("writing {name}: {error}"));

        let second = otio_core::from_str(&written)
            .unwrap_or_else(|error| panic!("re-parsing {name}: {error}"));
        let rewritten = otio_core::to_string(&second)
            .unwrap_or_else(|error| panic!("re-writing {name}: {error}"));

        assert_eq!(
            written, rewritten,
            "{name}: a second round trip changed the bytes"
        );
    }
}

#[test]
fn round_trip_loses_nothing() {
    for (name, contents) in samples() {
        let document = otio_core::from_str(&contents)
            .unwrap_or_else(|error| panic!("parsing {name}: {error}"));
        let written = otio_core::to_string(&document)
            .unwrap_or_else(|error| panic!("writing {name}: {error}"));

        let input = json::parse(&contents).expect("sample parses");
        let output = json::parse(&written).expect("output parses");
        assert_no_data_lost(&input, &output, &format!("{name}$"));
    }
}

#[test]
fn output_is_valid_json() {
    for (name, contents) in samples() {
        let document = otio_core::from_str(&contents)
            .unwrap_or_else(|error| panic!("parsing {name}: {error}"));
        let written = otio_core::to_string(&document)
            .unwrap_or_else(|error| panic!("writing {name}: {error}"));
        json::parse(&written)
            .unwrap_or_else(|error| panic!("{name} produced invalid JSON: {error}"));
    }
}

#[test]
fn samples_cover_the_schemas_we_claim_to_support() {
    // A guard against the suite quietly ceasing to exercise a schema.
    let mut seen = BTreeSet::new();
    for (_, contents) in samples() {
        let value: Value = json::parse(&contents).expect("sample parses");
        collect_schemas(&value, &mut seen);
    }

    for expected in [
        "Clip",
        "Gap",
        "Stack",
        "Timeline",
        "Track",
        "Transition",
        "Marker",
        "ExternalReference",
        "MissingReference",
        "GeneratorReference",
        "LinearTimeWarp",
        "RationalTime",
        "TimeRange",
    ] {
        assert!(
            seen.contains(expected),
            "no sample exercises {expected}; coverage has regressed"
        );
    }
}

fn collect_schemas(value: &Value, seen: &mut BTreeSet<String>) {
    match value {
        Value::Object(entries) => {
            if let Some(schema) = value.get("OTIO_SCHEMA").and_then(Value::as_str) {
                let name = schema.rsplit_once('.').map_or(schema, |(name, _)| name);
                seen.insert(canonical_schema_name(name).to_string());
            }
            for (_, entry) in entries {
                collect_schemas(entry, seen);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_schemas(item, seen);
            }
        }
        _ => {}
    }
}

#[test]
fn unknown_schemas_survive_a_round_trip() {
    // A third-party plugin's schema. Nothing here is known to this library,
    // and all of it has to come back out.
    let json = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "with a plugin object",
    "metadata": {
        "vendor_thing": {
            "OTIO_SCHEMA": "VendorThing.7",
            "widget_count": 3,
            "nested": { "deep": [1, 2, 3] }
        }
    },
    "tracks": { "OTIO_SCHEMA": "Stack.1", "children": [] }
}"#;

    let document = otio_core::from_str(json).expect("parses");
    let written = otio_core::to_string(&document).expect("writes");

    let input = json::parse(json).expect("valid JSON");
    let output = json::parse(&written).expect("valid JSON");
    assert_no_data_lost(&input, &output, "$");

    assert!(
        written.contains("\"VendorThing.7\""),
        "the plugin's schema tag and version must be preserved exactly"
    );
}

#[test]
fn missing_fields_take_upstream_defaults() {
    // Files written by older releases lack fields that later ones added.
    let json = r#"{"OTIO_SCHEMA": "Clip.1", "name": "sparse"}"#;
    let document = otio_core::from_str(json).expect("parses");
    let root = document.root().expect("has a root");

    let otio_core::Node::Clip(clip) = document.try_get(root).expect("live") else {
        panic!("expected a clip");
    };
    assert_eq!(clip.item.base.name, "sparse");
    assert!(clip.item.enabled, "enabled defaults to true");
    assert_eq!(clip.item.source_range, None);
    assert!(clip.item.markers.is_empty());
}

#[test]
fn parents_are_rebuilt_from_the_nesting() {
    // The parent link is not serialized, so reading has to restore it.
    let json = r#"{
    "OTIO_SCHEMA": "Stack.1",
    "children": [{ "OTIO_SCHEMA": "Track.1", "children": [] }]
}"#;
    let document = otio_core::from_str(json).expect("parses");
    let root = document.root().expect("has a root");
    let children = document
        .try_get(root)
        .expect("live")
        .children()
        .expect("a stack has children");
    assert_eq!(children.len(), 1);

    let child = children[0];
    assert_eq!(
        document.try_get(child).expect("live").parent(),
        Some(root),
        "a child must point back at its stack"
    );
}

#[test]
fn a_missing_schema_tag_is_an_error() {
    let error = otio_core::from_str(r#"{"name": "no schema"}"#).unwrap_err();
    assert!(matches!(error, otio_core::Error::MissingSchema { .. }));
}

#[test]
fn a_sample_path_appears_in_error_messages() {
    // Errors have to say where the problem is, or they are useless on a
    // 350KB timeline.
    let error = otio_core::from_str(r#"{"OTIO_SCHEMA": "Timeline.1", "name": 5}"#).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("$.name"), "unhelpful message: {message}");
}

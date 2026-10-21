//! With the extension off, ORI's files round-trip exactly as upstream
//! OpenTimelineIO writes them without ORI's plugin, and the API refuses to
//! work.
//!
//! This is its own test binary because enabling an extension cannot be
//! undone within a process.

use otio_core::Node;
use otio_ext_ori_annotations::{Error, annotations};

const FIXTURES: [&str; 3] = [
    "testchart_annotations.otio",
    "annotationreview.otio",
    "ori_built.otio",
];

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture is vendored")
}

/// `annotationreview.otio` was written by an OpenTimelineIO older than 0.18,
/// before items had `color`; upstream 0.18 adds `"color": null` to each item
/// on the way through, and so does otio-core. Every other byte is the same.
fn as_upstream_writes(name: &str, text: &str) -> String {
    if name != "annotationreview.otio" {
        return text.trim_end().to_string();
    }
    let mut out = String::with_capacity(text.len());
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let indent = &line[..line.len() - line.trim_start().len()];
        if line.trim_start() == "\"enabled\": true" || line.trim_start() == "\"enabled\": true," {
            // `color` follows `enabled` in an item.
            let last = !line.ends_with(',');
            out.push_str(indent);
            out.push_str("\"enabled\": true,\n");
            out.push_str(indent);
            out.push_str(if last {
                "\"color\": null"
            } else {
                "\"color\": null,"
            });
        } else {
            out.push_str(line);
        }
        if index + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

#[test]
fn ori_files_round_trip_unchanged() {
    otio_ext_ori_annotations::install();
    for name in FIXTURES {
        let text = fixture(name);
        let document = otio_core::from_str(&text).expect("reads");
        let written = otio_core::to_string_pretty(&document, 4).expect("writes");
        assert!(
            written.trim_end() == as_upstream_writes(name, &text),
            "{name} changed on the way through"
        );
    }
}

#[test]
fn ori_objects_are_unknown_schemas() {
    otio_ext_ori_annotations::install();
    let document = otio_core::from_str(&fixture("ori_built.otio")).expect("reads");
    let unknown: Vec<&str> = document
        .iter()
        .filter_map(|(_, node)| match node {
            Node::Unknown(unknown) => Some(unknown.original_schema_name.as_str()),
            _ => None,
        })
        .collect();
    assert!(unknown.contains(&"PaintStart"));
    assert!(unknown.contains(&"PaintVertices"));
    assert!(
        !document
            .iter()
            .any(|(_, node)| matches!(node, Node::Dynamic(_)))
    );
}

#[test]
fn the_api_needs_the_extension_on() {
    otio_ext_ori_annotations::install();
    let document = otio_core::from_str(&fixture("ori_built.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    assert_eq!(
        annotations::list(&document, timeline),
        Err(Error::NotEnabled)
    );
    assert!(!otio_ext_ori_annotations::is_enabled());
}

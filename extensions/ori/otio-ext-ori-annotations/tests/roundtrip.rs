//! With the extension on, ORI's objects read as typed ORI schemas, and ORI's
//! files still come out byte for byte as they went in.

mod common;

use common::{enable, fixture, write};
use otio_core::Node;
use otio_ext_ori_annotations::annotations::{self, Form, Target};
use otio_ext_ori_annotations::command::Command;
use otio_ext_ori_annotations::schema;

#[test]
fn the_test_chart_round_trips_byte_for_byte() {
    enable();
    let text = fixture("testchart_annotations.otio");
    let document = otio_core::from_str(&text).expect("reads");
    assert_eq!(write(&document), text.trim_end());
}

#[test]
fn a_file_ori_built_round_trips_byte_for_byte() {
    enable();
    let text = fixture("ori_built.otio");
    let document = otio_core::from_str(&text).expect("reads");
    assert_eq!(write(&document), text.trim_end());
}

#[test]
fn ori_objects_read_as_their_schemas() {
    enable();
    let document = otio_core::from_str(&fixture("testchart_annotations.otio")).expect("reads");
    assert!(
        !document
            .iter()
            .any(|(_, node)| matches!(node, Node::Unknown(_)))
    );
    for name in [
        schema::PAINT_START,
        schema::PAINT_POINT,
        schema::PAINT_END,
        schema::PAINT_VERTICES,
        schema::TEXT_ANNOTATION,
        schema::ELLIPSE_ANNOTATION,
        schema::RECTANGLE_ANNOTATION,
        schema::ARROW_ANNOTATION,
    ] {
        assert!(
            document.iter().any(
                |(_, node)| matches!(node, Node::Dynamic(object) if object.schema_name == name)
            ),
            "no {name}"
        );
    }
}

#[test]
fn the_extension_claims_ori_schemas() {
    enable();
    assert_eq!(otio_ext::owner_of("PaintStart"), Some("ori-annotations"));
    assert_eq!(
        otio_ext::owner_of("AnnotationEffect"),
        Some("ori-annotations")
    );
    assert_eq!(
        otio_core::registry::built_in_schema("AnnotationEffect"),
        Some("Effect")
    );
}

/// Every command in ORI's test chart reads as a typed command, and inserting
/// that copy again writes exactly the same JSON as the original.
#[test]
fn every_command_reads_and_writes_back_the_same() {
    enable();
    let mut document = otio_core::from_str(&fixture("testchart_annotations.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    let found = annotations::list(&document, timeline).expect("lists");
    let mut kinds = std::collections::BTreeSet::new();
    let mut checked = 0;
    for annotation in found {
        for id in annotation.commands {
            let command = Command::read(&document, id).expect("reads");
            assert!(!matches!(command, Command::Other(_)), "{command:?}");
            kinds.insert(command.schema_name().to_string());
            let copy = command.insert(&mut document).expect("inserts");
            assert_eq!(
                otio_core::to_string_pretty_from(&document, copy, 4).expect("writes"),
                otio_core::to_string_pretty_from(&document, id, 4).expect("writes"),
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 144 * 3 + 14 + 4 * 3);
    assert_eq!(kinds.len(), 7, "{kinds:?}");
}

#[test]
fn ori_review_exports_are_found_by_clip_name() {
    enable();
    let document = otio_core::from_str(&fixture("annotationreview.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    let found = annotations::list(&document, timeline).expect("lists");
    // Every annotated frame on the "Review Video" track.
    assert_eq!(
        found.len(),
        document
            .iter()
            .filter(|(_, node)| matches!(node, Node::Clip(clip)
                if clip.item.base.metadata.contains_key("annotation_commands")))
            .count()
    );
    assert!(
        found
            .iter()
            .all(|annotation| annotation.form == Form::Track)
    );
    assert_eq!(
        found[0].target,
        Target::ClipName("chimera_cars_srgb".to_string())
    );
    let commands: usize = found
        .iter()
        .map(|annotation| annotation.commands.len())
        .sum();
    assert_eq!(commands, 17 * 2 + 1);
    // The review track is not named "Annotations", so it is not the track
    // new annotations go to.
    assert_eq!(annotations::find_track(&document, timeline), Ok(None));
}

#[test]
fn annotation_effects_are_read() {
    enable();
    let text = r#"{"OTIO_SCHEMA":"Timeline.1","tracks":{"OTIO_SCHEMA":"Stack.1","children":[
        {"OTIO_SCHEMA":"Track.1","name":"V1","children":[
          {"OTIO_SCHEMA":"Clip.2","name":"shot","metadata":{"sync":{"guid":"shot"}},"effects":[
            {"OTIO_SCHEMA":"AnnotationEffect.1","commands":[
              {"OTIO_SCHEMA":"TextAnnotation.1","uuid":"t","text":"hi"}],
             "visible":true,"effect_name":"Annotation.1","enabled":true}]}]}]}}"#;
    let document = otio_core::from_str(text).expect("reads");
    let timeline = document.root().expect("a timeline");
    let found = annotations::for_clip(&document, timeline, "shot").expect("lists");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].form, Form::Effect);
    assert_eq!(found[0].frame, None);
    let Command::Text(text) = Command::read(&document, found[0].commands[0]).expect("reads") else {
        panic!("not text");
    };
    assert_eq!(text.text.as_deref(), Some("hi"));
    // The effect is still an effect.
    assert!(matches!(
        document.get(found[0].holder),
        Some(Node::Effect(_))
    ));
    assert_eq!(
        annotations::at_frame(&document, timeline, "shot", 99)
            .expect("lists")
            .len(),
        1
    );
}

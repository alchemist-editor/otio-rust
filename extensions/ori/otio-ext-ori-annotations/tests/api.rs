//! The annotation API writes what ORI's own Python writes, and edits by uuid
//! without disturbing anyone else's commands.

mod common;

use common::{enable, fixture, write};
use opentime::RationalTime;
use otio_core::{Document, NodeId};
use otio_ext_ori_annotations::annotations::{self, Target};
use otio_ext_ori_annotations::command::{
    self, Arrow, Command, PaintStart, Shape, TextAnnotation, Vertices,
};
use otio_ext_ori_annotations::coords::ImageSize;
use otio_ext_sync::guid;

const TS: &str = "2026-10-21T09:30:00";
const HD: ImageSize = ImageSize::new(1920.0, 1080.0);

/// `ori_built.otio` with its annotation track taken out: the media ORI
/// started from.
fn base() -> (Document, NodeId) {
    let mut document = otio_core::from_str(&fixture("ori_built.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    let track = annotations::find_track(&document, timeline)
        .expect("looks")
        .expect("has one");
    let stack = document.parent_of(track).expect("in the stack");
    document.detach_child(stack, track).expect("detaches");
    document.remove_recursive(track).expect("removes");
    (document, timeline)
}

fn stroke(uuid: &str, pixels: &[(f64, f64)], rgba: [f64; 4], size: f64) -> [Command; 3] {
    let points: Vec<(f64, f64)> = pixels
        .iter()
        .map(|(x, y)| HD.pixels_to_ori(*x, *y))
        .collect();
    command::stroke(
        PaintStart {
            rgba: Some(rgba),
            friendly_name: Some("Jeff".to_string()),
            participant_hash: Some("p-jeff".to_string()),
            timestamp: Some(TS.to_string()),
            ..PaintStart::new(uuid)
        },
        Vertices::with_size(&points, size),
    )
}

fn text(uuid: &str, px: f64, py: f64, body: &str) -> Command {
    let (x, y) = HD.pixels_to_ori(px, py);
    Command::Text(TextAnnotation {
        text: Some(body.to_string()),
        position: Some([x, y]),
        rgba: Some([1.0, 1.0, 1.0, 1.0]),
        font: Some("monospace".to_string()),
        font_size: Some(0.04),
        scale: Some(1.0),
        rotation: Some(0.0),
        spacing: Some(0.8),
        friendly_name: Some("Jeff".to_string()),
        timestamp: Some(TS.to_string()),
        ..TextAnnotation::new(uuid)
    })
}

/// What `make_ori_built.py` does through ORI's `SyncManager`, done through
/// this crate, comes out byte for byte the same.
#[test]
fn builds_what_ori_builds() {
    enable();
    let (mut document, timeline) = base();

    let track = annotations::ensure_track(&mut document, timeline).expect("adds the track");
    let at_10 =
        annotations::ensure_clip(&mut document, track, "shot", RationalTime::new(10.0, 24.0))
            .expect("adds a clip");
    annotations::add_commands(
        &mut document,
        at_10,
        &stroke(
            "stroke-1",
            &[(200.0, 200.0), (960.0, 540.0), (1720.0, 880.0)],
            [1.0, 0.25, 0.25, 1.0],
            0.01,
        ),
    )
    .expect("adds a stroke");
    annotations::add_commands(
        &mut document,
        at_10,
        &[text("text-1", 960.0, 620.0, "Eyeline is late")],
    )
    .expect("adds text");
    let at_4 = annotations::ensure_clip(&mut document, track, "shot", RationalTime::new(4.0, 24.0))
        .expect("adds a clip");
    annotations::add_commands(
        &mut document,
        at_4,
        &stroke(
            "stroke-2",
            &[(100.0, 100.0), (300.0, 300.0)],
            [0.2, 0.8, 0.3, 1.0],
            0.005,
        ),
    )
    .expect("adds a stroke");

    // Asking again finds the same clip rather than adding one.
    assert_eq!(
        annotations::ensure_clip(&mut document, track, "shot", RationalTime::new(10.0, 24.0)),
        Ok(at_10)
    );

    // The new objects have GUIDs of their own; give them the fixture's.
    let children = document.children_of(track).expect("has children");
    assert_eq!(children.len(), 3, "a gap and two clips");
    for (id, name) in [
        (track, "annotations"),
        (children[0], "gap-0"),
        (at_10, "annotation-10"),
        (at_4, "annotation-4"),
    ] {
        assert!(guid::guid(&document, id).is_some(), "{name} has no GUID");
        guid::set_guid(&mut document, id, name).expect("sets");
    }

    assert_eq!(write(&document), fixture("ori_built.otio").trim_end());
}

fn built() -> (Document, NodeId, NodeId) {
    let document = otio_core::from_str(&fixture("ori_built.otio")).expect("reads");
    let timeline = document.root().expect("a timeline");
    let track = annotations::find_track(&document, timeline)
        .expect("looks")
        .expect("found");
    let clip = annotations::find_clip(&document, track, "shot", 10)
        .expect("looks")
        .expect("found");
    (document, timeline, clip)
}

fn uuids(document: &Document, clip: NodeId) -> Vec<String> {
    annotations::commands(document, clip)
        .expect("reads")
        .into_iter()
        .map(|id| command::uuid_of(document, id).unwrap_or("").to_string())
        .collect()
}

#[test]
fn finds_annotations_by_clip_frame_and_person() {
    enable();
    let (document, timeline, clip) = built();
    let all = annotations::list(&document, timeline).expect("lists");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].holder, clip);
    assert_eq!(all[0].target, Target::ClipGuid("shot".to_string()));
    assert_eq!(all[0].frame, Some(RationalTime::new(10.0, 24.0)));

    assert_eq!(
        annotations::for_clip(&document, timeline, "shot")
            .expect("lists")
            .len(),
        2
    );
    assert_eq!(
        annotations::for_clip(&document, timeline, "other")
            .expect("lists")
            .len(),
        0
    );
    let at_4 = annotations::at_frame(&document, timeline, "shot", 4).expect("lists");
    assert_eq!(at_4.len(), 1);
    assert_eq!(at_4[0].commands.len(), 3);

    // Jeff drew both strokes; the text has no participant.
    let jeff = annotations::by_participant(&document, timeline, "p-jeff").expect("lists");
    assert_eq!(jeff.len(), 2);
    assert_eq!(jeff[0].commands.len(), 3, "the stroke, not the text");
    assert!(
        annotations::by_participant(&document, timeline, "p-sam")
            .expect("lists")
            .is_empty()
    );
}

#[test]
fn replaces_and_removes_by_uuid_leaving_the_rest() {
    enable();
    let (mut document, _, clip) = built();
    assert_eq!(
        uuids(&document, clip),
        ["stroke-1", "stroke-1", "stroke-1", "text-1"]
    );
    let before = document.len();

    // Replace the stroke with an arrow; it goes where the stroke was.
    let arrow = Command::Arrow(Arrow {
        end: [0.5, 0.25],
        ..Arrow::new("arrow-1")
    });
    assert_eq!(
        annotations::replace_commands(&mut document, clip, "stroke-1", &[arrow]),
        Ok(true)
    );
    assert_eq!(uuids(&document, clip), ["arrow-1", "text-1"]);
    // Three commands and the stroke's points went, one arrow came.
    assert_eq!(document.len(), before - 4 + 1);
    assert_eq!(
        annotations::replace_commands(&mut document, clip, "nobody", &[]),
        Ok(false)
    );

    // The text is untouched.
    let Command::Text(text) = Command::read(
        &document,
        annotations::commands(&document, clip).expect("reads")[1],
    )
    .expect("reads") else {
        panic!("not text");
    };
    assert_eq!(text.text.as_deref(), Some("Eyeline is late"));

    assert_eq!(
        annotations::remove_commands(&mut document, clip, &["arrow-1", "x"]),
        Ok(1)
    );
    assert_eq!(uuids(&document, clip), ["text-1"]);

    // An empty replacement clears the clip, as ORI's does.
    annotations::set_commands(&mut document, clip, &[]).expect("clears");
    assert!(
        annotations::commands(&document, clip)
            .expect("reads")
            .is_empty()
    );
    assert_eq!(document.len(), before - 5);

    let ellipse = Command::Ellipse(Shape {
        max: [0.1, 0.1],
        ..Shape::new("e")
    });
    annotations::set_commands(&mut document, clip, std::slice::from_ref(&ellipse)).expect("sets");
    let id = annotations::commands(&document, clip).expect("reads")[0];
    assert_eq!(Command::read(&document, id), Ok(ellipse));
}

#[test]
fn new_commands_are_written_with_ori_defaults() {
    enable();
    let mut document = Document::new();
    // What ORI's Python classes write when built with no arguments, from
    // upstream OpenTimelineIO 0.18.1 with ORI's plugin, timestamps aside.
    let cases: [(Command, &str); 6] = [
        (
            Command::PaintStart(PaintStart::new("")),
            r#"{"OTIO_SCHEMA":"PaintStart.1","brush":"circle","friendly_name":null,"ghost":null,"ghost_after":null,"ghost_before":null,"hold":null,"layer_range":null,"participant_hash":null,"rgba":null,"source_index":0,"timestamp":null,"type":"color","uuid":null,"visible":true}"#,
        ),
        (
            Command::PaintPoint(command::PaintPoint {
                points: None,
                ..command::PaintPoint::new("", Vertices::default())
            }),
            r#"{"OTIO_SCHEMA":"PaintPoint.1","layer_range":null,"points":null,"source_index":0,"timestamp":null,"uuid":null}"#,
        ),
        (
            Command::PaintEnd(command::PaintEnd::new("")),
            r#"{"OTIO_SCHEMA":"PaintEnd.1","points":null,"timestamp":null,"uuid":null}"#,
        ),
        (
            Command::Text(TextAnnotation::new("")),
            r#"{"OTIO_SCHEMA":"TextAnnotation.1","font":null,"font_size":null,"friendly_name":null,"position":null,"rgba":null,"rotation":null,"scale":null,"spacing":null,"text":null,"timestamp":null,"uuid":null}"#,
        ),
        (
            Command::Rectangle(Shape::new("")),
            r#"{"OTIO_SCHEMA":"RectangleAnnotation.1","inner_rgba":[0.0,0.0,0.0,0.0],"max":[0.0,0.0],"min":[0.0,0.0],"rgba":[1.0,1.0,1.0,1.0],"size":1.0,"timestamp":null,"uuid":null,"visible":true}"#,
        ),
        (
            Command::Arrow(Arrow::new("")),
            r#"{"OTIO_SCHEMA":"ArrowAnnotation.1","end":[0.0,0.0],"rgba":[1.0,1.0,1.0,1.0],"size":1.0,"start":[0.0,0.0],"timestamp":null,"uuid":null,"visible":true}"#,
        ),
    ];
    for (command, expected) in cases {
        let id = command.insert(&mut document).expect("inserts");
        document.set_root(Some(id));
        let written = otio_core::to_string(&document).expect("writes");
        assert_eq!(written.split_whitespace().collect::<String>(), expected);
    }
    let vertices = command::PaintPoint::new("", Vertices::default());
    let id = Command::PaintPoint(vertices)
        .insert(&mut document)
        .expect("inserts");
    let points: String = otio_core::to_string_pretty_from(&document, id, 4)
        .expect("writes")
        .split_whitespace()
        .collect();
    assert!(
        points.contains(r#""OTIO_SCHEMA":"PaintVertices.1","alpha":[],"size":[],"x":[],"y":[]"#),
        "{points}"
    );
}

#[test]
fn refuses_what_is_not_an_annotation_clip_or_track() {
    enable();
    let (mut document, timeline, _) = built();
    assert!(matches!(
        annotations::add_commands(&mut document, timeline, &[]),
        Err(otio_ext_ori_annotations::Error::WrongKind { .. })
    ));
    assert!(matches!(
        annotations::find_clip(&document, timeline, "shot", 1),
        Err(otio_ext_ori_annotations::Error::WrongKind { .. })
    ));
}

#[test]
fn timestamps_are_iso_8601() {
    let now = command::timestamp_now();
    assert_eq!(now.len(), "2026-10-21T09:30:00.000000Z".len(), "{now}");
    assert!(now.starts_with("20"), "{now}");
    assert_eq!(&now[10..11], "T");
}

//! EXPERIMENTAL multicam prototype (ADR 0014): the `Multicam` and
//! `MulticamClip` subclasses and `Timeline.multicams`, checked against files
//! the C++/Python prototype wrote.

use opentime::{RationalTime, TimeRange};
use otio_core::multicam::{self, Cut};
use otio_core::{Document, Error, Node, NodeId};

fn data(name: &str) -> String {
    let path = format!("{}/tests/data/multicam/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).expect("fixture")
}

fn read(name: &str) -> (Document, NodeId) {
    let document = otio_core::from_str(&data(name)).expect("reads");
    let root = document.root().expect("root");
    (document, root)
}

fn tracks(document: &Document, timeline: NodeId) -> Vec<NodeId> {
    let Node::Timeline(node) = document.try_get(timeline).unwrap() else {
        panic!("not a timeline")
    };
    document.children_of(node.tracks.unwrap()).unwrap()
}

/// (schema, name, source start, duration) for each item on each track.
fn signature(document: &Document, timeline: NodeId) -> Vec<Vec<(String, String, f64, f64)>> {
    tracks(document, timeline)
        .into_iter()
        .map(|track| {
            document
                .children_of(track)
                .unwrap()
                .into_iter()
                .map(|item| {
                    let node = document.try_get(item).unwrap();
                    let range = document.trimmed_range(item).unwrap();
                    let start = if matches!(node, Node::Clip(_)) {
                        range.start_time().value()
                    } else {
                        0.0
                    };
                    (
                        node.schema_name().to_string(),
                        node.name().to_string(),
                        start,
                        range.duration().value(),
                    )
                })
                .collect()
        })
        .collect()
}

fn lines(json: &str) -> Vec<String> {
    let mut lines: Vec<String> = json
        .lines()
        .map(|line| line.trim().trim_end_matches(',').to_string())
        .collect();
    lines.sort();
    lines
}

#[test]
fn files_from_the_cpp_prototype_read_as_multicams() {
    let (document, timeline) = read("multicam_synthetic.otio");
    let definitions = multicam::multicams(&document, timeline).unwrap();
    assert_eq!(definitions.keys().collect::<Vec<_>>(), ["mc", "quad"]);

    let mc = definitions["mc"];
    let node = document.try_get(mc).unwrap();
    assert!(multicam::is_multicam(node));
    assert!(matches!(node, Node::Stack(_)), "a multicam is a stack");
    assert_eq!(
        multicam::global_start_time(node),
        Some(RationalTime::new(90000.0, 25.0))
    );
    assert_eq!(
        multicam::angle_names(&document, mc).unwrap(),
        ["A", "B", "REC"]
    );

    let audio = tracks(&document, timeline)[1];
    let first = document.children_of(audio).unwrap()[0];
    let node = document.try_get(first).unwrap();
    assert!(matches!(node, Node::Item(_)), "a cut is an item");
    assert_eq!(
        multicam::cut(node),
        Some(Cut {
            multicam: "mc".into(),
            angle: "A".into(),
            angle_track: 1
        })
    );
    let resolved = multicam::resolve(&document, timeline, first, None).unwrap();
    assert_eq!(resolved.multicam, mc);
    let angle = multicam::angle(&document, mc, "A").unwrap().unwrap();
    assert_eq!(
        resolved.track,
        multicam::angle_tracks(&document, angle, "Audio").unwrap()[1]
    );
    assert_eq!(
        multicam::validate(&document, timeline).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn a_round_trip_keeps_every_field() {
    let (document, _) = read("multicam_synthetic.otio");
    let once = otio_core::to_string(&document).unwrap();
    let again = otio_core::to_string(&otio_core::from_str(&once).unwrap()).unwrap();
    assert_eq!(once, again);
    // Field order can differ from the C++ prototype's (extension fields are
    // written first), so compare the lines as a multiset rather than bytes.
    assert_eq!(lines(&once), lines(&data("multicam_synthetic.otio")));
}

#[test]
fn flattening_matches_the_cpp_prototype() {
    for (input, expected) in [
        ("multicam_synthetic.otio", "multicam_synthetic_flat.otio"),
        ("avid_mcam_edit_multicam.otio", "avid_mcam_edit_flat.otio"),
    ] {
        let (mut document, timeline) = read(input);
        let report = multicam::flatten(&mut document, timeline).unwrap();
        assert_eq!(report, Vec::<String>::new(), "{input}");
        assert!(multicam::multicams(&document, timeline).unwrap().is_empty());

        let (want, want_timeline) = read(expected);
        assert_eq!(
            signature(&document, timeline),
            signature(&want, want_timeline),
            "{input}"
        );
        assert_eq!(
            otio_core::to_string(&document).unwrap(),
            otio_core::to_string(&want).unwrap(),
            "{input}: same JSON as the C++ flatten"
        );
    }
}

#[test]
fn trevors_avid_edit_has_thirteen_angles_and_sixty_two_cuts() {
    let (document, timeline) = read("avid_mcam_edit_multicam.otio");
    let definitions = multicam::multicams(&document, timeline).unwrap();
    assert_eq!(definitions.len(), 1);
    let mc = *definitions.values().next().unwrap();
    assert_eq!(multicam::angle_names(&document, mc).unwrap().len(), 13);
    let a01 = multicam::angle(&document, mc, "A01").unwrap().unwrap();
    assert_eq!(
        multicam::angle_tracks(&document, a01, "Audio")
            .unwrap()
            .len(),
        16
    );
    let Node::Timeline(node) = document.try_get(timeline).unwrap() else {
        unreachable!()
    };
    let cuts = document
        .find_children(
            node.tracks.unwrap(),
            None,
            false,
            &multicam::is_multicam_clip,
        )
        .unwrap();
    assert_eq!(cuts.len(), 62);
    assert_eq!(
        multicam::validate(&document, timeline).unwrap(),
        Vec::<String>::new()
    );
}

fn built() -> (Document, NodeId, NodeId) {
    let mut document: Document = otio_core::from_str(
        r#"{"OTIO_SCHEMA": "Timeline.1", "name": "edit",
            "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
              {"OTIO_SCHEMA": "Track.1", "kind": "Video", "children": []}]}}"#,
    )
    .unwrap();
    let timeline = document.root().unwrap();
    let video = tracks(&document, timeline)[0];

    let mc = multicam::new_multicam(&mut document, "Group", None);
    let angle = document.insert(Node::Stack(otio_core::schema::Stack {
        item: otio_core::schema::ItemData {
            base: otio_core::schema::Base::new("A".into(), Default::default()),
            ..otio_core::schema::ItemData::new()
        },
        children: Vec::new(),
    }));
    document.append_child(mc, angle).unwrap();
    multicam::set_multicam(&mut document, timeline, "mc", Some(mc)).unwrap();

    let cut = multicam::new_multicam_clip(
        &mut document,
        "c1",
        &Cut {
            multicam: "mc".into(),
            angle: "A".into(),
            angle_track: 0,
        },
        TimeRange::new(RationalTime::new(0.0, 25.0), RationalTime::new(10.0, 25.0)),
    );
    document.append_child(video, cut).unwrap();
    (document, timeline, cut)
}

#[test]
fn built_in_code_and_validation() {
    let (mut document, timeline, cut) = built();
    // the angle has no video track yet
    let problems = multicam::validate(&document, timeline).unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("has 0 Video track(s)"), "{problems:?}");
    assert!(matches!(
        multicam::resolve(&document, timeline, cut, None),
        Err(Error::UnresolvedMulticam { .. })
    ));

    multicam::set_cut(
        &mut document,
        cut,
        &Cut {
            multicam: "nope".into(),
            angle: "A".into(),
            angle_track: 0,
        },
    )
    .unwrap();
    let problems = multicam::validate(&document, timeline).unwrap();
    assert_eq!(problems, ["'c1': no multicam with key 'nope'"]);

    // flattening an unresolved cut leaves a gap of the same length
    let report = multicam::flatten(&mut document, timeline).unwrap();
    assert_eq!(report.len(), 1);
    let sig = signature(&document, timeline);
    assert_eq!(sig[0], [("Gap".to_string(), String::new(), 0.0, 10.0)]);
    let text = otio_core::to_string(&document).unwrap();
    assert!(
        !text.contains("multicams"),
        "definitions are gone after flattening"
    );
}

#[test]
fn removing_the_last_multicam_removes_the_field() {
    let (mut document, timeline, _) = built();
    assert!(
        otio_core::to_string(&document)
            .unwrap()
            .contains("\"multicams\"")
    );
    multicam::set_multicam(&mut document, timeline, "mc", None).unwrap();
    assert!(
        !otio_core::to_string(&document)
            .unwrap()
            .contains("\"multicams\"")
    );
}

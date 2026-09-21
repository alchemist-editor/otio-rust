//! Conformance against upstream's ALE adapter.
//!
//! Every test here is ported from `otio-ale-adapter`'s own
//! `tests/test_ale_adapter.py`, against the same sample files, so that a
//! failure can be read straight against the test it came from. Where the
//! assertion here is weaker or stronger than upstream's, the comment says
//! why.

use std::path::PathBuf;

use opentime::{RationalTime, TimeRange};
use otio_adapter::TextAdapter;
use otio_ale::{Ale, ReadOptions, WriteOptions};
use otio_core::schema::Node;
use otio_core::{Any, AnyDictionary, Document, NodeId};

/// Returns the contents of a vendored sample file.
fn sample(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

/// Reads a sample file with the adapter's usual behaviour.
fn read(name: &str) -> Document {
    Ale::read_from_str(&sample(name), &ReadOptions::default())
        .unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

/// Returns a document's clips, in order.
fn clips(document: &Document) -> Vec<NodeId> {
    let root = document.root().expect("a parsed ALE has a root");
    document.find_clips(root).expect("a collection of clips")
}

/// Returns the metadata dictionary at `path` below an object's own.
fn metadata_at<'a>(document: &'a Document, id: NodeId, path: &[&str]) -> &'a AnyDictionary {
    let mut current = &document
        .try_get(id)
        .expect("a live object")
        .base()
        .expect("an object with metadata")
        .metadata;
    for key in path {
        current = current
            .get(*key)
            .unwrap_or_else(|| panic!("metadata has no {key}"))
            .as_dictionary()
            .unwrap_or_else(|| panic!("{key} is not a dictionary"));
    }
    current
}

/// Returns a string from a metadata dictionary.
fn string(metadata: &AnyDictionary, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Any::as_str)
        .unwrap_or_else(|| panic!("metadata has no string at {key}"))
        .to_string()
}

/// Returns the rate a file's heading states, as written.
fn stated_fps(document: &Document) -> f64 {
    let root = document.root().expect("a root");
    string(metadata_at(document, root, &["ALE", "header"]), "FPS")
        .parse()
        .expect("the heading's FPS is a number")
}

/// Returns the clips' names, in order.
fn names(document: &Document) -> Vec<String> {
    clips(document)
        .into_iter()
        .map(|clip| document.try_get(clip).expect("live").name().to_string())
        .collect()
}

/// Returns the clips' spans of their media, in order.
fn source_ranges(document: &Document) -> Vec<Option<TimeRange>> {
    clips(document)
        .into_iter()
        .map(|clip| {
            document
                .try_get(clip)
                .expect("live")
                .item()
                .expect("a clip is an item")
                .source_range
        })
        .collect()
}

/// Builds the span an ALE row states, from its timecodes.
fn span(start: &str, duration: &str, rate: f64) -> Option<TimeRange> {
    Some(TimeRange::new(
        RationalTime::from_timecode(start, rate).expect("valid timecode"),
        RationalTime::from_timecode(duration, rate).expect("valid timecode"),
    ))
}

/// Returns the three colour-decision channel triples on a clip.
fn asc_sop(document: &Document, clip: NodeId) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let sop = metadata_at(document, clip, &["cdl", "asc_sop"]);
    let channels = |key: &str| -> Vec<f64> {
        sop.get(key)
            .and_then(Any::as_slice)
            .unwrap_or_else(|| panic!("asc_sop has no {key}"))
            .iter()
            .map(|value| match value {
                Any::Double(value) => *value,
                other => panic!("a channel value is {}", other.type_name()),
            })
            .collect()
    };
    (channels("slope"), channels("offset"), channels("power"))
}

/// Returns the saturation on a clip.
fn asc_sat(document: &Document, clip: NodeId) -> f64 {
    match metadata_at(document, clip, &["cdl"]).get("asc_sat") {
        Some(Any::Double(value)) => *value,
        other => panic!("asc_sat is {other:?}"),
    }
}

#[test]
fn reads_a_shot_log() {
    let document = read("sample.ale");
    let root = document.root().expect("a root");
    assert!(matches!(
        document.try_get(root).expect("live"),
        Node::SerializableCollection(_)
    ));

    assert_eq!(clips(&document).len(), 4);
    let fps = stated_fps(&document);
    assert!((fps - 24.0).abs() < f64::EPSILON);

    assert_eq!(
        names(&document),
        ["test_017056", "test_017057", "test_017058", "Something"]
    );
    assert_eq!(
        source_ranges(&document),
        [
            span("01:00:00:00", "00:00:04:03", fps),
            span("01:00:00:00", "00:00:04:04", fps),
            span("01:00:00:00", "00:00:04:05", fps),
            span("01:00:00:00", "00:00:04:06", fps),
        ]
    );
}

#[test]
fn reads_a_heading_written_as_one_line() {
    // sample2.ale puts every heading pair on a single line, and states a rate
    // of 23.98, which is how 24000/1001 is written in the wild.
    let document = read("sample2.ale");
    assert_eq!(clips(&document).len(), 2);

    let stated = stated_fps(&document);
    assert!((stated - 23.98).abs() < f64::EPSILON);
    let rate = RationalTime::nearest_smpte_timecode_rate(stated);

    assert_eq!(names(&document), ["19A-1xa", "19A-2xa"]);
    assert_eq!(
        source_ranges(&document),
        [
            span("04:00:00:00", "00:00:46:16", rate),
            span("04:00:46:16", "00:00:50:16", rate),
        ]
    );
}

#[test]
fn reads_colour_decisions() {
    let document = read("sample_cdl.ale");
    assert_eq!(clips(&document).len(), 4);

    let stated = stated_fps(&document);
    assert!((stated - 23.976).abs() < f64::EPSILON);
    let rate = RationalTime::nearest_smpte_timecode_rate(stated);

    assert_eq!(
        names(&document),
        [
            "A005_C010_0501J0",
            "A005_C010_0501J0",
            "A005_C009_0501A0",
            "A005_C010_0501J0"
        ]
    );
    assert_eq!(
        source_ranges(&document),
        [
            span("17:49:33:01", "00:00:02:09", rate),
            span("17:49:55:19", "00:00:06:09", rate),
            span("17:40:25:06", "00:00:02:20", rate),
            span("17:50:21:23", "00:00:03:14", rate),
        ]
    );

    let clips = clips(&document);
    let expected = [
        (
            [0.8714, 0.9334, 0.9947],
            [-0.087, -0.0922, -0.0808],
            [0.9988, 1.0218, 1.0101],
        ),
        (
            [0.8714, 0.9334, 0.9947],
            [-0.087, -0.0922, -0.0808],
            [0.9988, 1.0218, 1.0101],
        ),
        (
            [0.8604, 0.9252, 0.9755],
            [-0.0735, -0.0813, -0.0737],
            [0.9988, 1.0218, 1.0101],
        ),
        (
            [0.8714, 0.9334, 0.9947],
            [-0.087, -0.0922, -0.0808],
            [0.9988, 1.0218, 1.0101],
        ),
    ];
    for (clip, (slope, offset, power)) in clips.iter().zip(expected) {
        assert_eq!(
            asc_sop(&document, *clip),
            (slope.into(), offset.into(), power.into())
        );
        assert!((asc_sat(&document, *clip) - 0.9).abs() < f64::EPSILON);
    }
}

#[test]
fn keeps_a_heading_that_already_names_the_format() {
    // sampleUHD.ale is 4096x2304, and says CUSTOM itself. The heading wins:
    // guessing only fills a gap.
    let document = read("sampleUHD.ale");
    let root = document.root().expect("a root");
    assert_eq!(
        string(
            metadata_at(&document, root, &["ALE", "header"]),
            "VIDEO_FORMAT"
        ),
        "CUSTOM"
    );
}

#[test]
fn reads_a_file_padded_with_blank_lines() {
    let document = read("sample_blanks.ale");
    assert_eq!(clips(&document).len(), 1);

    // The heading keeps the rate the file states, not the SMPTE rate it is
    // read at. Here they are the same, but for 23.98 they are not.
    let fps = stated_fps(&document);
    assert!((fps - 25.0).abs() < f64::EPSILON);

    assert_eq!(names(&document), ["A020C003_150905_E2XZ.mov"]);
    assert_eq!(
        source_ranges(&document),
        [span("05:42:12:20", "00:00:17:17", fps)]
    );

    let clip = clips(&document)[0];
    assert_eq!(
        asc_sop(&document, clip),
        (
            vec![1.1822, 1.2183, 1.2284],
            vec![-0.2429, -0.2823, -0.2849],
            vec![0.7283, 0.7096, 0.7054],
        )
    );
    assert!((asc_sat(&document, clip) - 1.068).abs() < f64::EPSILON);
}

#[test]
fn reads_a_file_with_no_blank_line_between_sections() {
    let document = read("sample_no_blanks.ale");
    let clips = clips(&document);
    assert_eq!(clips.len(), 6);

    let fields = metadata_at(&document, clips[4], &["ALE"]);
    assert_eq!(string(fields, "Tape"), "A_0076C005_230511_190706_h1CTJ");
    assert_eq!(string(fields, "Take"), "5");
}

#[test]
fn guesses_the_format_from_the_clips() {
    // Upstream's test_ale_add_format: each call adds another clip to the same
    // timeline, so the format tracks the largest frame seen so far.
    let mut document = Document::new();
    let track = document.insert(Node::Track(otio_core::schema::Track {
        item: otio_core::schema::ItemData::new(),
        children: Vec::new(),
        kind: "Video".to_string(),
    }));
    let tracks = document.insert(Node::Stack(otio_core::schema::Stack::default()));
    document
        .append_child(tracks, track)
        .expect("a stack holds tracks");
    let timeline = document.insert(Node::Timeline(otio_core::schema::Timeline {
        base: otio_core::schema::Base {
            name: "Add Format".to_string(),
            metadata: AnyDictionary::new(),
        },
        tracks: Some(tracks),
        global_start_time: None,
    }));
    document.set_root(Some(timeline));

    for (size, expected) in [
        ("720 x 486", "NTSC"),
        ("720 x 576", "PAL"),
        ("1280x 720", "720"),
        ("1920x1080", "1080"),
        ("2048x1080", "CUSTOM"),
        ("4096x2304", "CUSTOM"),
    ] {
        let mut fields = AnyDictionary::new();
        fields.insert("Image Size".to_string(), Any::String(size.to_string()));
        let mut metadata = AnyDictionary::new();
        metadata.insert("ALE".to_string(), Any::Dictionary(fields));

        let rate = 24000.0 / 1001.0;
        let clip = document.insert(Node::Clip(otio_core::schema::Clip {
            item: otio_core::schema::ItemData {
                base: otio_core::schema::Base {
                    name: String::new(),
                    metadata,
                },
                source_range: Some(TimeRange::new(
                    RationalTime::new(0.0, rate),
                    RationalTime::new(48.0, rate),
                )),
                ..otio_core::schema::ItemData::new()
            },
            ..otio_core::schema::Clip::default()
        }));
        document
            .append_child(track, clip)
            .expect("a track holds clips");

        let written = Ale::write_to_string(&document, &WriteOptions::default()).expect("writes");
        let reread = Ale::read_from_str(&written, &ReadOptions::default()).expect("reads back");
        let root = reread.root().expect("a root");
        assert_eq!(
            string(
                metadata_at(&reread, root, &["ALE", "header"]),
                "VIDEO_FORMAT"
            ),
            expected,
            "for {size}"
        );
    }
}

#[test]
fn a_round_trip_reproduces_the_file() {
    // The strongest statement this adapter can make: a real Avid-written ALE
    // read and written back is the same bytes, so passing one through a tool
    // built on this library does not perturb it.
    let original = sample("sample.ale");
    let document = Ale::read_from_str(&original, &ReadOptions::default()).expect("reads");
    let written = Ale::write_to_string(&document, &WriteOptions::default()).expect("writes");
    assert_eq!(written, original);
}

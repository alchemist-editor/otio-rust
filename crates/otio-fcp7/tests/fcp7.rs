//! Conformance against upstream's FCP 7 XML adapter.
//!
//! Every test here is ported from `otio-fcp-adapter`'s own
//! `tests/test_fcp7_xml_adapter.py`, against the same sample files, so that a
//! failure can be read straight against the test it came from. Where the
//! assertion here is weaker or stronger than upstream's, the comment says why.

use std::path::PathBuf;

use opentime::{RationalTime, TimeRange};
use otio_adapter::TextAdapter;
use otio_core::schema::Node;
use otio_core::{Any, AnyDictionary, Document, NodeId};
use otio_fcp7::Fcp7Xml;

/// Returns the contents of a vendored sample file.
fn sample(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

/// Reads a sample file with the adapter's usual behaviour.
fn read(name: &str) -> Document {
    Fcp7Xml::read_from_str(&sample(name), &Default::default())
        .unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

fn read_str(input: &str) -> Document {
    Fcp7Xml::read_from_str(input, &Default::default()).expect("a readable document")
}

fn write(document: &Document) -> String {
    Fcp7Xml::write_to_string(document, &Default::default()).expect("a writable document")
}

/// Returns the stack holding a timeline's tracks.
fn tracks_stack(document: &Document) -> NodeId {
    let root = document.root().expect("a parsed document has a root");
    match document.try_get(root).expect("a live root") {
        Node::Timeline(timeline) => timeline.tracks.expect("the timeline has tracks"),
        other => panic!("expected a Timeline, found a {}", other.schema_name()),
    }
}

/// Returns a timeline's tracks, in order.
fn tracks(document: &Document) -> Vec<NodeId> {
    document
        .children_of(tracks_stack(document))
        .expect("a stack of tracks")
}

/// Returns a timeline's tracks of one kind, in order.
fn tracks_of_kind(document: &Document, kind: &str) -> Vec<NodeId> {
    tracks(document)
        .into_iter()
        .filter(|&track| match document.try_get(track) {
            Ok(Node::Track(data)) => data.kind == kind,
            _ => false,
        })
        .collect()
}

/// Returns the names of a composition's children, in order.
fn child_names(document: &Document, parent: NodeId) -> Vec<String> {
    document
        .children_of(parent)
        .expect("a composition")
        .into_iter()
        .map(|child| {
            document
                .try_get(child)
                .expect("a live child")
                .name()
                .to_string()
        })
        .collect()
}

fn source_range(document: &Document, id: NodeId) -> TimeRange {
    document
        .try_get(id)
        .expect("a live object")
        .item()
        .and_then(|item| item.source_range)
        .expect("an item with a source range")
}

/// Returns the active media reference of a clip.
fn media_reference(document: &Document, clip: NodeId) -> NodeId {
    match document.try_get(clip).expect("a live clip") {
        Node::Clip(clip) => *clip
            .media_references
            .get(&clip.active_media_reference_key)
            .expect("a clip has a media reference"),
        other => panic!("expected a Clip, found a {}", other.schema_name()),
    }
}

/// Returns the `fcp_xml` metadata dictionary of an object, if it has one.
fn fcp_metadata(document: &Document, id: NodeId) -> Option<&AnyDictionary> {
    document
        .try_get(id)
        .expect("a live object")
        .base()?
        .metadata
        .get(otio_fcp7::META_NAMESPACE)?
        .as_dictionary()
}

fn metadata_str<'a>(document: &'a Document, id: NodeId, key: &str) -> Option<&'a str> {
    fcp_metadata(document, id)?.get(key)?.as_str()
}

// ---------------------------------------------------------------- reading --

/// Upstream's `test_read`, against the eight-track Premiere export.
///
/// This is the broadest read assertion upstream has: every track's clip names
/// and every clip's duration, plus the markers on the timeline and on a clip.
#[test]
fn reads_a_premiere_export() {
    let document = read("premiere_example.xml");
    assert_eq!(tracks(&document).len(), 8);

    let video = tracks_of_kind(&document, "Video");
    let audio = tracks_of_kind(&document, "Audio");
    assert_eq!(video.len(), 4);
    assert_eq!(audio.len(), 4);

    let expected_video_names: [&[&str]; 4] = [
        &["", "sc01_sh010_anim.mov"],
        &[
            "",
            "sc01_sh010_anim.mov",
            "",
            "sc01_sh020_anim.mov",
            "sc01_sh030_anim.mov",
            "Cross Dissolve",
            "",
            "sc01_sh010_anim",
        ],
        &["", "test_title"],
        &[
            "",
            "sc01_master_layerA_sh030_temp.mov",
            "Cross Dissolve",
            "sc01_sh010_anim.mov",
        ],
    ];
    for (index, &track) in video.iter().enumerate() {
        assert_eq!(
            child_names(&document, track),
            expected_video_names[index],
            "video track {index}"
        );
    }

    let expected_audio_names: [&[&str]; 4] = [
        &["", "sc01_sh010_anim.mov", "", "sc01_sh010_anim.mov"],
        &["", "sc01_placeholder.wav", "", "sc01_sh010_anim"],
        &["", "track_08.wav"],
        &[
            "",
            "sc01_master_layerA_sh030_temp.mov",
            "sc01_sh010_anim.mov",
        ],
    ];
    for (index, &track) in audio.iter().enumerate() {
        assert_eq!(
            child_names(&document, track),
            expected_audio_names[index],
            "audio track {index}"
        );
    }

    // A transition is checked by its two offsets; everything else by its
    // duration. All of these are at 30fps.
    let expected_video_durations: [&[(f64, f64)]; 4] = [
        &[(536.0, 0.0), (100.0, 0.0)],
        &[
            (13.0, 0.0),
            (100.0, 0.0),
            (52.0, 0.0),
            (157.0, 0.0),
            (235.0, 0.0),
            (19.0, 0.0),
            (79.0, 0.0),
            (320.0, 0.0),
        ],
        &[(15.0, 0.0), (941.0, 0.0)],
        &[(956.0, 0.0), (208.0, 0.0), (12.0, 13.0), (82.0, 0.0)],
    ];
    for (track_index, &track) in video.iter().enumerate() {
        for (child_index, child) in document
            .children_of(track)
            .expect("a track")
            .into_iter()
            .enumerate()
        {
            let (first, second) = expected_video_durations[track_index][child_index];
            match document.try_get(child).expect("a live child") {
                Node::Transition(transition) => {
                    assert_eq!(transition.in_offset, RationalTime::new(first, 30.0));
                    assert_eq!(transition.out_offset, RationalTime::new(second, 30.0));
                }
                _ => assert_eq!(
                    source_range(&document, child).duration(),
                    RationalTime::new(first, 30.0),
                    "video track {track_index} child {child_index}"
                ),
            }
        }
    }

    let expected_audio_durations: [&[f64]; 4] = [
        &[13.0, 100.0, 423.0, 100.0],
        &[335.0, 170.0, 131.0, 294.0],
        &[153.0, 198.0],
        &[956.0, 221.0, 94.0],
    ];
    for (track_index, &track) in audio.iter().enumerate() {
        for (child_index, child) in document
            .children_of(track)
            .expect("a track")
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                source_range(&document, child).duration(),
                RationalTime::new(expected_audio_durations[track_index][child_index], 30.0),
                "audio track {track_index} child {child_index}"
            );
        }
    }

    let stack = tracks_stack(&document);
    let markers = document
        .try_get(stack)
        .expect("a live stack")
        .item()
        .expect("a stack is an item")
        .markers
        .clone();

    let expected_markers = [
        ("My MArker 1", 113.0, Some("so, this happened")),
        ("dsf", 492.0, Some("fsfsfs")),
        ("", 298.0, None),
    ];
    for (index, &marker) in markers.iter().enumerate() {
        let (name, start, comment) = expected_markers[index];
        let node = document.try_get(marker).expect("a live marker");
        assert_eq!(node.name(), name);
        let Node::Marker(data) = node else {
            panic!("expected a Marker");
        };
        assert_eq!(
            data.marked_range.start_time(),
            RationalTime::new(start, 30.0)
        );
        assert_eq!(metadata_str(&document, marker, "comment"), comment);
    }

    // The marker on the fifth clip of the second video track.
    let clip = document.children_of(video[1]).expect("a track")[4];
    let clip_markers = document
        .try_get(clip)
        .expect("a live clip")
        .item()
        .expect("a clip is an item")
        .markers
        .clone();
    let marker = clip_markers[0];
    assert_eq!(document.try_get(marker).expect("a live marker").name(), "");
    let Node::Marker(data) = document.try_get(marker).expect("a live marker") else {
        panic!("expected a Marker");
    };
    assert_eq!(
        data.marked_range.start_time(),
        RationalTime::new(73.0, 30.0)
    );
    assert_eq!(metadata_str(&document, marker, "comment"), None);
}

/// Upstream's `test_hiero_flavored_xml`.
///
/// Hiero writes a much sparser file than Premiere does, and in particular
/// writes a clip with no `pathurl` at all.
#[test]
fn reads_hieros_dialect() {
    let document = read("hiero_xml_export.xml");
    let tracks = tracks(&document);
    assert_eq!(tracks.len(), 1);
    assert_eq!(
        document.try_get(tracks[0]).expect("a live track").name(),
        "Video 1"
    );

    let clips = document.find_clips(tracks[0]).expect("a track of clips");
    assert_eq!(clips.len(), 2);
    assert_eq!(
        document.try_get(clips[0]).expect("a live clip").name(),
        "A160C005_171213_R0MN"
    );
    assert_eq!(document.try_get(clips[1]).expect("a live clip").name(), "/");

    assert!(matches!(
        document
            .try_get(media_reference(&document, clips[0]))
            .expect("a live reference"),
        Node::ExternalReference(_)
    ));
    assert!(matches!(
        document
            .try_get(media_reference(&document, clips[1]))
            .expect("a live reference"),
        Node::MissingReference(_)
    ));

    let expected = TimeRange::new(
        RationalTime::new(1_101_071.0, 24.0),
        RationalTime::new(1055.0, 24.0),
    );
    assert_eq!(source_range(&document, clips[0]), expected);
    assert_eq!(
        document
            .available_range(clips[0])
            .expect("an available range"),
        expected
    );

    // The clip with no media still gets a range, because the file states a
    // timecode for it even though it states no duration.
    assert_eq!(
        document
            .available_range(clips[1])
            .expect("an available range"),
        TimeRange::new(RationalTime::default(), RationalTime::new(1.0, 24.0))
    );

    // Upstream only checks that writing produces something different from the
    // input, since OTIO carries a subset of what the format can say.
    assert_ne!(write(&document), sample("hiero_xml_export.xml"));
}

/// Upstream's `test_xml_with_empty_elements`.
///
/// This file used to throw on load. Its `name` elements are empty, which is
/// what makes the difference between "no name" and "no element" matter.
#[test]
fn reads_a_file_whose_name_elements_are_empty() {
    let document = read("empty_name_tags.xml");
    let video = tracks_of_kind(&document, "Video");
    assert_eq!(video.len(), 12);
    assert_eq!(document.children_of(video[0]).expect("a track").len(), 34);
}

/// Upstream's `test_read_generators`.
#[test]
fn reads_premieres_two_kinds_of_generator() {
    let document = read("premiere_generators.xml");
    let tracks = tracks(&document);

    let video = document.children_of(tracks[0]).expect("a track");
    let audio = document.children_of(tracks[3]).expect("a track");
    assert_eq!(video.len(), 6);
    assert_eq!(audio.len(), 3);

    let kinds: Vec<String> = video
        .iter()
        .map(|&clip| {
            match document
                .try_get(media_reference(&document, clip))
                .expect("a live reference")
            {
                Node::GeneratorReference(generator) => generator.generator_kind.clone(),
                other => panic!(
                    "expected a GeneratorReference, found a {}",
                    other.schema_name()
                ),
            }
        })
        .collect();
    assert_eq!(
        kinds,
        ["Slug", "Slug", "Color", "Slug", "Slug", "GraphicAndType"]
    );
}

/// Upstream's `test_enable_property`.
#[test]
fn reads_whether_a_track_and_a_clip_are_enabled() {
    let document = read("premiere_enable_property.xml");
    let tracks = tracks(&document);

    let enabled = |id: NodeId| {
        document
            .try_get(id)
            .expect("a live object")
            .item()
            .expect("an item")
            .enabled
    };

    assert!(!enabled(tracks[2]));
    assert!(enabled(tracks[1]));

    assert!(!enabled(
        document.children_of(tracks[2]).expect("a track")[1]
    ));
    assert!(enabled(
        document.children_of(tracks[0]).expect("a track")[0]
    ));
}

/// Upstream's `test_track_name_property`.
///
/// Premiere puts the name a person sees in an `MZ.TrackName` attribute rather
/// than in the `name` element, which holds its own internal label.
#[test]
fn a_track_takes_premieres_name_over_the_name_element() {
    let document = read("premiere_enable_property.xml");
    let tracks = tracks(&document);
    let audio = tracks_of_kind(&document, "Audio");

    assert_eq!(
        document.try_get(tracks[2]).expect("a live track").name(),
        "disabled_track"
    );
    assert_eq!(
        document.try_get(audio[0]).expect("a live track").name(),
        "audio_with_disabled"
    );
    assert_eq!(document.try_get(audio[1]).expect("a live track").name(), "");
}

/// A file with no sequence in it has no timeline to give back.
#[test]
fn a_file_with_no_sequence_is_refused() {
    let error = Fcp7Xml::read_from_str("<xmeml version=\"4\"/>", &Default::default())
        .expect_err("there is no sequence");
    assert!(
        error.to_string().contains("no top-level sequences"),
        "{error}"
    );
}

/// Several sequences in one file read as a collection, since OTIO has nothing
/// else that holds timelines side by side.
#[test]
fn several_sequences_read_as_a_collection() {
    let document = read_str(
        r#"<xmeml version="4"><project><children>
             <sequence><name>one</name>
               <rate><timebase>24</timebase></rate><media><video/></media></sequence>
             <sequence><name>two</name>
               <rate><timebase>24</timebase></rate><media><video/></media></sequence>
           </children></project></xmeml>"#,
    );

    let root = document.root().expect("a root");
    let Node::SerializableCollection(collection) = document.try_get(root).expect("a live root")
    else {
        panic!("expected a SerializableCollection");
    };
    assert_eq!(collection.base.name, "Sequences");
    assert_eq!(collection.children.len(), 2);
}

// ---------------------------------------------------------------- writing --

/// Upstream's `test_roundtrip_disk2mem2disk`, against the Premiere export.
///
/// The document has to survive a write and a read unchanged. It is the
/// strongest single check on the pair, since it covers every construction the
/// file holds: eight tracks, transitions, nested sequences, markers, filters,
/// and media shared between clips.
#[test]
fn a_premiere_export_survives_a_write_and_a_read() {
    let original = read("premiere_example.xml");
    let written = write(&original);
    let result = read_str(&written);

    // OTIO has no way to express a `link`, so the reader keeps them in
    // metadata and the writer drops them rather than writing links that no
    // longer point anywhere. Upstream scrubs the same key before comparing.
    let mut original = original;
    let mut result = result;
    scrub_links(&mut original);
    scrub_links(&mut result);

    assert_eq!(
        otio_core::to_string(&result).expect("a serializable document"),
        otio_core::to_string(&original).expect("a serializable document"),
    );

    // The text on disk is not identical, because OTIO carries a subset of
    // what the format can say and the writer drops the rest.
    assert_ne!(written, sample("premiere_example.xml"));
}

/// Removes the `link` entries the writer does not write, everywhere in a
/// document.
fn scrub_links(document: &mut Document) {
    let ids: Vec<NodeId> = document.iter().map(|(id, _)| id).collect();
    for id in ids {
        if let Some(base) = document
            .get_mut(id)
            .and_then(otio_core::schema::Node::base_mut)
        {
            scrub_links_in(&mut base.metadata);
        }
    }
}

fn scrub_links_in(dictionary: &mut AnyDictionary) {
    dictionary.remove("link");
    for value in dictionary.values_mut() {
        if let Any::Dictionary(nested) = value {
            scrub_links_in(nested);
        }
    }
}

/// Upstream's `test_roundtrip_mem2disk2mem`, built the same way.
///
/// A timeline assembled by hand, written out, and read back: the point is that
/// nothing a caller put in is lost on the way through the file.
#[test]
fn a_hand_built_timeline_survives_a_write_and_a_read() {
    const RATE: f64 = 48.0;

    let mut document = Document::new();

    let video_reference = external_reference(
        &mut document,
        "test_vid_one",
        "/var/tmp/test1.mov",
        TimeRange::new(
            RationalTime::new(100.0, RATE),
            RationalTime::new(1000.0, RATE),
        ),
    );
    let audio_reference = external_reference(
        &mut document,
        "test_wav_one",
        "/var/tmp/test1.wav",
        TimeRange::new(
            RationalTime::new(0.0, RATE),
            RationalTime::new(1000.0, RATE),
        ),
    );

    let video_track = track(&mut document, "Video");
    let clip_one = clip(
        &mut document,
        "test_clip1",
        video_reference,
        TimeRange::new(
            RationalTime::new(112.0, RATE),
            RationalTime::new(40.0, RATE),
        ),
    );
    let gap_one = gap(&mut document, RationalTime::new(60.0, RATE));
    let clip_two = clip(
        &mut document,
        "test_clip2",
        video_reference,
        TimeRange::new(
            RationalTime::new(123.0, RATE),
            RationalTime::new(260.0, RATE),
        ),
    );
    for child in [clip_one, gap_one, clip_two] {
        document
            .append_child(video_track, child)
            .expect("a track takes children");
    }

    let audio_track = track(&mut document, "Audio");
    let gap_two = gap(&mut document, RationalTime::new(10.0, RATE));
    let clip_three = clip(
        &mut document,
        "test_clip4",
        audio_reference,
        TimeRange::new(
            RationalTime::new(152.0, RATE),
            RationalTime::new(248.0, RATE),
        ),
    );
    for child in [gap_two, clip_three] {
        document
            .append_child(audio_track, child)
            .expect("a track takes children");
    }

    let stack = document.insert(Node::Stack(otio_core::schema::Stack {
        item: otio_core::schema::ItemData::new(),
        children: Vec::new(),
    }));
    document
        .append_child(stack, video_track)
        .expect("a stack takes tracks");
    document
        .append_child(stack, audio_track)
        .expect("a stack takes tracks");

    let timeline = document.insert(Node::Timeline(otio_core::schema::Timeline {
        base: otio_core::schema::Base {
            name: "test_timeline".to_string(),
            metadata: AnyDictionary::new(),
            extension: None,
        },
        tracks: Some(stack),
        global_start_time: Some(RationalTime::new(100.0, RATE)),
    }));
    document.set_root(Some(timeline));

    let result = read_str(&write(&document));

    let root = result.root().expect("a root");
    assert_eq!(
        result.try_get(root).expect("a live root").name(),
        "test_timeline"
    );

    let video = tracks_of_kind(&result, "Video");
    let audio = tracks_of_kind(&result, "Audio");
    assert_eq!(
        child_names(&result, video[0]),
        ["test_clip1", "", "test_clip2"]
    );
    assert_eq!(child_names(&result, audio[0]), ["", "test_clip4"]);

    // The rate of the timeline's start survives, which is what upstream's
    // comment on this test is about.
    let Node::Timeline(read_back) = result.try_get(root).expect("a live root") else {
        panic!("expected a Timeline");
    };
    assert_eq!(
        read_back.global_start_time,
        Some(RationalTime::new(100.0, RATE))
    );

    let clips = result.find_clips(root).expect("a timeline of clips");
    assert_eq!(
        source_range(&result, clips[0]),
        TimeRange::new(
            RationalTime::new(112.0, RATE),
            RationalTime::new(40.0, RATE)
        )
    );
    match result
        .try_get(media_reference(&result, clips[0]))
        .expect("a live reference")
    {
        Node::ExternalReference(reference) => {
            assert_eq!(reference.target_url, "/var/tmp/test1.mov");
            assert_eq!(
                reference.media.available_range,
                Some(TimeRange::new(
                    RationalTime::new(100.0, RATE),
                    RationalTime::new(1000.0, RATE)
                ))
            );
        }
        other => panic!(
            "expected an ExternalReference, found a {}",
            other.schema_name()
        ),
    }
}

/// Upstream's `test_img_seq_media_references`: an image sequence has to write
/// without complaint, with its frame number left as a placeholder.
#[test]
fn an_image_sequence_writes_as_a_printf_path() {
    let document = otio_core::from_str(&sample("img_seq_media_reference.otio"))
        .expect("a readable OTIO document");
    let written = write(&document);
    assert!(
        written.contains("%0"),
        "the frame number should be a placeholder:\n{written}"
    );
}

/// The writer refuses a document it cannot express rather than writing a file
/// that says something else.
#[test]
fn a_document_that_is_not_a_timeline_is_refused() {
    let document = otio_core::from_str(
        r#"{"OTIO_SCHEMA": "Clip.2", "name": "lonely", "media_references": {}}"#,
    )
    .expect("a readable OTIO document");
    let error =
        Fcp7Xml::write_to_string(&document, &Default::default()).expect_err("a clip is not a file");
    assert!(error.to_string().contains("expected a Timeline"), "{error}");
}

// ------------------------------------------------------------- test setup --

// ------------------------------------------------ the deviations on write --

/// A transition's `effect` subtree is the only statement of what the
/// transition actually is, and OTIO has a field for none of it.
///
/// Upstream keeps the effect's display name and drops the rest, so every wipe
/// comes back out as a plain cross dissolve. Kept here instead.
#[test]
fn a_transitions_effect_settings_survive_a_write() {
    let document = read("premiere_example.xml");
    let written = write(&document);

    let transition = written
        .split("<transitionitem>")
        .nth(1)
        .expect("the export has a transition");
    for detail in [
        "<effectid>Cross Dissolve</effectid>",
        "<effectcategory>Dissolve</effectcategory>",
        "<wipecode>0</wipecode>",
        "<wipeaccuracy>100</wipeaccuracy>",
        "<startratio>0</startratio>",
        "<endratio>1</endratio>",
        "<reverse>FALSE</reverse>",
    ] {
        assert!(transition.contains(detail), "missing {detail}");
    }
}

/// `enabled` is a real OTIO field, so the writer answers with it rather than
/// with whatever the file it read happened to say.
///
/// Upstream's writer never looks at the field, so a clip disabled after a read
/// is written as enabled and one re-enabled stays disabled.
#[test]
fn disabling_a_clip_is_written() {
    let mut document = read("premiere_example.xml");
    let root = document.root().expect("a parsed document has a root");
    let clip = document.find_clips(root).expect("a timeline of clips")[0];

    let before = write(&document).matches("<enabled>FALSE</enabled>").count();

    document
        .try_get_mut(clip)
        .expect("a live clip")
        .item_mut()
        .expect("a clip is an item")
        .enabled = false;

    let after = write(&document).matches("<enabled>FALSE</enabled>").count();
    assert_eq!(
        after,
        before + 1,
        "the disabled clip is written as disabled"
    );

    // And the other way: the export has a disabled clip, which re-enabling
    // must take back out.
    let disabled = document
        .find_clips(root)
        .expect("a timeline of clips")
        .into_iter()
        .find(|&clip| {
            document
                .try_get(clip)
                .ok()
                .and_then(Node::item)
                .is_some_and(|item| !item.enabled)
        })
        .expect("the export has a disabled clip");
    document
        .try_get_mut(disabled)
        .expect("a live clip")
        .item_mut()
        .expect("a clip is an item")
        .enabled = true;

    let reenabled = write(&document).matches("<enabled>FALSE</enabled>").count();
    assert_eq!(
        reenabled,
        after - 1,
        "the re-enabled clip is written as enabled"
    );
}

/// A clip's effects are written from its effect list, not from the `filter`
/// elements the file it was read from happened to carry.
///
/// Upstream's writer never looks at `effects`, so an effect deleted in code is
/// written anyway and one added in code is not written at all.
#[test]
fn removing_an_effect_is_written() {
    let mut document = read("hiero_xml_export.xml");
    assert_eq!(write(&document).matches("<filter>").count(), 1);

    let root = document.root().expect("a parsed document has a root");
    for clip in document.find_clips(root).expect("a timeline of clips") {
        if let Some(item) = document.try_get_mut(clip).expect("a live clip").item_mut() {
            item.effects.clear();
        }
    }

    assert_eq!(
        write(&document).matches("<filter>").count(),
        0,
        "a cleared effect list writes no filters"
    );
}

/// A timeline with no `global_start_time` is written as starting at zero, and
/// the rate of that zero is the rate everything in the sequence is then
/// written against.
#[test]
fn a_timeline_with_no_start_keeps_its_rate() {
    const RATE: f64 = 24.0;

    let mut document = Document::new();
    let reference = external_reference(
        &mut document,
        "shot",
        "/var/tmp/shot.mov",
        TimeRange::new(RationalTime::new(0.0, RATE), RationalTime::new(240.0, RATE)),
    );
    let video = track(&mut document, "Video");
    let only = clip(
        &mut document,
        "shot",
        reference,
        TimeRange::new(RationalTime::new(12.0, RATE), RationalTime::new(35.0, RATE)),
    );
    document
        .append_child(video, only)
        .expect("a track takes children");

    let stack = document.insert(Node::Stack(otio_core::schema::Stack {
        item: otio_core::schema::ItemData::new(),
        children: Vec::new(),
    }));
    document
        .append_child(stack, video)
        .expect("a stack takes tracks");
    let timeline = document.insert(Node::Timeline(otio_core::schema::Timeline {
        base: otio_core::schema::Base {
            name: "no start".to_string(),
            metadata: AnyDictionary::new(),
            extension: None,
        },
        tracks: Some(stack),
        global_start_time: None,
    }));
    document.set_root(Some(timeline));

    let written = write(&document);
    assert!(
        written.contains("<timebase>24</timebase>"),
        "the sequence is written at the tracks' rate, not at one frame a second"
    );
    assert!(!written.contains("<timebase>1</timebase>"));

    // And the clip comes back where it went in, which a one-frame-a-second
    // timebase would have rounded away.
    let result = read_str(&written);
    let clips = result
        .find_clips(result.root().expect("a root"))
        .expect("clips");
    assert_eq!(
        source_range(&result, clips[0]),
        TimeRange::new(RationalTime::new(12.0, RATE), RationalTime::new(35.0, RATE))
    );
}

/// A `timecode` that states no rate of its own takes the rate of the `file`
/// holding it, not of the clip the file sits in.
///
/// Upstream reads this timecode in the clip's context while the very same
/// element, read again inside the media reference, gets the file's, so a file
/// whose rate differs from its clip's ends up with a media start that
/// disagrees with its own available range.
#[test]
fn a_files_timecode_is_read_at_the_files_rate() {
    // The track runs at 30, the file at 24, and the timecode states no rate.
    // A drop-frame timecode is the case where the two rates give different
    // answers, so the test uses one.
    let document = read_str(
        r#"<xmeml version="4">
             <sequence>
               <name>rates</name>
               <rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>
               <media><video><track>
                 <clipitem id="clipitem-1">
                   <name>shot</name>
                   <start>0</start><end>48</end><in>0</in><out>48</out>
                   <file id="file-1">
                     <name>shot.mov</name>
                     <pathurl>file:///shot.mov</pathurl>
                     <rate><timebase>24</timebase><ntsc>FALSE</ntsc></rate>
                     <duration>240</duration>
                     <timecode>
                       <string>01:00:00:00</string>
                       <displayformat>NDF</displayformat>
                     </timecode>
                   </file>
                 </clipitem>
               </track></video></media>
             </sequence>
           </xmeml>"#,
    );

    let root = document.root().expect("a parsed document has a root");
    let clip = document.find_clips(root).expect("a timeline of clips")[0];
    let media = media_reference(&document, clip);
    let available = document
        .try_get(media)
        .expect("a live media reference")
        .media()
        .and_then(|media| media.available_range)
        .expect("the file states a duration");

    // The clip starts at the head of its media, so its source range must
    // start where the media does. Reading the timecode at the track's rate
    // instead would put it a different number of frames in.
    assert_eq!(
        source_range(&document, clip).start_time(),
        available.start_time()
    );
    assert_eq!(available.start_time(), RationalTime::new(86_400.0, 24.0));
}

fn external_reference(
    document: &mut Document,
    name: &str,
    target_url: &str,
    available_range: TimeRange,
) -> NodeId {
    document.insert(Node::ExternalReference(
        otio_core::schema::ExternalReference {
            media: otio_core::schema::MediaReferenceData {
                base: otio_core::schema::Base {
                    name: name.to_string(),
                    metadata: AnyDictionary::new(),
                    extension: None,
                },
                available_range: Some(available_range),
                available_image_bounds: None,
            },
            target_url: target_url.to_string(),
        },
    ))
}

fn track(document: &mut Document, kind: &str) -> NodeId {
    document.insert(Node::Track(otio_core::schema::Track {
        item: otio_core::schema::ItemData::new(),
        children: Vec::new(),
        kind: kind.to_string(),
    }))
}

fn clip(document: &mut Document, name: &str, reference: NodeId, source_range: TimeRange) -> NodeId {
    let mut media_references = std::collections::BTreeMap::new();
    media_references.insert("DEFAULT_MEDIA".to_string(), reference);
    document.insert(Node::Clip(otio_core::schema::Clip {
        item: otio_core::schema::ItemData {
            base: otio_core::schema::Base {
                name: name.to_string(),
                metadata: AnyDictionary::new(),
                extension: None,
            },
            source_range: Some(source_range),
            ..otio_core::schema::ItemData::new()
        },
        media_references,
        active_media_reference_key: "DEFAULT_MEDIA".to_string(),
    }))
}

fn gap(document: &mut Document, duration: RationalTime) -> NodeId {
    document.insert(Node::Gap(otio_core::schema::Gap {
        item: otio_core::schema::ItemData {
            source_range: Some(TimeRange::new(
                RationalTime::new(0.0, duration.rate()),
                duration,
            )),
            ..otio_core::schema::ItemData::new()
        },
    }))
}

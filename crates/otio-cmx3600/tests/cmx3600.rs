//! Conformance against upstream's CMX 3600 adapter.
//!
//! Every test here is ported from `otio-cmx3600-adapter`'s own
//! `tests/test_cmx_3600_adapter.py`, against the same sample files, so that a
//! failure can be read straight against the test it came from. Where the
//! assertion here is weaker or stronger than upstream's, the comment says
//! why.

use std::path::PathBuf;

use opentime::{RationalTime, TimeRange};
use otio_adapter::TextAdapter;
use otio_cmx3600::{Cmx3600, ReadOptions, Style, WriteOptions};
use otio_core::schema::Node;
use otio_core::{Any, AnyDictionary, Document, NodeId};

/// Returns the contents of a vendored sample file.
fn sample(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

/// Reads a sample file at the given rate.
fn read_at(name: &str, rate: f64) -> Document {
    let options = ReadOptions {
        rate,
        ..ReadOptions::default()
    };
    Cmx3600::read_from_str(&sample(name), &options)
        .unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

/// Reads a sample file at 24, which is what most of upstream's tests use.
fn read(name: &str) -> Document {
    read_at(name, 24.0)
}

/// Reads an EDL written out in the test itself.
fn parse(input: &str, rate: f64) -> Document {
    let options = ReadOptions {
        rate,
        ..ReadOptions::default()
    };
    Cmx3600::read_from_str(input, &options).expect("a readable EDL")
}

/// Returns the timeline's tracks, in order.
fn tracks(document: &Document) -> Vec<NodeId> {
    let timeline = document.root().expect("a parsed EDL has a timeline");
    let Node::Timeline(timeline) = document.try_get(timeline).expect("a live timeline") else {
        panic!("the root of a parsed EDL is a timeline");
    };
    document
        .children_of(timeline.tracks.expect("a timeline has a stack"))
        .expect("a stack holds tracks")
}

/// Returns one track of the timeline.
fn track(document: &Document, index: usize) -> NodeId {
    tracks(document)[index]
}

/// Returns the items on one track, in order.
fn items(document: &Document, index: usize) -> Vec<NodeId> {
    document
        .children_of(track(document, index))
        .expect("a track holds items")
}

/// Returns one item of one track.
fn item(document: &Document, track: usize, index: usize) -> NodeId {
    items(document, track)[index]
}

/// Returns an object's name.
fn name(document: &Document, id: NodeId) -> String {
    document
        .try_get(id)
        .expect("a live object")
        .name()
        .to_string()
}

/// Returns an item's duration.
fn duration(document: &Document, id: NodeId) -> RationalTime {
    document.duration(id).expect("an item has a duration")
}

/// Returns a timecode as a time at the given rate.
fn tc(timecode: &str, rate: f64) -> RationalTime {
    RationalTime::from_timecode(timecode, rate).expect("a readable timecode")
}

/// Returns the markers on an item.
fn markers(document: &Document, id: NodeId) -> Vec<NodeId> {
    document
        .try_get(id)
        .expect("a live object")
        .item()
        .expect("an item")
        .markers
        .clone()
}

/// Returns the effects on an item.
fn effects(document: &Document, id: NodeId) -> Vec<NodeId> {
    document
        .try_get(id)
        .expect("a live object")
        .item()
        .expect("an item")
        .effects
        .clone()
}

/// Returns an object's metadata dictionary.
fn metadata(document: &Document, id: NodeId) -> &AnyDictionary {
    &document
        .try_get(id)
        .expect("a live object")
        .base()
        .expect("an object with metadata")
        .metadata
}

/// Returns a string from an object's `cmx_3600` metadata.
fn cmx_string(document: &Document, id: NodeId, key: &str) -> Option<String> {
    Some(
        metadata(document, id)
            .get("cmx_3600")?
            .as_dictionary()?
            .get(key)?
            .as_str()?
            .to_string(),
    )
}

/// Writes a document out with the adapter's usual behaviour.
fn write(document: &Document) -> String {
    Cmx3600::write_to_string(document, &WriteOptions::default()).expect("writes")
}

#[test]
fn reads_a_screening_edl() {
    let document = read("screening_example.edl");
    assert_eq!(tracks(&document).len(), 1);

    let expected = [
        ("ZZ100_501 (LAY3)", "00:00:01:07"),
        ("ZZ100_502A (LAY3)", "00:00:02:02"),
        ("ZZ100_503A (LAY1)", "00:00:01:04"),
        ("ZZ100_504C (LAY1)", "00:00:04:19"),
        ("ZZ100_504B (LAY1)", "00:00:04:05"),
        ("ZZ100_507C (LAY2)", "00:00:06:17"),
        ("ZZ100_508 (LAY2)", "00:00:07:02"),
        ("ZZ100_510 (LAY1)", "00:00:05:16"),
        ("ZZ100_510B (LAY1)", "00:00:10:17"),
    ];

    let items = items(&document, 0);
    assert_eq!(items.len(), expected.len());
    for (index, (expected_name, expected_duration)) in expected.into_iter().enumerate() {
        let id = items[index];
        assert_eq!(name(&document, id), expected_name, "clip {index}");
        assert_eq!(
            document
                .trimmed_range(id)
                .expect("a clip has a range")
                .duration(),
            tc(expected_duration, 24.0),
            "clip {index}"
        );
    }
}

#[test]
fn reads_the_locators_on_a_clip_as_markers() {
    let document = read("screening_example.edl");

    let fourth = item(&document, 0, 3);
    let on_fourth = markers(&document, fourth);
    assert_eq!(on_fourth.len(), 2);

    let marker = on_fourth[0];
    assert_eq!(name(&document, marker), "ANIM FIX NEEDED");
    assert_eq!(
        cmx_string(&document, marker, "color").as_deref(),
        Some("RED")
    );
    let Node::Marker(marker) = document.try_get(marker).expect("a live marker") else {
        panic!("a marker");
    };
    assert_eq!(marker.marked_range.start_time(), tc("01:00:01:14", 24.0));
    // Upstream compares against `MarkerColor.RED`, which is the string "RED".
    // Here a colour is a real colour, and `otio-core` canonicalizes the
    // legacy name to "Red" and gives it its components, so assert on both.
    let color = marker.color.as_ref().expect("a coloured marker");
    assert_eq!(color.name, "Red");
    assert_eq!((color.r, color.g, color.b, color.a), (1.0, 0.0, 0.0, 1.0));

    // A locator with nothing after its colour is still a marker, with no name.
    let seventh = item(&document, 0, 6);
    let unnamed = markers(&document, seventh)[0];
    assert_eq!(name(&document, unnamed), "");
}

#[test]
fn tabs_and_single_spaces_read_as_well_as_columns() {
    let document = parse(
        "001  Z10 V  C\t\t01:00:04:05 01:00:05:12 00:59:53:11 00:59:54:18",
        24.0,
    );
    assert_eq!(tracks(&document).len(), 1);

    let track = track(&document, 0);
    let Node::Track(track_data) = document.try_get(track).expect("a live track") else {
        panic!("a track");
    };
    assert_eq!(track_data.kind, "Video");

    let items = items(&document, 0);
    assert_eq!(items.len(), 1);
    let range = document
        .trimmed_range(items[0])
        .expect("a clip has a range");
    assert_eq!(range.start_time().value(), 86501.0);
    assert_eq!(range.duration().value(), 31.0);
}

#[test]
fn a_file_without_column_padding_reads_the_same() {
    // Upstream compares the two documents for equality; comparing the clips'
    // names and ranges says the same thing without depending on the
    // serializer.
    let spaced = read("screening_example.edl");
    let unspaced = read("no_spaces_test.edl");

    let spaced_items = items(&spaced, 0);
    let unspaced_items = items(&unspaced, 0);
    assert_eq!(spaced_items.len(), unspaced_items.len());
    for (left, right) in spaced_items.into_iter().zip(unspaced_items) {
        assert_eq!(name(&spaced, left), name(&unspaced, right));
        assert_eq!(
            spaced.trimmed_range(left).expect("a range"),
            unspaced.trimmed_range(right).expect("a range")
        );
    }
}

/// Returns a clip's active media reference.
fn media_reference(document: &Document, clip: NodeId) -> NodeId {
    let Node::Clip(clip) = document.try_get(clip).expect("a live clip") else {
        panic!("a clip");
    };
    *clip
        .media_references
        .get(&clip.active_media_reference_key)
        .expect("a clip's active media reference")
}

/// Returns the kind of generator a clip draws from, if it draws from one.
fn generator_kind(document: &Document, clip: NodeId) -> Option<String> {
    let reference = media_reference(document, clip);
    match document.try_get(reference).expect("a live reference") {
        Node::GeneratorReference(generator) => Some(generator.generator_kind.clone()),
        _ => None,
    }
}

/// Returns a transition, and asserts that the object is one.
fn transition(document: &Document, id: NodeId) -> &otio_core::schema::Transition {
    match document.try_get(id).expect("a live object") {
        Node::Transition(transition) => transition,
        other => panic!("expected a transition, found {other:?}"),
    }
}

/// Returns how long an item's visible media runs, in frames.
fn visible_frames(document: &Document, id: NodeId) -> f64 {
    document
        .visible_range(id)
        .expect("an item has a visible range")
        .duration()
        .value()
}

#[test]
fn reads_a_dissolve_at_the_head_of_a_clip() {
    let document = read("dissolve_test.edl");
    let items = items(&document, 0);
    // clip, transition, clip, clip
    assert_eq!(items.len(), 4);

    assert_eq!(duration(&document, items[0]).value(), 9.0);
    // The visible range has to hold every frame the transition needs: the
    // edit's own duration plus the transition's.
    assert_eq!(visible_frames(&document, items[0]), 19.0);
    assert_eq!(name(&document, items[0]), "clip_A");

    assert_eq!(duration(&document, items[1]).value(), 10.0);
    assert_eq!(
        name(&document, items[1]),
        "SMPTE_Dissolve from clip_A to clip_B"
    );

    assert_eq!(duration(&document, items[2]).value(), 10.0);
    assert_eq!(visible_frames(&document, items[2]), 10.0);
    assert_eq!(name(&document, items[2]), "clip_B");

    assert_eq!(duration(&document, items[3]).value(), 1.0);
}

#[test]
fn reads_a_dissolve_in_the_middle_of_a_clip() {
    let document = read("dissolve_test_2.edl");
    let items = items(&document, 0);
    assert_eq!(items.len(), 4);

    assert_eq!(duration(&document, items[0]).value(), 5.0);
    assert_eq!(visible_frames(&document, items[0]), 15.0);

    assert_eq!(duration(&document, items[1]).value(), 10.0);
    assert_eq!(
        name(&document, items[1]),
        "SMPTE_Dissolve from clip_A to clip_B"
    );

    assert_eq!(
        document
            .trimmed_range(items[2])
            .expect("a range")
            .start_time()
            .value(),
        tc("01:00:08:04", 24.0).value()
    );
    assert_eq!(name(&document, items[2]), "clip_B");
    assert_eq!(duration(&document, items[2]).value(), 10.0);
    assert_eq!(visible_frames(&document, items[2]), 10.0);
}

#[test]
fn reads_a_dissolve_that_covers_a_whole_clip() {
    let document = read("dissolve_test_3.edl");
    let items = items(&document, 0);
    assert_eq!(items.len(), 4);

    assert_eq!(name(&document, items[0]), "Clip_A.mov");
    assert_eq!(duration(&document, items[0]).value(), 61.0);
    assert_eq!(visible_frames(&document, items[0]), 61.0 + 30.0);

    // The clip names in the file are wrong: the dissolve really runs from
    // Clip_A to Clip_B. The adapter reports what the file says.
    let dissolve = transition(&document, items[1]);
    assert_eq!(
        dissolve.base.name,
        "SMPTE_Dissolve from Clip_B.mov to Clip_C.mov"
    );
    assert_eq!(dissolve.in_offset.value(), 0.0);
    assert_eq!(dissolve.out_offset.value(), 30.0);

    assert_eq!(name(&document, items[2]), "Clip_C.mov");
    assert_eq!(
        document
            .trimmed_range(items[2])
            .expect("a range")
            .start_time()
            .value(),
        86400.0 + (33.0 * 24.0 + 22.0)
    );
    assert_eq!(duration(&document, items[2]).value(), 30.0);
    assert_eq!(visible_frames(&document, items[2]), 30.0);

    assert_eq!(name(&document, items[3]), "Clip_D.mov");
    assert_eq!(
        document
            .trimmed_range(items[3])
            .expect("a range")
            .start_time()
            .value(),
        86400.0
    );
    assert_eq!(duration(&document, items[3]).value(), 46.0);
}

#[test]
fn a_dissolve_of_an_odd_number_of_frames_keeps_the_track_length() {
    let document = parse(
        "1 CLPA V C     00:00:04:17 00:00:07:02 00:00:00:00 00:00:02:09\n\
         2 CLPA V C     00:00:07:02 00:00:07:02 00:00:02:09 00:00:02:09\n\
         2 CLPB V D 027 00:00:06:18 00:00:07:21 00:00:02:09 00:00:03:12\n\
         3 CLPB V C     00:00:07:21 00:00:15:21 00:00:03:12 00:00:11:12\n",
        24.0,
    );
    // Upstream asks the timeline; here a timeline is not an item, so ask the
    // stack it holds, which is what upstream's `Timeline.duration` does.
    let timeline = document.root().expect("a timeline");
    let Node::Timeline(timeline) = document.try_get(timeline).expect("a live timeline") else {
        panic!("a timeline");
    };
    let stack = timeline.tracks.expect("a timeline has a stack");
    assert_eq!(duration(&document, stack).value(), (11.0 * 24.0) + 12.0);
}

#[test]
fn reads_a_wipe() {
    let document = read("wipe_test.edl");
    let items = items(&document, 0);
    assert_eq!(items.len(), 4);

    let wipe = transition(&document, items[1]);
    assert_eq!(wipe.transition_type, "SMPTE_Wipe");
    assert_eq!(
        cmx_string(&document, items[1], "transition").as_deref(),
        Some("W001")
    );

    assert_eq!(duration(&document, items[0]).value(), 9.0);
    assert_eq!(visible_frames(&document, items[0]), 19.0);
    assert_eq!(duration(&document, items[2]).value(), 10.0);
    assert_eq!(visible_frames(&document, items[2]), 10.0);
    assert_eq!(duration(&document, items[3]).value(), 1.0);
}

#[test]
fn reads_a_fade_to_black() {
    let document = parse(
        "1 CLPA V C     00:00:03:18 00:00:12:15 00:00:00:00 00:00:08:21\n\
         2 CLPA V C     00:00:12:15 00:00:12:15 00:00:08:21 00:00:08:21\n\
         2 BL   V D 024 00:00:00:00 00:00:01:00 00:00:08:21 00:00:09:21\n",
        24.0,
    );
    let items = items(&document, 0);
    assert_eq!(items.len(), 3);

    transition(&document, items[1]);
    assert!(matches!(
        document.try_get(items[2]).expect("a live object"),
        Node::Clip(_)
    ));
    assert_eq!(
        generator_kind(&document, items[2]).as_deref(),
        Some("black")
    );
    assert_eq!(duration(&document, items[2]).value(), 24.0);
    assert_eq!(
        document
            .trimmed_range(items[2])
            .expect("a range")
            .start_time()
            .value(),
        0.0
    );
}

#[test]
fn reads_the_reels_that_name_a_generator() {
    let document = parse(
        "1 BL V C 00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00\n\
         2 BLACK V C 00:00:00:00 00:00:01:00 00:00:01:00 00:00:02:00\n\
         3 BARS V C 00:00:00:00 00:00:01:00 00:00:02:00 00:00:03:00\n",
        24.0,
    );
    let items = items(&document, 0);
    assert_eq!(
        generator_kind(&document, items[0]).as_deref(),
        Some("black")
    );
    assert_eq!(
        generator_kind(&document, items[1]).as_deref(),
        Some("black")
    );
    assert_eq!(
        generator_kind(&document, items[2]).as_deref(),
        Some("SMPTEBars")
    );
}

#[test]
fn reads_a_file_stated_at_twenty_five() {
    let document = read_at("25fps.edl", 25.0);
    let items = items(&document, 0);
    for (index, expected) in [161.0, 200.0, 86.0, 49.0].into_iter().enumerate() {
        assert_eq!(
            document
                .trimmed_range(items[index])
                .expect("a range")
                .duration()
                .value(),
            expected,
            "clip {index}"
        );
    }
}

#[test]
fn a_hole_in_the_record_timecode_becomes_a_gap() {
    let document = read("gap_test.edl");
    let track = track(&document, 0);
    let items = items(&document, 0);
    assert_eq!(items.len(), 5);
    assert_eq!(duration(&document, track).value(), 5.0 * 24.0 + 6.0);

    // clip, gap, clip, gap, clip
    for (index, expected) in [24.0, 16.0, 24.0, 38.0, 24.0].into_iter().enumerate() {
        assert_eq!(
            duration(&document, items[index]).value(),
            expected,
            "item {index}"
        );
    }

    let expected_starts = [0.0, 24.0, 40.0, 64.0, 102.0];
    let expected_durations = [24.0, 16.0, 24.0, 38.0, 24.0];
    for (index, id) in items.into_iter().enumerate() {
        let range = document.range_in_parent(id).expect("a placed item");
        assert_eq!(
            range.start_time(),
            RationalTime::from_frames(expected_starts[index], 24.0),
            "item {index}"
        );
        assert_eq!(
            range.duration(),
            RationalTime::from_frames(expected_durations[index], 24.0),
            "item {index}"
        );
    }
}

/// Builds an empty timeline with one video track, and returns both.
fn timeline_with_a_track(title: &str, kind: &str) -> (Document, NodeId) {
    use otio_core::schema::{Base, ItemData, Stack, Timeline, Track};

    let mut document = Document::new();
    let track = document.insert(Node::Track(Track {
        item: ItemData {
            base: Base {
                name: kind.to_string(),
                metadata: AnyDictionary::new(),
            },
            ..ItemData::new()
        },
        children: Vec::new(),
        kind: "Video".to_string(),
    }));
    let tracks = document.insert(Node::Stack(Stack::default()));
    document
        .append_child(tracks, track)
        .expect("a stack holds tracks");
    let timeline = document.insert(Node::Timeline(Timeline {
        base: Base {
            name: title.to_string(),
            metadata: AnyDictionary::new(),
        },
        tracks: Some(tracks),
        global_start_time: None,
    }));
    document.set_root(Some(timeline));
    (document, track)
}

/// Appends a clip drawing on `url`, running `frames` frames from the head of
/// its media.
fn append_clip(document: &mut Document, track: NodeId, clip_name: &str, url: &str, frames: f64) {
    use otio_core::schema::{Base, Clip, ExternalReference, ItemData, MediaReferenceData};
    use otio_core::upgrade::DEFAULT_MEDIA_KEY;

    let reference = document.insert(Node::ExternalReference(ExternalReference {
        media: MediaReferenceData::default(),
        target_url: url.to_string(),
    }));
    let mut media_references = std::collections::BTreeMap::new();
    media_references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);

    let clip = document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base {
                name: clip_name.to_string(),
                metadata: AnyDictionary::new(),
            },
            source_range: Some(TimeRange::new(
                RationalTime::new(0.0, 24.0),
                RationalTime::new(frames, 24.0),
            )),
            ..ItemData::new()
        },
        media_references,
        active_media_reference_key: DEFAULT_MEDIA_KEY.to_string(),
    }));
    document
        .append_child(track, clip)
        .expect("a track holds clips");
}

/// Appends a gap of `frames` frames.
fn append_gap(document: &mut Document, track: NodeId, frames: f64) {
    use otio_core::schema::{Gap, ItemData};

    let gap = document.insert(Node::Gap(Gap {
        item: ItemData {
            source_range: Some(TimeRange::new(
                RationalTime::new(0.0, 24.0),
                RationalTime::new(frames, 24.0),
            )),
            ..ItemData::new()
        },
    }));
    document
        .append_child(track, gap)
        .expect("a track holds gaps");
}

/// Writes a document out in one dialect.
fn write_as(document: &Document, style: Style) -> String {
    let options = WriteOptions {
        style,
        ..WriteOptions::default()
    };
    Cmx3600::write_to_string(document, &options).expect("writes")
}

#[test]
fn each_dialect_names_media_its_own_way() {
    let (mut document, track) = timeline_with_a_track("temp", "V");
    append_clip(
        &mut document,
        track,
        "test clip1",
        "S:/var/tmp/test.exr",
        5.0,
    );
    append_gap(&mut document, track, 24.0);
    append_clip(
        &mut document,
        track,
        "test clip2",
        "S:/var/tmp/test.exr",
        5.0,
    );

    let expected = [
        (
            Style::Nucoda,
            "test_nucoda_timeline",
            "\
001  test     V     C        00:00:00:00 00:00:00:05 00:00:00:00 00:00:00:05
* FROM CLIP NAME:  test clip1
* FROM FILE: S:/var/tmp/test.exr
* OTIO TRUNCATED REEL NAME FROM: test.exr
002  test     V     C        00:00:00:00 00:00:00:05 00:00:01:05 00:00:01:10
* FROM CLIP NAME:  test clip2
* FROM FILE: S:/var/tmp/test.exr
* OTIO TRUNCATED REEL NAME FROM: test.exr
",
        ),
        (
            Style::Avid,
            "test_avid_timeline",
            "\
001  test     V     C        00:00:00:00 00:00:00:05 00:00:00:00 00:00:00:05
* FROM CLIP NAME:  test clip1
* FROM CLIP: S:/var/tmp/test.exr
* OTIO TRUNCATED REEL NAME FROM: test.exr
002  test     V     C        00:00:00:00 00:00:00:05 00:00:01:05 00:00:01:10
* FROM CLIP NAME:  test clip2
* FROM CLIP: S:/var/tmp/test.exr
* OTIO TRUNCATED REEL NAME FROM: test.exr
",
        ),
        (
            // Premiere reads a FROM comment as meaning the clip has no name,
            // so the path goes in a comment Premiere ignores instead.
            Style::Premiere,
            "test_premiere_timeline",
            "\
001  AX       V     C        00:00:00:00 00:00:00:05 00:00:00:00 00:00:00:05
* FROM CLIP NAME:  test.exr
* OTIO REFERENCE FROM: S:/var/tmp/test.exr
* OTIO TRUNCATED REEL NAME FROM: test.exr
002  AX       V     C        00:00:00:00 00:00:00:05 00:00:01:05 00:00:01:10
* FROM CLIP NAME:  test.exr
* OTIO REFERENCE FROM: S:/var/tmp/test.exr
* OTIO TRUNCATED REEL NAME FROM: test.exr
",
        ),
    ];

    for (style, title, body) in expected {
        let root = document.root().expect("a timeline");
        let Node::Timeline(timeline) = document.try_get_mut(root).expect("a live timeline") else {
            panic!("a timeline");
        };
        timeline.base.name = title.to_string();

        assert_eq!(
            write_as(&document, style),
            format!("TITLE: {title}\n\n{body}"),
            "as {style}"
        );
    }
}

#[test]
fn a_reel_name_is_padded_or_truncated_to_the_length_asked_for() {
    let (mut document, track) = timeline_with_a_track("test_timeline", "V1");
    append_clip(
        &mut document,
        track,
        "test clip1",
        "/var/tmp/test_a_really_really_long_filename.mov",
        5.0,
    );

    // Eight characters by default, with the full name kept in a comment so
    // that reading the file back gets it.
    assert_eq!(
        write(&document),
        "\
TITLE: test_timeline

001  testarea V     C        00:00:00:00 00:00:00:05 00:00:00:00 00:00:00:05
* FROM CLIP NAME:  test clip1
* FROM CLIP: /var/tmp/test_a_really_really_long_filename.mov
* OTIO TRUNCATED REEL NAME FROM: test_a_really_really_long_filename.mov
"
    );

    // No length keeps the whole name, which most systems will not read but
    // loses nothing.
    let whole = WriteOptions {
        reelname_len: None,
        ..WriteOptions::default()
    };
    assert_eq!(
        Cmx3600::write_to_string(&document, &whole).expect("writes"),
        "\
TITLE: test_timeline

001  test_a_really_really_long_filename V     C        00:00:00:00 00:00:00:05 00:00:00:00 00:00:00:05
* FROM CLIP NAME:  test clip1
* FROM CLIP: /var/tmp/test_a_really_really_long_filename.mov
"
    );

    let twelve = WriteOptions {
        reelname_len: Some(12),
        ..WriteOptions::default()
    };
    assert_eq!(
        Cmx3600::write_to_string(&document, &twelve).expect("writes"),
        "\
TITLE: test_timeline

001  testareallyr V     C        00:00:00:00 00:00:00:05 00:00:00:00 00:00:00:05
* FROM CLIP NAME:  test clip1
* FROM CLIP: /var/tmp/test_a_really_really_long_filename.mov
* OTIO TRUNCATED REEL NAME FROM: test_a_really_really_long_filename.mov
"
    );
}

#[test]
fn a_nucoda_file_written_back_out_is_the_same_bytes() {
    let original = "\
TITLE: Reels_Example.01

001  ZZ100_50 V     C        01:00:04:05 01:00:05:12 00:59:53:11 00:59:54:18
* FROM CLIP NAME:  take_1
* FROM FILE: S:/path/to/ZZ100_501.take_1.0001.exr
002  ZZ100_50 V     C        01:00:06:13 01:00:08:15 00:59:54:18 00:59:56:20
* FROM CLIP NAME:  take_2
* FROM FILE: S:/path/to/ZZ100_502A.take_2.0101.exr
";
    let document = parse(original, 24.0);
    assert_eq!(write_as(&document, Style::Nucoda), original);
}

/// Appends a clip that names its own reel, as a file read back in would.
///
/// `url` being empty leaves the clip with no media reference at all, which is
/// the case a reel name has to carry on its own.
fn append_reel_clip(
    document: &mut Document,
    track: NodeId,
    clip_name: &str,
    reel: &str,
    url: &str,
    start: f64,
    frames: f64,
) {
    use otio_core::schema::{Base, Clip, ExternalReference, ItemData, MediaReferenceData};
    use otio_core::upgrade::DEFAULT_MEDIA_KEY;

    let mut cmx = AnyDictionary::new();
    cmx.insert("reel".to_string(), Any::String(reel.to_string()));
    let mut metadata = AnyDictionary::new();
    metadata.insert("cmx_3600".to_string(), Any::Dictionary(cmx));

    let mut media_references = std::collections::BTreeMap::new();
    if !url.is_empty() {
        let reference = document.insert(Node::ExternalReference(ExternalReference {
            media: MediaReferenceData::default(),
            target_url: url.to_string(),
        }));
        media_references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);
    }

    let clip = document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base {
                name: clip_name.to_string(),
                metadata,
            },
            source_range: Some(TimeRange::new(
                RationalTime::new(start, 24.0),
                RationalTime::new(frames, 24.0),
            )),
            ..ItemData::new()
        },
        media_references,
        active_media_reference_key: DEFAULT_MEDIA_KEY.to_string(),
    }));
    document
        .append_child(track, clip)
        .expect("a track holds clips");
}

/// Appends a dissolve reaching `into` frames back and `out_of` frames on.
fn append_transition(document: &mut Document, track: NodeId, into: f64, out_of: f64) {
    use otio_core::schema::Transition;

    let transition = document.insert(Node::Transition(Transition {
        in_offset: RationalTime::new(into, 24.0),
        out_offset: RationalTime::new(out_of, 24.0),
        ..Transition::default()
    }));
    document
        .append_child(track, transition)
        .expect("a track holds transitions");
}

#[test]
fn writes_a_dissolve_as_a_restated_event() {
    let (mut document, track) = timeline_with_a_track("Example CrossDissolve", "V");
    append_reel_clip(
        &mut document,
        track,
        "Clip1",
        "Clip1",
        "/var/tmp/clip1.001.exr",
        131.0,
        102.0,
    );
    append_transition(&mut document, track, 57.0, 43.0);
    append_reel_clip(
        &mut document,
        track,
        "Clip2",
        "Clip2",
        "/var/tmp/clip2.001.exr",
        280.0,
        143.0,
    );
    append_reel_clip(
        &mut document,
        track,
        "Clip3",
        "Clip3",
        "/var/tmp/clip3.001.exr",
        0.0,
        24.0,
    );

    assert_eq!(
        write_as(&document, Style::Nucoda),
        "\
TITLE: Example CrossDissolve

001  Clip1    V     C        00:00:05:11 00:00:07:08 00:00:00:00 00:00:01:21
* FROM CLIP NAME:  Clip1
* FROM FILE: /var/tmp/clip1.001.exr
002  Clip1    V     C        00:00:07:08 00:00:07:08 00:00:01:21 00:00:01:21
002  Clip2    V     D 100    00:00:09:07 00:00:17:15 00:00:01:21 00:00:10:05
* FROM CLIP NAME:  Clip1
* FROM FILE: /var/tmp/clip1.001.exr
* TO CLIP NAME:  Clip2
* TO FILE: /var/tmp/clip2.001.exr
003  Clip3    V     C        00:00:00:00 00:00:01:00 00:00:10:05 00:00:11:05
* FROM CLIP NAME:  Clip3
* FROM FILE: /var/tmp/clip3.001.exr
"
    );
}

#[test]
fn writes_a_fade_in_as_a_dissolve_from_black() {
    let (mut document, track) = timeline_with_a_track("Example Fade In", "V");
    append_transition(&mut document, track, 0.0, 12.0);
    append_reel_clip(
        &mut document,
        track,
        "My Clip",
        "My_Clip",
        "/var/tmp/clip.001.exr",
        50.0,
        26.0,
    );

    assert_eq!(
        write_as(&document, Style::Nucoda),
        "\
TITLE: Example Fade In

001  BL       V     C        00:00:00:00 00:00:00:00 00:00:00:00 00:00:00:00
001  My_Clip  V     D 012    00:00:02:02 00:00:03:04 00:00:00:00 00:00:01:02
* TO CLIP NAME:  My Clip
* TO FILE: /var/tmp/clip.001.exr
"
    );
}

#[test]
fn writes_a_fade_out_as_a_dissolve_to_black() {
    let (mut document, track) = timeline_with_a_track("Example Fade Out", "V");
    append_reel_clip(
        &mut document,
        track,
        "My Clip",
        "My_Clip",
        "/var/tmp/clip.001.exr",
        24.0,
        24.0,
    );
    append_transition(&mut document, track, 12.0, 0.0);

    assert_eq!(
        write_as(&document, Style::Nucoda),
        "\
TITLE: Example Fade Out

001  My_Clip  V     C        00:00:01:00 00:00:01:12 00:00:00:00 00:00:00:12
* FROM CLIP NAME:  My Clip
* FROM FILE: /var/tmp/clip.001.exr
002  My_Clip  V     C        00:00:01:12 00:00:01:12 00:00:00:12 00:00:00:12
002  BL       V     D 012    00:00:00:00 00:00:00:12 00:00:00:12 00:00:01:00
* FROM CLIP NAME:  My Clip
* FROM FILE: /var/tmp/clip.001.exr
"
    );
}

#[test]
fn writes_two_dissolves_running_back_to_back() {
    let (mut document, track) = timeline_with_a_track("Double Transition", "V");
    append_reel_clip(&mut document, track, "", "Reel1", "", 24.0, 24.0);
    append_transition(&mut document, track, 6.0, 6.0);
    append_reel_clip(&mut document, track, "", "Reel2", "", 24.0, 24.0);
    append_transition(&mut document, track, 6.0, 6.0);
    append_reel_clip(&mut document, track, "", "Reel3", "", 24.0, 24.0);

    assert_eq!(
        write_as(&document, Style::Nucoda),
        "\
TITLE: Double Transition

001  Reel1    V     C        00:00:01:00 00:00:01:18 00:00:00:00 00:00:00:18
002  Reel1    V     C        00:00:01:18 00:00:01:18 00:00:00:18 00:00:00:18
002  Reel2    V     D 012    00:00:00:18 00:00:01:18 00:00:00:18 00:00:01:18
003  Reel2    V     C        00:00:01:18 00:00:01:18 00:00:01:18 00:00:01:18
003  Reel3    V     D 012    00:00:00:18 00:00:02:00 00:00:01:18 00:00:03:00
"
    );
}

#[test]
fn reads_a_file_that_targets_two_audio_tracks() {
    let document = read("multi_audio.edl");
    let audio: Vec<NodeId> = tracks(&document)
        .into_iter()
        .filter(|id| {
            matches!(
                document.try_get(*id).expect("a live track"),
                Node::Track(track) if track.kind == "Audio"
            )
        })
        .collect();

    assert_eq!(audio.len(), 2);
    assert_eq!(name(&document, audio[0]), "A1");
    assert_eq!(name(&document, audio[1]), "A2");
}

#[test]
fn a_clip_can_name_its_own_reel() {
    let (mut document, track) = timeline_with_a_track("", "V");
    append_reel_clip(&mut document, track, "", "v330_21f", "", 1.0, 24.0);

    assert_eq!(
        write_as(&document, Style::Nucoda),
        "001  v330_21f V     C        00:00:00:01 00:00:01:01 00:00:00:00 00:00:01:00\n"
    );
}

#[test]
fn an_unknown_dialect_cannot_be_asked_for() {
    // Upstream takes the dialect as a string and raises at write time for one
    // it does not know. Here the dialect is an enum, so the same mistake is
    // refused where a name from outside the library is read.
    use std::str::FromStr as _;
    let error = Style::from_str("bogus").expect_err("an unknown dialect");
    assert!(error.to_string().contains("bogus"));
}

#[test]
fn record_timecode_that_does_not_add_up_is_refused_unless_asked_to_ignore_it() {
    let refused = Cmx3600::read_from_str(
        &sample("timecode_mismatch.edl"),
        &ReadOptions {
            rate: 25.0,
            ignore_timecode_mismatch: false,
        },
    )
    .expect_err("a file whose record timecode does not add up");
    assert!(
        refused.to_string().contains("record")
            || refused.to_string().contains("timecode")
            || refused.to_string().contains("gap"),
        "unexpected message: {refused}"
    );

    let document = Cmx3600::read_from_str(
        &sample("timecode_mismatch.edl"),
        &ReadOptions {
            rate: 25.0,
            ignore_timecode_mismatch: true,
        },
    )
    .expect("reads once told to believe the source timecode");

    let fourth = item(&document, 0, 3);
    let range = document.range_in_parent(fourth).expect("a placed item");
    assert_eq!(range.start_time(), tc("00:00:17:22", 25.0));
    assert_eq!(range.duration(), tc("00:00:01:24", 25.0));
}

#[test]
fn frame_numbers_read_where_timecode_belongs() {
    let document = parse(
        "1 CLPA V C     113 170 0 57\n\
         2 CLPA V C     170 170 57 57\n\
         2 CLPB V D 027 162 189 57 84\n\
         3 CLPB V C     189 381 84 276\n",
        24.0,
    );

    let root = document.root().expect("a timeline");
    let Node::Timeline(timeline) = document.try_get(root).expect("a live timeline") else {
        panic!("a timeline");
    };
    let stack = timeline.tracks.expect("a timeline has a stack");
    assert_eq!(duration(&document, stack).value(), 276.0);

    let items = items(&document, 0);
    assert_eq!(items.len(), 4);
    assert_eq!(duration(&document, items[0]).value(), 57.0);
    assert_eq!(visible_frames(&document, items[0]), 57.0 + 27.0);

    let dissolve = transition(&document, items[1]);
    assert_eq!(dissolve.in_offset.value(), 0.0);
    assert_eq!(dissolve.out_offset.value(), 27.0);

    assert_eq!(duration(&document, items[2]).value(), 27.0);
    assert_eq!(duration(&document, items[3]).value(), 276.0 - 84.0);
}

#[test]
fn reads_a_transition_stated_over_three_events() {
    let document = read("dissolve_test_4.edl");
    let items = items(&document, 0);
    assert_eq!(items.len(), 8);

    assert_eq!(duration(&document, items[0]).value(), 30.0);
    assert_eq!(duration(&document, items[1]).value(), 51.0);
    assert_eq!(visible_frames(&document, items[1]), 51.0 + 35.0);

    transition(&document, items[2]);
    assert_eq!(duration(&document, items[2]).value(), 35.0);

    assert_eq!(duration(&document, items[3]).value(), 81.0);
    assert_eq!(visible_frames(&document, items[3]), 81.0 + 64.0);

    transition(&document, items[4]);
    assert_eq!(duration(&document, items[4]).value(), 64.0);

    assert_eq!(duration(&document, items[5]).value(), 84.0);
    assert_eq!(visible_frames(&document, items[5]), 84.0);
    assert_eq!(duration(&document, items[6]).value(), 96.0);
    assert_eq!(duration(&document, items[7]).value(), 135.0);
}

#[test]
fn a_transition_stated_on_the_wrong_event_is_refused() {
    let error = Cmx3600::read_from_str(&sample("dissolve_test_fail.edl"), &ReadOptions::default())
        .expect_err("a transition whose event id does not match");
    assert!(
        error
            .to_string()
            .contains("transition and event id mismatch"),
        "unexpected message: {error}"
    );
}

#[test]
fn reads_a_transition_whose_duration_is_stated_in_frames() {
    let document = read("transition_duration.edl");
    let items = items(&document, 0);
    assert_eq!(items.len(), 5);

    transition(&document, items[2]);
    assert_eq!(duration(&document, items[2]).value(), 26.0);
}

#[test]
fn reads_the_speed_changes_in_a_long_file() {
    let document = read("speed_effects.edl");

    let root = document.root().expect("a timeline");
    let Node::Timeline(timeline) = document.try_get(root).expect("a live timeline") else {
        panic!("a timeline");
    };
    let stack = timeline.tracks.expect("a timeline has a stack");
    assert_eq!(duration(&document, stack), tc("00:21:03:18", 24.0));

    // A freeze frame.
    let frozen = item(&document, 0, 182);
    assert_eq!(name(&document, frozen), "Z682_156 (LAY3)");
    let on_frozen = effects(&document, frozen);
    assert_eq!(on_frozen.len(), 1);
    assert!(matches!(
        document.try_get(on_frozen[0]).expect("a live effect"),
        Node::FreezeFrame { .. }
    ));
    assert_eq!(duration(&document, frozen), tc("00:00:00:17", 24.0));
    assert_eq!(
        document.range_in_parent(frozen).expect("a placed clip"),
        TimeRange::new(tc("00:08:30:00", 24.0), tc("00:00:00:17", 24.0))
    );

    // A speed ramp, stated as an M2 comment.
    let ramped = item(&document, 0, 281);
    assert_eq!(name(&document, ramped), "Z686_5A (LAY2) (47.56 FPS)");
    let on_ramped = effects(&document, ramped);
    assert_eq!(on_ramped.len(), 1);
    let Node::LinearTimeWarp { time_scalar, .. } =
        document.try_get(on_ramped[0]).expect("a live effect")
    else {
        panic!("a linear time warp");
    };
    assert!(
        (time_scalar - 1.983_333_33).abs() < 0.000_000_1,
        "time scalar was {time_scalar}"
    );

    // The comment is consumed rather than kept, since the effect now says it.
    assert_eq!(cmx_string(&document, ramped, "motion"), None);
    assert_eq!(duration(&document, ramped), tc("00:00:01:12", 24.0));
    assert_eq!(
        document.range_in_parent(ramped).expect("a placed clip"),
        TimeRange::new(tc("00:11:31:16", 24.0), tc("00:00:01:12", 24.0))
    );
}

#[test]
fn a_bracketed_frame_range_reads_as_an_image_sequence() {
    let sequence = "\
TITLE: Image Sequence Write

001  myimages V     C        01:00:01:00 01:00:02:12 00:00:00:00 00:00:01:12
* FROM CLIP NAME:  my_image_sequence
* FROM CLIP: /media/path/my_image_sequence.[1025-1060].ext
* OTIO TRUNCATED REEL NAME FROM: my_image_sequence.[1025-1060].ext
";
    let document = parse(sequence, 24.0);
    let clip = item(&document, 0, 0);
    let Node::ImageSequenceReference(reference) = document
        .try_get(media_reference(&document, clip))
        .expect("a live reference")
    else {
        panic!("an image sequence reference");
    };
    assert_eq!(reference.start_frame, 1025);
    assert_eq!(reference.start_frame + reference.frame_step * 35, 1060);
    assert_eq!(
        document.available_range(clip).expect("an available range"),
        TimeRange::range_from_start_end_time(tc("01:00:01:00", 24.0), tc("01:00:02:12", 24.0))
    );

    // A bare frame number is a file, not a sequence, and so is a range of one.
    for path in [
        "/media/path/my_image_file.1025.ext",
        "/media/path/my_image_file.[1025].ext",
    ] {
        let one_file = format!(
            "\
TITLE: Image Sequence Write

001  myimages V     C        01:00:01:00 01:00:02:12 00:00:00:00 00:00:01:12
* FROM CLIP NAME:  my_image_sequence
* FROM CLIP: {path}
"
        );
        let document = parse(&one_file, 24.0);
        let clip = item(&document, 0, 0);
        assert!(
            matches!(
                document
                    .try_get(media_reference(&document, clip))
                    .expect("a live reference"),
                Node::ExternalReference(_)
            ),
            "for {path}"
        );
    }
}

#[test]
fn an_image_sequence_writes_the_frames_the_clip_uses() {
    use otio_core::schema::{
        Base, Clip, ImageSequenceReference, ItemData, MediaReferenceData, Stack, Timeline, Track,
    };
    use otio_core::upgrade::DEFAULT_MEDIA_KEY;

    let mut document = Document::new();
    let track = document.insert(Node::Track(Track {
        item: ItemData {
            base: Base {
                name: "V1".to_string(),
                metadata: AnyDictionary::new(),
            },
            ..ItemData::new()
        },
        children: Vec::new(),
        kind: "Video".to_string(),
    }));
    let tracks = document.insert(Node::Stack(Stack::default()));
    document
        .append_child(tracks, track)
        .expect("a stack holds tracks");
    let timeline = document.insert(Node::Timeline(Timeline {
        base: Base {
            name: "Image Sequence Write".to_string(),
            metadata: AnyDictionary::new(),
        },
        tracks: Some(tracks),
        global_start_time: None,
    }));
    document.set_root(Some(timeline));

    let reference = document.insert(Node::ImageSequenceReference(ImageSequenceReference {
        media: MediaReferenceData {
            available_range: Some(TimeRange::range_from_start_end_time(
                tc("01:00:00:00", 24.0),
                tc("01:00:03:00", 24.0),
            )),
            ..MediaReferenceData::default()
        },
        target_url_base: "/media/path/".to_string(),
        name_prefix: "my_image_sequence.".to_string(),
        name_suffix: ".ext".to_string(),
        start_frame: 1001,
        frame_step: 1,
        rate: 24.0,
        frame_zero_padding: 4,
        missing_frame_policy: otio_core::schema::MissingFramePolicy::default(),
    }));
    let mut media_references = std::collections::BTreeMap::new();
    media_references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);

    let clip = document.insert(Node::Clip(Clip {
        item: ItemData {
            base: Base {
                name: "my_image_sequence".to_string(),
                metadata: AnyDictionary::new(),
            },
            source_range: Some(TimeRange::range_from_start_end_time(
                tc("01:00:01:00", 24.0),
                tc("01:00:02:12", 24.0),
            )),
            ..ItemData::new()
        },
        media_references,
        active_media_reference_key: DEFAULT_MEDIA_KEY.to_string(),
    }));
    document
        .append_child(track, clip)
        .expect("a track holds clips");

    let at_24 = WriteOptions {
        rate: Some(24.0),
        ..WriteOptions::default()
    };
    assert_eq!(
        Cmx3600::write_to_string(&document, &at_24).expect("writes"),
        "\
TITLE: Image Sequence Write

001  myimages V     C        01:00:01:00 01:00:02:12 00:00:00:00 00:00:01:12
* FROM CLIP NAME:  my_image_sequence
* FROM CLIP: /media/path/my_image_sequence.[1025-1060].ext
* OTIO TRUNCATED REEL NAME FROM: my_image_sequence.[1025-1060].ext
"
    );

    // With no length asked for, only the extension comes off the reel name.
    let whole = WriteOptions {
        rate: Some(24.0),
        reelname_len: None,
        ..WriteOptions::default()
    };
    assert_eq!(
        Cmx3600::write_to_string(&document, &whole).expect("writes"),
        "\
TITLE: Image Sequence Write

001  my_image_sequence.[1025-1060] V     C        01:00:01:00 01:00:02:12 00:00:00:00 00:00:01:12
* FROM CLIP NAME:  my_image_sequence
* FROM CLIP: /media/path/my_image_sequence.[1025-1060].ext
"
    );
}

#[test]
fn a_disabled_track_or_clip_is_left_out() {
    let mut document = otio_core::from_str(&sample("enabled.otio")).expect("reads the OTIO");

    // Two enabled video tracks have no EDL form, so writing is refused.
    let refused = Cmx3600::write_to_string(&document, &WriteOptions::default())
        .expect_err("two video tracks");
    assert!(
        refused.to_string().contains("track"),
        "unexpected message: {refused}"
    );

    let tracks = tracks(&document);
    let Node::Track(second) = document.try_get_mut(tracks[1]).expect("a live track") else {
        panic!("a track");
    };
    second.item.enabled = false;

    assert_eq!(
        write(&document),
        "\
TITLE: enable_test

001  Clip001  V     C        00:00:00:00 00:00:00:03 00:00:00:00 00:00:00:03
* FROM CLIP NAME:  Clip-001
* OTIO TRUNCATED REEL NAME FROM: Clip-001
002  Clip002  V     C        00:00:00:03 00:00:00:06 00:00:00:03 00:00:00:06
* FROM CLIP NAME:  Clip-002
* OTIO TRUNCATED REEL NAME FROM: Clip-002
"
    );

    let first_clip = item(&document, 0, 0);
    document
        .try_get_mut(first_clip)
        .expect("a live clip")
        .item_mut()
        .expect("an item")
        .enabled = false;

    assert_eq!(
        write(&document),
        "\
TITLE: enable_test

001  Clip002  V     C        00:00:00:03 00:00:00:06 00:00:00:03 00:00:00:06
* FROM CLIP NAME:  Clip-002
* OTIO TRUNCATED REEL NAME FROM: Clip-002
"
    );
}

#[test]
fn a_file_written_back_out_reads_as_the_same_timeline() {
    // Upstream compares the two documents' JSON. Comparing the items' kinds
    // and ranges says the same thing without depending on the serializer, and
    // names the item that differs when one does.
    for name in [
        "screening_example.edl",
        "dissolve_test.edl",
        "dissolve_test_2.edl",
        "dissolve_test_3.edl",
        "dissolve_test_4.edl",
        "speed_effects_small.edl",
    ] {
        let original = read(name);
        let written = write(&original);
        let reread = parse(&written, 24.0);

        let before = tracks(&original);
        let after = tracks(&reread);
        assert_eq!(before.len(), after.len(), "track count of {name}");

        for (index, (left_track, right_track)) in before.into_iter().zip(after).enumerate() {
            let left_items = original.children_of(left_track).expect("items");
            let right_items = reread.children_of(right_track).expect("items");
            assert_eq!(
                left_items.len(),
                right_items.len(),
                "item count of track {index} of {name}"
            );

            for (position, (left, right)) in left_items.into_iter().zip(right_items).enumerate() {
                let left_node = original.try_get(left).expect("a live item");
                let right_node = reread.try_get(right).expect("a live item");
                assert_eq!(
                    std::mem::discriminant(left_node),
                    std::mem::discriminant(right_node),
                    "item {position} of track {index} of {name}"
                );

                match (left_node, right_node) {
                    (Node::Transition(left), Node::Transition(right)) => {
                        assert_eq!(left.in_offset, right.in_offset, "item {position} of {name}");
                        assert_eq!(
                            left.out_offset, right.out_offset,
                            "item {position} of {name}"
                        );
                        assert_eq!(
                            left.transition_type, right.transition_type,
                            "item {position} of {name}"
                        );
                    }
                    _ => assert_eq!(
                        left_node.item().and_then(|item| item.source_range),
                        right_node.item().and_then(|item| item.source_range),
                        "item {position} of track {index} of {name}"
                    ),
                }
            }
        }
    }
}

#[test]
fn a_screening_edl_written_back_out_is_not_the_same_bytes() {
    // Upstream says so too: the writer normalizes spacing, comment order and
    // reel padding, so the timeline survives where the text does not.
    let original = sample("screening_example.edl");
    let written = write(&read("screening_example.edl"));
    assert_ne!(written, original);
}

/// Returns the CDL a clip carries, as `otio-adapter` reads it back.
fn cdl(document: &Document, clip: NodeId) -> otio_adapter::cdl::Cdl {
    otio_adapter::cdl::Cdl::from_metadata(
        metadata(document, clip)
            .get("cdl")
            .expect("a graded clip")
            .as_dictionary()
            .expect("a dictionary"),
    )
}

#[test]
fn reads_the_colour_decisions_on_a_clip() {
    let document = read("cdl.edl");
    assert_eq!(tracks(&document).len(), 1);

    let items = items(&document, 0);
    assert_eq!(items.len(), 2);

    for (index, clip) in items.into_iter().enumerate() {
        assert_eq!(name(&document, clip), "ZZ100_501 (LAY3)", "clip {index}");
        assert_eq!(
            document.trimmed_range(clip).expect("a range").duration(),
            tc("00:00:01:07", 24.0),
            "clip {index}"
        );

        let cdl = cdl(&document, clip);
        assert_eq!(cdl.sat, Some(0.9), "clip {index}");
        let sop = cdl.sop.expect("a slope, offset and power");
        assert_eq!(sop.slope, [0.1, 0.2, 0.3], "clip {index}");
        assert_eq!(sop.offset, [1.0, -0.0122, 0.0305], "clip {index}");
        assert_eq!(sop.power, [1.0, 0.0, 1.0], "clip {index}");
    }
}

#[test]
fn reads_colour_decisions_written_with_commas_between_them() {
    // Premiere's CDL master effect writes the triples this way.
    let premiere = "\
TITLE: Sequence 01
FCM: NON-DROP FRAME

000001  A006C014_1701069O V     C        04:34:41:13 04:34:41:16 00:00:00:00 00:00:00:03
* FROM CLIP NAME: A006C014_1701069O_LOG_NO_LUT.mov
* ASC_SOP: (1.1549, 1.1469, 1.1422000000000001)(-0.067799999999999999, -0.055500000000000001, -0.032300000000000002)(1.1325000000000001, 1.1351, 1.1221000000000001)
* ASC_SAT: 1.2988
";
    let document = parse(premiere, 24.0);
    let cdl = cdl(&document, item(&document, 0, 0));

    assert!((cdl.sat.expect("a saturation") - 1.2988).abs() < 1e-12);
    let sop = cdl.sop.expect("a slope, offset and power");
    // Upstream spells these out to seventeen digits, which is how Premiere
    // writes them; every one of those is the same f64 as the short form.
    for (read, expected) in [
        (sop.slope, [1.1549, 1.1469, 1.1422]),
        (sop.offset, [-0.0678, -0.0555, -0.0323]),
        (sop.power, [1.1325, 1.1351, 1.1221]),
    ] {
        for (index, (read, expected)) in read.into_iter().zip(expected).enumerate() {
            assert!(
                (read - expected).abs() < 1e-12,
                "channel {index}: {read} is not {expected}"
            );
        }
    }
}

#[test]
fn colour_decision_comments_survive_a_round_trip() {
    let original = "\
TITLE: Example_Screening.01

001  AX       V     C        01:00:04:05 01:00:05:12 00:00:00:00 00:00:01:07
* FROM CLIP NAME:  ZZ100_501 (LAY3)
*ASC_SOP (0.1 0.2 0.3) (1.0 -0.0122 0.0305) (1.0 0.0 1.0)
*ASC_SAT 0.9
* SOURCE FILE: ZZ100_501.LAY3.01
";
    // The reel name comes from the clip's name once the file's own reel is
    // the placeholder AX, so it is truncated and the original kept in a
    // comment.
    let expected = "\
TITLE: Example_Screening.01

001  ZZ100501 V     C        01:00:04:05 01:00:05:12 00:00:00:00 00:00:01:07
* FROM CLIP NAME:  ZZ100_501 (LAY3)
* OTIO TRUNCATED REEL NAME FROM: ZZ100_501 (LAY3)
*ASC_SOP (0.1 0.2 0.3) (1.0 -0.0122 0.0305) (1.0 0.0 1.0)
*ASC_SAT 0.9
* SOURCE FILE: ZZ100_501.LAY3.01
";
    let document = parse(original, 24.0);
    assert_eq!(write(&document), expected);
}

#[test]
fn each_dialect_reads_the_same_two_clips() {
    for file in [
        "avid_example.edl",
        "nucoda_example.edl",
        "premiere_example.edl",
    ] {
        let document = read(file);
        assert_eq!(tracks(&document).len(), 1, "{file}");

        let items = items(&document, 0);
        assert_eq!(items.len(), 2, "{file}");

        // Premiere's dialect names no media, so the clip takes its name from
        // the file it points at rather than from a FROM CLIP NAME comment.
        for (index, (named, fallback, duration)) in [
            ("take_1", "ZZ100_501.take_1.0001.exr", "00:00:01:07"),
            ("take_2", "ZZ100_502A.take_2.0101.exr", "00:00:02:02"),
        ]
        .into_iter()
        .enumerate()
        {
            let clip = items[index];
            let read_name = name(&document, clip);
            assert!(
                read_name == named || read_name == fallback,
                "{file} clip {index} is named {read_name}"
            );
            assert_eq!(
                document.trimmed_range(clip).expect("a range").duration(),
                tc(duration, 24.0),
                "{file} clip {index}"
            );
        }
    }
}

// Upstream's reader starts from `schema.Timeline()`, which builds an enabled
// stack named `tracks`. Upstream's `test_edl_round_trip_mem2disk2mem`
// compares a timeline built that way with one read back from an EDL, as
// text, so both show.
#[test]
fn the_stack_is_the_one_upstreams_timeline_builds() {
    let document = read("screening_example.edl");
    let root = document.root().unwrap();
    let Node::Timeline(timeline) = document.try_get(root).unwrap() else {
        panic!("an EDL reads as a timeline");
    };
    let stack = document.try_get(timeline.tracks.unwrap()).unwrap();
    assert_eq!(stack.name(), "tracks");
    assert!(stack.item().unwrap().enabled);
}

//! Conformance against upstream's FCP X XML adapter.
//!
//! Every test here is ported from `otio-fcpx-xml-adapter`'s own
//! `tests/test_fcpx_adapter.py`, against the same sample files, so that a
//! failure can be read straight against the test it came from. Where the
//! assertion here is weaker or stronger than upstream's, the comment says why.

use std::path::PathBuf;

use otio_adapter::TextAdapter;
use otio_core::schema::Node;
use otio_core::{Document, NodeId};
use otio_fcpx::FcpxXml;

/// Returns the contents of a vendored sample file.
fn sample(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

fn read(name: &str) -> Document {
    FcpxXml::read_from_str(&sample(name), &Default::default())
        .unwrap_or_else(|error| panic!("reading {name}: {error}"))
}

fn read_str(input: &str) -> Document {
    FcpxXml::read_from_str(input, &Default::default()).expect("a readable document")
}

fn write(document: &Document) -> String {
    FcpxXml::write_to_string(document, &Default::default()).expect("a writable document")
}

fn json(document: &Document) -> String {
    otio_core::to_string_pretty(document, otio_core::DEFAULT_INDENT).expect("a writable document")
}

/// Returns the first timeline in a document, whether it is the root or sits
/// in a collection.
fn first_timeline(document: &Document) -> NodeId {
    let root = document.root().expect("a parsed document has a root");
    if matches!(document.try_get(root), Ok(Node::Timeline(_))) {
        return root;
    }
    *document
        .find_children(root, None, false, &|node| matches!(node, Node::Timeline(_)))
        .expect("a searchable root")
        .first()
        .expect("the document holds a timeline")
}

/// Returns a timeline's tracks, in order.
fn tracks(document: &Document) -> Vec<NodeId> {
    let timeline = first_timeline(document);
    let Ok(Node::Timeline(data)) = document.try_get(timeline) else {
        panic!("expected a Timeline");
    };
    document
        .children_of(data.tracks.expect("the timeline has tracks"))
        .expect("a stack of tracks")
}

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

/// The clip names upstream asserts for each of the three video tracks, in
/// every one of its three round-trip tests. A gap reads as an empty name.
const VIDEO_CLIP_NAMES: [&[&str]; 3] = [
    &[
        "IMG_0715",
        "",
        "compound_clip_1",
        "IMG_0233",
        "IMG_0687",
        "IMG_0268",
        "compound_clip_1",
    ],
    &["", "IMG_0513", "", "IMG_0268", "IMG_0740"],
    &["", "IMG_0857"],
];

/// Upstream's shared body: four tracks, three of them video, with the clip
/// names it lists, and a write and a read that gives back what went in.
fn assert_the_sample_edit(document: &Document) {
    assert_eq!(tracks(document).len(), 4);
    assert_eq!(tracks_of_kind(document, "Video").len(), 3);
    assert_eq!(tracks_of_kind(document, "Audio").len(), 1);

    for (track, expected) in tracks_of_kind(document, "Video")
        .into_iter()
        .zip(VIDEO_CLIP_NAMES)
    {
        assert_eq!(child_names(document, track), expected);
    }

    let written = write(document);
    let again = read_str(&written);
    assert_eq!(
        json(document),
        json(&again),
        "the round trip lost something"
    );
}

// ------------------------------------------------------- round tripping --

/// Upstream's `test_library_roundtrip`.
#[test]
fn a_library_survives_a_write_and_a_read() {
    assert_the_sample_edit(&read("fcpx_library.fcpxml"));
}

/// Upstream's `test_event_roundtrip`.
#[test]
fn an_event_survives_a_write_and_a_read() {
    assert_the_sample_edit(&read("fcpx_event.fcpxml"));
}

/// Upstream's `test_project_roundtrip`.
///
/// A bare project reads as a `Timeline` rather than as a collection, which is
/// the only thing that distinguishes this from the two above.
#[test]
fn a_project_survives_a_write_and_a_read() {
    let document = read("fcpx_project.fcpxml");
    let root = document.root().expect("a parsed document has a root");
    assert!(matches!(document.try_get(root), Ok(Node::Timeline(_))));
    assert_the_sample_edit(&document);
}

/// Upstream's `test_clips_roundtrip`.
///
/// A file of loose clips holds no edit at all, so upstream asserts nothing
/// about its shape beyond the round trip. Checked here as well: the clips and
/// the compound clip are all there, because a round trip that dropped all
/// three would pass on its own.
#[test]
fn a_file_of_loose_clips_survives_a_write_and_a_read() {
    let document = read("fcpx_clips.fcpxml");
    let root = document.root().expect("a parsed document has a root");
    assert_eq!(
        child_names(&document, root),
        ["IMG_0857", "IMG_0858", "compound_clip_1"]
    );

    let written = write(&document);
    let again = read_str(&written);
    assert_eq!(
        json(&document),
        json(&again),
        "the round trip lost something"
    );
}

// ------------------------------------------------------------- the parts --

/// Upstream's `test_format_name`, with the `ffprobe` call it mocks out
/// replaced by the frame size that mock returns.
#[test]
fn a_format_is_named_after_its_frame_size_and_rate() {
    assert_eq!(
        otio_fcpx::format_name(25, "640x360"),
        "FFVideoFormat640x360p25"
    );

    // The two sizes upstream collapses to the number a person would use, and
    // the asymmetry between them: `1920` matches anywhere, `1280` only at the
    // end, so `1280x720` is left alone.
    assert_eq!(
        otio_fcpx::format_name(25, "1920x1080"),
        "FFVideoFormat1080p25"
    );
    assert_eq!(
        otio_fcpx::format_name(30, "720x1280"),
        "FFVideoFormat720p30"
    );
    assert_eq!(
        otio_fcpx::format_name(30, "1280x720"),
        "FFVideoFormat1280x720p30"
    );

    // No frame size means `ffprobe` found nothing, which upstream reports by
    // giving the format no name at all.
    assert_eq!(otio_fcpx::format_name(25, ""), "");
}

/// Markers carry their colour in an attribute that is only there for two of
/// the three colours Final Cut has.
#[test]
fn a_markers_colour_survives_a_write_and_a_read() {
    let document = read("fcpx_event.fcpxml");
    let written = write(&document);

    assert!(
        written.contains(r#"value="Marker 5" completed="0""#),
        "a red marker"
    );
    assert!(
        written.contains(r#"value="Marker 7" completed="1""#),
        "a green marker"
    );
    // Purple is the absence of the attribute rather than a value of it.
    assert!(written.contains(r#"value="Marker 4"/>"#), "a purple marker");
}

/// What Final Cut knows about a piece of media rides along in metadata.
#[test]
fn a_clips_notes_and_keywords_survive_a_write_and_a_read() {
    let document = read("fcpx_event.fcpxml");
    let written = write(&document);

    assert!(written.contains("<note>A simple note</note>"));
    assert!(written.contains(r#"<keyword duration="6240/600s" start="0s" value="snow, truck"/>"#));
    assert!(written.contains(r#"<md key="com.apple.proapps.studio.reel" value="5"/>"#));
}

/// A file that is none of the four things this adapter reads is refused
/// rather than read as an empty document.
#[test]
fn a_file_with_no_edit_in_it_is_refused() {
    let error = FcpxXml::read_from_str(
        r#"<fcpxml version="1.8"><resources/></fcpxml>"#,
        &Default::default(),
    )
    .expect_err("a document with nothing in it");
    assert!(
        error
            .to_string()
            .contains("no library, event, project or clips"),
        "{error}"
    );
}

// --------------------------------------------------------- the deviations --

/// Upstream reads the first event in a library and silently drops the rest.
///
/// This adapter keeps them, because the timelines in the later events are
/// gone otherwise and nothing tells the reader they were there.
#[test]
fn every_event_in_a_library_is_read() {
    let document = read_str(
        r#"<fcpxml version="1.8">
             <resources>
               <format id="r1" frameDuration="100/3000s"/>
               <asset id="r2" name="shot" src="file:///shot.mov"
                      format="r1" start="0s" duration="10s"/>
             </resources>
             <library>
               <event name="first">
                 <project name="one">
                   <sequence format="r1" duration="10s">
                     <spine>
                       <asset-clip name="shot" ref="r2" offset="0s" duration="10s"/>
                     </spine>
                   </sequence>
                 </project>
               </event>
               <event name="second">
                 <project name="two">
                   <sequence format="r1" duration="10s">
                     <spine>
                       <asset-clip name="shot" ref="r2" offset="0s" duration="10s"/>
                     </spine>
                   </sequence>
                 </project>
               </event>
             </library>
           </fcpxml>"#,
    );

    let root = document.root().expect("a parsed document has a root");
    // The collection takes the first event's name, since a file holds one
    // event and that is all a write can put back.
    assert_eq!(document.try_get(root).unwrap().name(), "first");
    assert_eq!(child_names(&document, root), ["one", "two"]);
}

/// Upstream sorts lane numbers as the strings they are, so lane `10` ends up
/// under lane `2` and the picture composites in the wrong order.
#[test]
fn lanes_are_ordered_as_numbers() {
    let document = read_str(
        r#"<fcpxml version="1.8">
             <resources>
               <format id="r1" frameDuration="100/3000s"/>
               <asset id="r2" name="shot" src="file:///shot.mov"
                      format="r1" start="0s" duration="10s"/>
             </resources>
             <project name="ten lanes">
               <sequence format="r1" duration="10s">
                 <spine>
                   <asset-clip name="base" ref="r2" offset="0s" duration="10s">
                     <asset-clip name="two" lane="2" ref="r2" offset="0s" duration="10s"/>
                     <asset-clip name="ten" lane="10" ref="r2" offset="0s" duration="10s"/>
                   </asset-clip>
                 </spine>
               </sequence>
             </project>
           </fcpxml>"#,
    );

    let names: Vec<String> = tracks(&document)
        .into_iter()
        .map(|track| document.try_get(track).unwrap().name().to_string())
        .collect();
    assert_eq!(names, ["0", "2", "10"]);
}

/// Upstream names an unnamed event after today's date, which makes a write
/// irreproducible. Two writes of the same document should agree.
#[test]
fn an_unnamed_event_is_written_unnamed() {
    let document = read("fcpx_clips.fcpxml");
    let written = write(&document);
    assert_eq!(written, write(&document));
}

// ------------------------------------------- timelines from other formats --
//
// Upstream's writer fails on each of these. They are what another adapter's
// reader hands it, so every EDL-to-FCPXML or FCP 7-to-FCPXML conversion that
// took one of these shapes failed. See `crates/otio-capi/tests/conversions.rs`
// for the same conversions from the other adapters' sample files.

/// A time as OTIO JSON.
fn time(value: f64, rate: f64) -> String {
    format!(r#"{{"OTIO_SCHEMA": "RationalTime.1", "value": {value:?}, "rate": {rate:?}}}"#)
}

/// A range as OTIO JSON.
fn range(start: f64, duration: f64, rate: f64) -> String {
    format!(
        r#"{{"OTIO_SCHEMA": "TimeRange.1", "start_time": {}, "duration": {}}}"#,
        time(start, rate),
        time(duration, rate)
    )
}

/// A clip with no media, `frames` long, as OTIO JSON.
fn clip_json(name: &str, frames: f64, rate: f64) -> String {
    format!(
        r#"{{"OTIO_SCHEMA": "Clip.2", "name": "{name}", "source_range": {},
             "media_references": {{"DEFAULT_MEDIA": {{"OTIO_SCHEMA": "MissingReference.1"}}}},
             "active_media_reference_key": "DEFAULT_MEDIA"}}"#,
        range(0.0, frames, rate)
    )
}

fn gap_json(frames: f64, rate: f64) -> String {
    format!(
        r#"{{"OTIO_SCHEMA": "Gap.1", "source_range": {}}}"#,
        range(0.0, frames, rate)
    )
}

/// A track as OTIO JSON, with an optional range of its own.
fn track_json(name: &str, kind: &str, own_range: Option<String>, children: &[String]) -> String {
    let own_range = own_range.unwrap_or_else(|| "null".to_string());
    format!(
        r#"{{"OTIO_SCHEMA": "Track.1", "name": "{name}", "kind": "{kind}",
             "source_range": {own_range}, "children": [{}]}}"#,
        children.join(", ")
    )
}

fn timeline_json(name: &str, tracks: &[String]) -> Document {
    otio_core::from_str(&format!(
        r#"{{"OTIO_SCHEMA": "Timeline.1", "name": "{name}",
             "tracks": {{"OTIO_SCHEMA": "Stack.1", "name": "tracks", "children": [{}]}}}}"#,
        tracks.join(", ")
    ))
    .expect("valid OTIO JSON")
}

/// Each track's children as (name, length in frames), gaps named `""`.
fn layout(document: &Document, track: NodeId) -> Vec<(String, f64)> {
    document
        .children_of(track)
        .expect("a composition")
        .into_iter()
        .map(|child| {
            let name = document.try_get(child).unwrap().name().to_string();
            let frames = document.duration(child).unwrap().value();
            (name, frames)
        })
        .collect()
}

/// The EDL reader, as upstream's, says where a track starts in record time
/// by giving it a range that starts that far *before* zero — so read strictly
/// it trims every clip away. Upstream's writer fails on it: "computed time
/// range would be invalid". Here the offsets are honoured, less the one every
/// track shares, so the sound that starts two seconds after the picture in
/// record time still does.
#[test]
fn an_edl_tracks_record_offset_is_written_as_a_lead_in() {
    const RATE: f64 = 24.0;
    // 01:00:00:00 and 01:00:02:00, as the EDL reader writes them.
    let document = timeline_json(
        "Offsets",
        &[
            track_json(
                "V",
                "Video",
                Some(range(-86_400.0, 24.0, RATE)),
                &[clip_json("picture.mov", 24.0, RATE)],
            ),
            track_json(
                "A1",
                "Audio",
                Some(range(-86_448.0, 24.0, RATE)),
                &[clip_json("sound.wav", 24.0, RATE)],
            ),
        ],
    );

    let result = read_str(&write(&document));
    let video = tracks_of_kind(&result, "Video");
    let audio = tracks_of_kind(&result, "Audio");
    assert_eq!(
        layout(&result, video[0]),
        [("picture.mov".to_string(), 24.0)]
    );
    assert_eq!(
        layout(&result, audio[0]),
        [(String::new(), 48.0), ("sound.wav".to_string(), 24.0)]
    );
}

/// With no video there is no storyline for audio to hang off, and upstream
/// crashes on the missing parent. Final Cut spells an empty storyline as a
/// gap, so one is written, and the sound survives.
#[test]
fn an_audio_only_timeline_hangs_off_a_storyline_gap() {
    const RATE: f64 = 24.0;
    let document = timeline_json(
        "Sound",
        &[
            track_json("A1", "Audio", None, &[clip_json("left.wav", 48.0, RATE)]),
            track_json(
                "A2",
                "Audio",
                None,
                &[gap_json(24.0, RATE), clip_json("right.wav", 24.0, RATE)],
            ),
        ],
    );

    let written = write(&document);
    let result = read_str(&written);
    let audio = tracks_of_kind(&result, "Audio");
    let names: Vec<Vec<String>> = audio
        .iter()
        .map(|&track| child_names(&result, track))
        .collect();
    assert!(
        names.contains(&vec!["left.wav".to_string()]),
        "{names:?}\n{written}"
    );
    assert!(
        names.contains(&vec![String::new(), "right.wav".to_string()]),
        "{names:?}\n{written}"
    );
}

/// A second video track that runs on past the end of the first has nothing
/// under it there. The storyline is lengthened with a gap, as for an audio-only
/// timeline, rather than the write failing.
#[test]
fn a_lane_past_the_storylines_end_hangs_off_a_storyline_gap() {
    const RATE: f64 = 30.0;
    let document = timeline_json(
        "Overhang",
        &[
            track_json("V1", "Video", None, &[clip_json("base", 30.0, RATE)]),
            track_json(
                "V2",
                "Video",
                None,
                &[gap_json(60.0, RATE), clip_json("title", 30.0, RATE)],
            ),
        ],
    );

    let result = read_str(&write(&document));
    let video = tracks_of_kind(&result, "Video");
    // The reader drops a storyline's trailing gap.
    assert_eq!(child_names(&result, video[0]), ["base"]);
    assert_eq!(
        layout(&result, video[1]),
        [(String::new(), 60.0), ("title".to_string(), 30.0)]
    );
}

/// Upstream writes an empty `frameDuration` for any rate missing from its
/// table, and neither Final Cut nor its own reader will take one. A 15 fps
/// timeline — any Premiere export with a 15 fps clip — is the usual way in.
#[test]
fn a_rate_outside_final_cuts_table_still_writes_a_readable_format() {
    const RATE: f64 = 15.0;
    let document = timeline_json(
        "Fifteen",
        &[track_json(
            "V1",
            "Video",
            None,
            &[clip_json("slow", 15.0, RATE)],
        )],
    );

    let written = write(&document);
    assert!(written.contains(r#"frameDuration="1/15s""#), "{written}");
    let result = read_str(&written);
    let video = tracks_of_kind(&result, "Video");
    assert_eq!(layout(&result, video[0]), [("slow".to_string(), 15.0)]);
}

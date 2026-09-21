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

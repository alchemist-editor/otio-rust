//! EXPERIMENTAL multicam prototype (ADR 0014): Avid multicam edits read as
//! `Multicam` and `MulticamClip`, and written back as Avid writes them.
//!
//! The fixture is Trevor Aylward's Avid test edit from
//! <https://github.com/TrevorAyl/otio-multicam-tests> (MIT): 13 cameras and
//! 62 cuts, exported from Media Composer.

use std::path::PathBuf;

use otio_core::multicam;
use otio_core::{Document, Node, NodeId};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/multicam/avid_mcam_edit.aaf")
}

fn read(multicam: bool) -> (Document, NodeId) {
    let options = otio_aaf::ReadOptions::new().with_multicam(multicam);
    let document = otio_aaf::read_from_file_with(fixture(), &options).expect("reads");
    let root = document.root().expect("root");
    assert!(matches!(document.get(root), Some(Node::Timeline(_))));
    (document, root)
}

fn tracks(document: &Document, timeline: NodeId) -> Vec<NodeId> {
    let Some(Node::Timeline(node)) = document.get(timeline) else {
        panic!("not a timeline")
    };
    document.children_of(node.tracks.unwrap()).unwrap()
}

fn cuts(document: &Document, timeline: NodeId) -> Vec<NodeId> {
    let Some(Node::Timeline(node)) = document.get(timeline) else {
        panic!("not a timeline")
    };
    document
        .find_children(
            node.tracks.unwrap(),
            None,
            false,
            &multicam::is_multicam_clip,
        )
        .unwrap()
}

/// (schema, name, source start, duration) for each item on each track,
/// with neighbouring gaps merged, since a flattened cut pads with gaps
/// where the angle runs out.
fn signature(document: &Document, timeline: NodeId) -> Vec<Vec<(String, String, f64, f64)>> {
    tracks(document, timeline)
        .into_iter()
        .map(|track| {
            let mut items: Vec<(String, String, f64, f64)> = Vec::new();
            for item in document.children_of(track).unwrap() {
                let node = document.get(item).unwrap();
                let range = document.trimmed_range(item).unwrap();
                if matches!(node, Node::Gap(_)) {
                    if let Some(last) = items.last_mut().filter(|last| last.0 == "Gap") {
                        last.3 += range.duration().value();
                        continue;
                    }
                    items.push(("Gap".into(), String::new(), 0.0, range.duration().value()));
                    continue;
                }
                items.push((
                    node.schema_name().to_string(),
                    node.name().to_string(),
                    range.start_time().value(),
                    range.duration().value(),
                ));
            }
            items
        })
        .collect()
}

/// Each cut on each track: what it plays, and over what span.
fn cut_list(document: &Document, timeline: NodeId) -> Vec<(usize, String, i64, f64, f64)> {
    let mut found = Vec::new();
    for (index, track) in tracks(document, timeline).into_iter().enumerate() {
        for cut in document
            .find_children(track, None, false, &multicam::is_multicam_clip)
            .unwrap()
        {
            let node = document.get(cut).unwrap();
            let what = multicam::cut(node).unwrap();
            let range = node.item().unwrap().source_range.unwrap();
            found.push((
                index,
                what.angle,
                what.angle_track,
                range.start_time().value(),
                range.duration().value(),
            ));
        }
    }
    found
}

/// Each angle of each multicam, with how many video and audio tracks it has.
fn angles(document: &Document, timeline: NodeId) -> Vec<(String, usize, usize)> {
    let mut found = Vec::new();
    for definition in multicam::multicams(document, timeline)
        .unwrap()
        .into_values()
    {
        for name in multicam::angle_names(document, definition).unwrap() {
            let angle = multicam::angle(document, definition, &name)
                .unwrap()
                .unwrap();
            let count = |kind| multicam::angle_tracks(document, angle, kind).unwrap().len();
            found.push((name, count("Video"), count("Audio")));
        }
    }
    found
}

fn flattened(document: &Document, timeline: NodeId) -> Document {
    let mut flat = document.clone();
    let report = multicam::flatten(&mut flat, timeline).unwrap();
    assert_eq!(report, Vec::<String>::new());
    flat
}

fn write_and_read(document: &Document, options: &otio_aaf::ReadOptions) -> (Document, NodeId) {
    let bytes = otio_aaf::write_to_bytes(document).expect("writes");
    let back = otio_aaf::read_with(std::io::Cursor::new(bytes), options).expect("reads back");
    let root = back.root().expect("root");
    (back, root)
}

#[test]
fn without_the_option_a_multicam_edit_reads_as_upstream_reads_it() {
    let (document, timeline) = read(false);
    assert!(multicam::multicams(&document, timeline).unwrap().is_empty());
    assert!(cuts(&document, timeline).is_empty());
}

#[test]
fn trevors_avid_edit_reads_as_one_multicam_of_thirteen_angles() {
    let (document, timeline) = read(true);
    let definitions = multicam::multicams(&document, timeline).unwrap();
    assert_eq!(definitions.len(), 1);
    let (key, definition) = definitions.into_iter().next().unwrap();
    assert!(key.starts_with("aaf:urn:smpte:umid:"), "{key}");
    assert_eq!(
        document.get(definition).unwrap().name(),
        "Mcam Avid - synced"
    );
    assert_eq!(
        angles(&document, timeline),
        [
            ("A01", 1, 16),
            ("B01", 1, 2),
            ("C01", 1, 1),
            ("D01", 1, 2),
            ("E01", 1, 1),
            ("F01", 1, 2),
            ("G01", 1, 12),
            ("H01", 1, 0),
            ("GF01", 1, 0),
            ("R01", 0, 8),
            ("GK01", 1, 0),
            ("R02", 0, 2),
            ("P01", 1, 2),
        ]
        .map(|(name, video, audio)| (name.to_owned(), video, audio))
    );
    assert_eq!(cuts(&document, timeline).len(), 62);
    assert_eq!(
        multicam::validate(&document, timeline).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn flattening_the_multicam_read_gives_the_ordinary_read() {
    let (document, timeline) = read(true);
    let (plain, plain_timeline) = read(false);
    assert_eq!(
        signature(&flattened(&document, timeline), timeline),
        signature(&plain, plain_timeline)
    );
}

#[test]
fn a_multicam_edit_survives_writing_and_reading_back() {
    let (document, timeline) = read(true);
    let multicam_read = otio_aaf::ReadOptions::new().with_multicam(true);
    let (back, back_timeline) = write_and_read(&document, &multicam_read);

    assert_eq!(angles(&back, back_timeline), angles(&document, timeline));
    assert_eq!(
        cut_list(&back, back_timeline),
        cut_list(&document, timeline)
    );
    let definition = *multicam::multicams(&back, back_timeline)
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(back.get(definition).unwrap().name(), "Mcam Avid - synced");
    assert_eq!(
        multicam::validate(&back, back_timeline).unwrap(),
        Vec::<String>::new()
    );

    // What the cuts play is what the same edit, written without multicam,
    // plays: the writer loses the same things either way (a stack's length
    // is its inputs' summed, as upstream writes it), and nothing more.
    let (plain, _) = read(false);
    let (plain_back, plain_back_timeline) = write_and_read(&plain, &otio_aaf::ReadOptions::new());
    let expected = signature(&plain_back, plain_back_timeline);
    assert_eq!(
        signature(&flattened(&back, back_timeline), back_timeline),
        expected
    );

    // And a reader that knows nothing of multicam reads the selectors'
    // choices, the flattened edit.
    let (unaware, unaware_timeline) = write_and_read(&document, &otio_aaf::ReadOptions::new());
    assert!(cuts(&unaware, unaware_timeline).is_empty());
    assert_eq!(signature(&unaware, unaware_timeline), expected);
}

#[test]
fn a_cut_into_an_angle_track_that_is_not_there_is_refused() {
    let (mut document, timeline) = read(true);
    let cut = cuts(&document, timeline)[0];
    let mut what = multicam::cut(document.get(cut).unwrap()).unwrap();
    what.angle_track = 99;
    multicam::set_cut(&mut document, cut, &what).unwrap();
    let error = otio_aaf::write_to_bytes(&document).unwrap_err().to_string();
    assert!(error.contains("does not have"), "{error}");
}

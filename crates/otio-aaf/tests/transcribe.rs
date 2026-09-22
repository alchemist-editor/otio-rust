//! Reading an AAF as OTIO, checked against upstream's own Python adapter.
//!
//! The baselines here were produced by `otio-aaf-adapter` reading the same
//! files and writing the result as OTIO JSON, so matching one means this crate
//! and the adapter it is a port of agree on the whole timeline: its shape, its
//! times, its media and everything either of them kept as metadata.
//!
//! Each file has two. `*.structural.otio.json` was read with `simplify=False`
//! and `attach_markers=False`, which is the transcription with only the one
//! pass upstream always runs, so a mismatch there is in the mapping. The other
//! was read with upstream's defaults, which is what a caller gets.

use std::path::{Path, PathBuf};

/// Every node reachable from the document's root, in the order met.
///
/// Not the same as everything in the arena. A mob named by two clips is
/// transcribed once and copied per use, so the arena also holds the originals
/// those copies came from, which are not part of the timeline.
fn reachable(document: &otio_core::Document) -> Vec<otio_core::NodeId> {
    fn walk(
        document: &otio_core::Document,
        id: otio_core::NodeId,
        out: &mut Vec<otio_core::NodeId>,
    ) {
        out.push(id);
        let Some(node) = document.get(id) else { return };
        let mut next: Vec<otio_core::NodeId> = node.children().unwrap_or_default().to_vec();
        if let otio_core::Node::Timeline(timeline) = node {
            next.extend(timeline.tracks);
        }
        if let otio_core::Node::Clip(clip) = node {
            next.extend(clip.media_references.values().copied());
        }
        if let Some(item) = node.item() {
            next.extend(item.effects.iter().copied());
            next.extend(item.markers.iter().copied());
        }
        for child in next {
            walk(document, child, out);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = document.root() {
        walk(document, root, &mut out);
    }
    out
}

/// Every node reachable from one object.
fn reachable_from(document: &otio_core::Document, id: otio_core::NodeId) -> Vec<otio_core::NodeId> {
    let mut inner = document.clone();
    inner.set_root(Some(id));
    reachable(&inner)
}

/// The baselines, which belong to this crate.
fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

/// An AAF file: one of upstream's samples vendored here, or one of the two
/// from pyaaf2's test suite, which the `aaf` crate vendors already.
fn fixture(name: &str) -> PathBuf {
    let here = data_dir().join(name);
    if here.exists() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../aaf/tests/data")
        .join(name)
}

/// Every AAF file with baselines, by name without the suffix.
fn fixtures() -> Vec<String> {
    let mut names = vec!["empty".to_owned(), "sector_size_512".to_owned()];
    let mut vendored: Vec<String> = std::fs::read_dir(data_dir())
        .expect("the data directory is readable")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".aaf").map(str::to_owned)
        })
        .collect();
    vendored.sort();
    names.extend(vendored);
    names
}

/// The options a structural read uses.
fn structural() -> otio_aaf::ReadOptions {
    otio_aaf::ReadOptions::structural()
}

/// Reads a fixture structurally.
fn read_structural(name: &str) -> otio_core::Document {
    otio_aaf::read_from_file_with(fixture(name), &structural()).expect("the fixture transcribes")
}

/// Reads a fixture and writes it back out as OTIO JSON.
fn transcribe(fixture_name: &str, options: &otio_aaf::ReadOptions) -> String {
    let document =
        otio_aaf::read_from_file_with(fixture(fixture_name), options).expect("the fixture reads");
    otio_core::to_string_pretty(&document, otio_core::DEFAULT_INDENT).expect("it serializes")
}

/// Every fixture, read both ways, against what upstream writes for it.
///
/// Byte for byte, not object by object. A comparison that parsed both sides
/// first would pass while writing `2.4e5` where upstream writes `240000.0`,
/// and a file this library wrote would not be the file upstream wrote.
#[test]
fn matches_upstreams_adapter_on_the_whole_timeline() {
    let modes = [
        (".structural.otio.json", structural()),
        (".otio.json", otio_aaf::ReadOptions::default()),
    ];
    let names = fixtures();
    assert!(names.len() >= 12, "only {} fixtures found", names.len());
    for name in &names {
        for (suffix, options) in &modes {
            let fixture = format!("{name}.aaf");
            let baseline = format!("{name}{suffix}");
            let expected = std::fs::read_to_string(data_dir().join(&baseline))
                .expect("the baseline is readable");
            let found = transcribe(&fixture, options);
            let fixture = &baseline;

            // A whole-file assertion on 88 KB prints 88 KB on failure, so the
            // first line that differs is named first and the rest follows.
            if let Some((line, want, got)) = expected
                .lines()
                .zip(found.lines())
                .enumerate()
                .find(|(_, (want, got))| want != got)
                .map(|(number, (want, got))| (number + 1, want, got))
            {
                panic!("{fixture}: line {line} differs\n  upstream: {want}\n  ours    : {got}");
            }
            assert_eq!(
                expected.lines().count(),
                found.lines().count(),
                "{fixture}: the two differ in length"
            );
        }
    }
}

/// A file holding nothing reads as a collection holding nothing.
///
/// The path down to the content storage has to work and then come back empty,
/// which is a different thing from failing. The name is upstream's, and
/// `LIST_NAME` in the crate explains where it comes from.
#[test]
fn an_empty_file_reads_as_an_empty_collection() {
    let document = read_structural("empty.aaf");
    let root = document.root().expect("a transcribed file has a root");
    let node = document.try_get(root).expect("the root is in the document");

    assert_eq!(node.schema_name(), "SerializableCollection");
    assert_eq!(node.name(), "list");
    assert_eq!(node.children().map(<[_]>::len), Some(0));
}

/// The shape of the edit `sector_size_512.aaf` describes.
///
/// One composition, two audio tracks and a timecode track. Each audio track
/// alternates fillers with effects, and each effect holds the clip it applies
/// to. Asserting this alongside the baseline says in words what the baseline
/// says in 88 KB of JSON.
#[test]
fn the_composition_reads_as_two_audio_tracks_and_a_timecode_track() {
    let document = read_structural("sector_size_512.aaf");
    let root = document.root().expect("a transcribed file has a root");

    let children = document
        .try_get(root)
        .expect("the root is in the document")
        .children()
        .expect("a collection has children")
        .to_vec();
    assert_eq!(children.len(), 1, "one top-level composition");

    let timeline = document
        .try_get(children[0])
        .expect("it is in the document");
    assert_eq!(timeline.schema_name(), "Timeline");
    assert_eq!(timeline.name(), "aaf_2trks_4clips");

    let otio_core::Node::Timeline(timeline) = timeline else {
        panic!("it is a timeline");
    };
    // The composition has no start of its own. It does carry a timecode
    // track, but only a timecode on physical track 1 means the edit's start,
    // and this file numbers none of the composition's slots. Upstream reads
    // it the same way and leaves the timeline unstarted.
    assert_eq!(timeline.global_start_time, None);

    let stack = timeline.tracks.expect("a timeline has tracks");
    let tracks = document
        .try_get(stack)
        .expect("it is in the document")
        .children()
        .expect("a stack has children")
        .to_vec();

    let kinds: Vec<(String, String)> = tracks
        .iter()
        .map(|id| {
            let otio_core::Node::Track(track) = document.try_get(*id).expect("it is there") else {
                panic!("a timeline's tracks are tracks");
            };
            (track.item.base.name.clone(), track.kind.clone())
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("Track1".to_owned(), "Audio".to_owned()),
            ("Track2".to_owned(), "Audio".to_owned()),
            // OTIO names two kinds of track and AAF names more, so the ones
            // with no OTIO name keep AAF's rather than being forced into one.
            ("Timecode".to_owned(), "AAF_Timecode".to_owned()),
        ]
    );
}

/// A clip carries the media behind it, and says what it could not find.
///
/// Every clip in this file resolves to a WAVE file that was next to the
/// session, and to the Pro Tools session itself, which has no file of its own
/// and so becomes a missing reference rather than being left out.
#[test]
fn a_clip_carries_the_media_the_chain_of_mobs_leads_to() {
    let document = read_structural("sector_size_512.aaf");

    let clips: Vec<&otio_core::schema::Clip> = reachable(&document)
        .into_iter()
        .filter_map(|id| match document.get(id) {
            Some(otio_core::Node::Clip(clip)) => Some(clip),
            _ => None,
        })
        .collect();
    assert_eq!(clips.len(), 4, "four clips in the timeline");

    let clip = clips
        .iter()
        .find(|clip| clip.item.base.name == "clip1_trk1")
        .expect("the file has that clip");

    assert_eq!(clip.active_media_reference_key, "DEFAULT_MEDIA");
    assert_eq!(clip.media_references.len(), 2);

    let active = clip.media_references[&clip.active_media_reference_key];
    let otio_core::Node::ExternalReference(reference) =
        document.try_get(active).expect("it is in the document")
    else {
        panic!("the active reference is a file");
    };
    assert!(
        reference.target_url.starts_with("file://"),
        "{}",
        reference.target_url
    );
    assert!(
        reference.target_url.ends_with(".wav"),
        "{}",
        reference.target_url
    );

    // The other is the session the clip came out of, which is described in
    // the file and has no media of its own.
    let other = clip
        .media_references
        .iter()
        .find(|(key, _)| *key != &clip.active_media_reference_key)
        .map(|(_, id)| *id)
        .expect("there is a second reference");
    let node = document.try_get(other).expect("it is in the document");
    assert_eq!(node.schema_name(), "MissingReference");
    assert_eq!(node.name(), "Pro Tools:aaf_2trks_4clips.ptx");
}

/// What OTIO has no field for is kept under the object's `AAF` metadata.
#[test]
fn every_object_keeps_the_aaf_object_it_came_from() {
    let document = read_structural("sector_size_512.aaf");

    let root = document.root().expect("a transcribed file has a root");
    let mut named = 0;
    for id in reachable(&document) {
        // The collection at the root stands for no AAF object at all, so it
        // has a name and nothing else. The crate's `LIST_NAME` says why.
        if id == root {
            continue;
        }
        let Some(node) = document.get(id) else {
            continue;
        };
        let Some(base) = node.base() else { continue };
        let Some(otio_core::Any::Dictionary(aaf)) = base.metadata.get("AAF") else {
            continue;
        };
        // A stack that stands for an operation group carries no metadata of
        // its own: the group's went onto the effect, which is the thing the
        // group actually described. Upstream empties it for the same reason.
        if aaf.is_empty() {
            continue;
        }
        assert!(
            aaf.contains_key("ClassName"),
            "{} kept an AAF dictionary naming no class",
            node.schema_name()
        );
        named += 1;
    }
    // Most of the timeline, rather than a handful of objects that happened to
    // carry something.
    assert!(named >= 20, "only {named} objects kept their AAF object");
}

/// A file of one composition reads as that composition's timeline.
///
/// Transcription gives a collection of every mob worth showing, and
/// simplifying a collection of one gives the one, as upstream does.
#[test]
fn a_file_of_one_composition_reads_as_its_timeline() {
    let document =
        otio_aaf::read_from_file(fixture("sector_size_512.aaf")).expect("the fixture reads");
    let root = document.root().expect("a read file has a root");
    let node = document.try_get(root).expect("the root is in the document");
    assert_eq!(node.schema_name(), "Timeline");
    assert_eq!(node.name(), "aaf_2trks_4clips");

    let structural = read_structural("sector_size_512.aaf");
    let root = structural.root().expect("a read file has a root");
    let node = structural
        .try_get(root)
        .expect("the root is in the document");
    assert_eq!(node.schema_name(), "SerializableCollection");
}

/// A marker pointing at a track the file does not have goes on the stack.
///
/// Avid exported this file with one track left out, and numbered the marker
/// against the tracks as they were before. Upstream puts such a marker on the
/// timeline's stack rather than dropping it or guessing a track.
#[test]
fn a_marker_on_a_track_that_is_not_there_goes_on_the_stack() {
    let document = otio_aaf::read_from_file(fixture("bad_marker_track_from_avid.aaf"))
        .expect("the fixture reads");
    let root = document.root().expect("a read file has a root");
    let Some(otio_core::Node::Timeline(timeline)) = document.get(root) else {
        panic!("the file reads as a timeline");
    };
    let stack = timeline.tracks.expect("a timeline has tracks");
    let markers = &document
        .try_get(stack)
        .expect("it is in the document")
        .item()
        .expect("a stack is an item")
        .markers;
    assert_eq!(markers.len(), 1);
    let marker = document.try_get(markers[0]).expect("it is in the document");
    assert!(
        marker.name().starts_with("Marker on Track 3!"),
        "{}",
        marker.name()
    );
}

/// Reading leaves nothing in the document the timeline does not reach.
///
/// Transcription copies a mob per use and simplifying empties containers
/// into their parents, and neither is part of what was read.
#[test]
fn reading_leaves_nothing_behind_in_the_document() {
    for name in ["sector_size_512", "nesting_test", "misc_speed_effects"] {
        for options in [structural(), otio_aaf::ReadOptions::default()] {
            let document = otio_aaf::read_from_file_with(fixture(&format!("{name}.aaf")), &options)
                .expect("the fixture reads");
            let mut reached = reachable(&document);
            // Metadata holds whole objects too: what an effect renders to.
            let mut held = Vec::new();
            for id in &reached {
                if let Some(node) = document.get(*id) {
                    let mut node = node.clone();
                    node.visit_held_objects_mut(&mut |id| held.push(*id));
                }
            }
            for id in held {
                reached.extend(reachable_from(&document, id));
            }
            reached.sort_unstable();
            reached.dedup();
            assert_eq!(reached.len(), document.len(), "{name}");
        }
    }
}

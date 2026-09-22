//! Upstream's `tests/test_bundle.cpp`, ported.
//!
//! Each test builds the timeline upstream's does, writes it as a bundle and
//! reads it back. The timelines are written as OTIO JSON here rather than
//! assembled call by call, which says the same thing in fewer lines.

use std::fs;
use std::path::{Path, PathBuf};

use otio_bundle::{
    MEDIA_DIR, MediaReferencePolicy, ReadOptions, TIMELINE_FILE, VERSION, VERSION_FILE,
    WriteOptions, dry_run, file_from_url, read_otiod, read_otioz, write_otiod, write_otioz,
};
use otio_core::{Document, Node, NodeId};

/// The path a URL names, as a string, for the tests to build paths from.
fn path_of(url: &str) -> Option<String> {
    file_from_url(url)
        .unwrap()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

/// A directory that is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "otio-bundle-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn range(duration: f64, rate: f64) -> String {
    format!(
        r#"{{"OTIO_SCHEMA": "TimeRange.1",
            "start_time": {{"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": {rate}}},
            "duration": {{"OTIO_SCHEMA": "RationalTime.1", "value": {duration}, "rate": {rate}}}}}"#
    )
}

fn external(url: &str) -> String {
    format!(r#"{{"OTIO_SCHEMA": "ExternalReference.1", "target_url": {url:?}}}"#)
}

fn missing() -> String {
    r#"{"OTIO_SCHEMA": "MissingReference.1"}"#.to_string()
}

fn sequence(images: u32) -> String {
    format!(
        r#"{{"OTIO_SCHEMA": "ImageSequenceReference.1",
            "available_range": {},
            "target_url_base": "", "name_prefix": "render.", "name_suffix": ".exr",
            "start_frame": 0, "frame_step": 1, "rate": 24.0, "frame_zero_padding": 0,
            "missing_frame_policy": "error"}}"#,
        range(f64::from(images), 24.0)
    )
}

fn generator() -> String {
    format!(
        r#"{{"OTIO_SCHEMA": "GeneratorReference.1", "name": "gradient",
            "generator_kind": "gradient", "available_range": {},
            "parameters": {{}}, "metadata": {{"meta": "data"}}}}"#,
        range(24.0, 24.0)
    )
}

fn clip(name: &str, rate: f64, references: &[(&str, String)], active: &str) -> String {
    let references: Vec<String> = references
        .iter()
        .map(|(key, reference)| format!("{key:?}: {reference}"))
        .collect();
    format!(
        r#"{{"OTIO_SCHEMA": "Clip.2", "name": {name:?}, "source_range": {},
            "media_references": {{{}}}, "active_media_reference_key": {active:?}}}"#,
        range(24.0, rate),
        references.join(", ")
    )
}

/// Upstream's `create_simple_timeline`, with the given media references on
/// its three clips.
fn simple_timeline(
    video_1: &[(&str, String)],
    video_2: &[(&str, String)],
    audio_1: (&[(&str, String)], &str),
) -> (Document, NodeId) {
    let json = format!(
        r#"{{"OTIO_SCHEMA": "Timeline.1", "name": "",
            "tracks": {{"OTIO_SCHEMA": "Stack.1", "children": [
                {{"OTIO_SCHEMA": "Track.1", "name": "video", "kind": "Video",
                  "source_range": {r}, "children": [{v1}, {v2}]}},
                {{"OTIO_SCHEMA": "Track.1", "name": "audio", "kind": "Audio",
                  "source_range": {r}, "children": [{a1}]}}]}}}}"#,
        r = range(48.0, 24.0),
        v1 = clip("video clip 1", 24.0, video_1, "DEFAULT_MEDIA"),
        v2 = clip("video clip 2", 24.0, video_2, "DEFAULT_MEDIA"),
        a1 = clip("audio clip 1", 48.0, audio_1.0, audio_1.1),
    );
    let document = otio_core::from_str(&json).unwrap();
    let root = document.root().unwrap();
    (document, root)
}

fn default_media(reference: String) -> Vec<(&'static str, String)> {
    vec![("DEFAULT_MEDIA", reference)]
}

fn plain_timeline() -> (Document, NodeId) {
    simple_timeline(
        &default_media(missing()),
        &default_media(missing()),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    )
}

fn empty_timeline() -> (Document, NodeId) {
    let document = otio_core::from_str(
        r#"{"OTIO_SCHEMA": "Timeline.1", "tracks": {"OTIO_SCHEMA": "Stack.1", "children": []}}"#,
    )
    .unwrap();
    let root = document.root().unwrap();
    (document, root)
}

fn create_file(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"").unwrap();
}

/// Upstream's `create_refs`: an empty file for every reference that is one.
fn create_refs(document: &Document, timeline: NodeId, base: &Path) {
    for clip in document.find_clips(timeline).unwrap() {
        let Node::Clip(clip) = document.try_get(clip).unwrap() else {
            continue;
        };
        for reference in clip.media_references.values() {
            match document.try_get(*reference).unwrap() {
                Node::ExternalReference(external) => {
                    if let Some(file) = path_of(&external.target_url) {
                        create_file(&base.join(file));
                    }
                }
                Node::ImageSequenceReference(sequence) => {
                    let mut frame = sequence.start_frame;
                    while frame <= sequence.end_frame() {
                        let url = sequence.target_url_for_image_number(frame).unwrap();
                        if let Some(file) = path_of(&url) {
                            create_file(&base.join(file));
                        }
                        frame += sequence.frame_step;
                    }
                }
                _ => {}
            }
        }
    }
}

fn find_clip<'a>(document: &'a Document, name: &str) -> &'a otio_core::schema::Clip {
    let root = document.root().unwrap();
    document
        .find_clips(root)
        .unwrap()
        .into_iter()
        .find_map(|id| match document.try_get(id).unwrap() {
            Node::Clip(clip) if clip.item.base.name == name => Some(clip),
            _ => None,
        })
        .unwrap()
}

fn active_reference<'a>(document: &'a Document, clip: &str) -> &'a Node {
    let clip = find_clip(document, clip);
    document
        .try_get(clip.media_references[&clip.active_media_reference_key])
        .unwrap()
}

/// Upstream's `compare_filenames`: the same media, by file name, in each.
fn compare_filenames(a: (&Document, NodeId), b: &Document) {
    let a_clips = a.0.find_clips(a.1).unwrap();
    let b_clips = b.find_clips(b.root().unwrap()).unwrap();
    assert_eq!(a_clips.len(), b_clips.len());
    for (a_clip, b_clip) in a_clips.iter().zip(&b_clips) {
        let (Node::Clip(a_clip), Node::Clip(b_clip)) =
            (a.0.try_get(*a_clip).unwrap(), b.try_get(*b_clip).unwrap())
        else {
            panic!("find_clips found something that is not a clip");
        };
        assert_eq!(a_clip.media_references.len(), b_clip.media_references.len());
        for (a_ref, b_ref) in a_clip
            .media_references
            .values()
            .zip(b_clip.media_references.values())
        {
            match (a.0.try_get(*a_ref).unwrap(), b.try_get(*b_ref).unwrap()) {
                (Node::ExternalReference(a_ext), Node::ExternalReference(b_ext)) => {
                    let a_file = path_of(&a_ext.target_url).unwrap();
                    let b_file = path_of(&b_ext.target_url).unwrap();
                    assert_eq!(
                        Path::new(&a_file).file_name(),
                        Path::new(&b_file).file_name()
                    );
                }
                (Node::ImageSequenceReference(a_seq), Node::ImageSequenceReference(b_seq)) => {
                    assert_eq!(a_seq.name_prefix, b_seq.name_prefix);
                    assert_eq!(a_seq.name_suffix, b_seq.name_suffix);
                }
                _ => {}
            }
        }
    }
}

/// The timeline both round-trip tests bundle.
fn round_trip_timeline(temp: &Path) -> (Document, NodeId) {
    let audio_mp3 = temp.join("audio.mp3").to_string_lossy().into_owned();
    simple_timeline(
        &default_media(external("video1.mov")),
        &default_media(sequence(24)),
        (
            &[
                ("wav", external("audio.wav")),
                ("absolute_path", external(&audio_mp3)),
                ("sub_dir", external("sub_dir/audio.ogg")),
            ],
            "wav",
        ),
    )
}

/// Upstream's `test_file_from_url`.
#[test]
fn file_from_url_reads_upstreams_urls() {
    for (url, path) in [
        ("file://host/S%3a/path/file.ext", "S:/path/file.ext"),
        ("file://S:/path/file.ext", "S:/path/file.ext"),
        (
            "file://unc/path/sub%20dir/file.ext",
            "//unc/path/sub dir/file.ext",
        ),
        (
            "file://unc/path/sub dir/file.ext",
            "//unc/path/sub dir/file.ext",
        ),
        (
            "file://localhost/path/sub dir/file.ext",
            "/path/sub dir/file.ext",
        ),
        ("file:///path/sub%20dir/file.ext", "/path/sub dir/file.ext"),
        ("file:///path/sub dir/file.ext", "/path/sub dir/file.ext"),
    ] {
        assert_eq!(path_of(url).as_deref(), Some(path), "{url}");
    }
}

#[test]
fn otioz_round_trip() {
    let temp = TempDir::new("otioz-round-trip");
    let (document, timeline) = round_trip_timeline(temp.path());
    create_refs(&document, timeline, temp.path());

    let otioz = temp.path().join("round_trip.otioz");
    let options = WriteOptions {
        relative_media_base_dir: Some(temp.path().to_path_buf()),
        ..WriteOptions::default()
    };
    let size = dry_run(&document, timeline, &options).unwrap();
    assert!(size > 0);

    write_otioz(&document, timeline, &otioz, &options).unwrap();

    let result = read_otioz(&otioz, &ReadOptions::default()).unwrap();
    compare_filenames((&document, timeline), &result);

    let extract = temp.path().join("extract");
    let result = read_otioz(
        &otioz,
        &ReadOptions {
            extract_path: Some(extract.clone()),
            ..ReadOptions::default()
        },
    )
    .unwrap();
    compare_filenames((&document, timeline), &result);
    let media = extract.join(MEDIA_DIR);
    assert!(media.join("video1.mov").exists());
    for image in 0..24 {
        assert!(media.join(format!("render.{image}.exr")).exists());
    }
    assert!(media.join("audio.wav").exists());
    assert!(media.join("audio.mp3").exists());
    assert!(media.join("audio.ogg").exists());
    assert_eq!(
        fs::read_to_string(extract.join(VERSION_FILE)).unwrap(),
        VERSION
    );
    assert!(extract.join(TIMELINE_FILE).exists());
}

#[test]
fn otiod_round_trip() {
    let temp = TempDir::new("otiod-round-trip");
    let (document, timeline) = round_trip_timeline(temp.path());
    create_refs(&document, timeline, temp.path());

    let otiod = temp.path().join("round_trip.otiod");
    let options = WriteOptions {
        relative_media_base_dir: Some(temp.path().to_path_buf()),
        ..WriteOptions::default()
    };
    write_otiod(&document, timeline, &otiod, &options).unwrap();

    let result = read_otiod(
        &otiod,
        &ReadOptions {
            absolute_media_reference_paths: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    compare_filenames((&document, timeline), &result);

    let Node::ExternalReference(external) = active_reference(&result, "video clip 1") else {
        panic!("video clip 1 lost its external reference");
    };
    assert!(Path::new(&path_of(&external.target_url).unwrap()).is_absolute());
    let Node::ImageSequenceReference(sequence) = active_reference(&result, "video clip 2") else {
        panic!("video clip 2 lost its image sequence");
    };
    let first = sequence.target_url_for_image_number(0).unwrap();
    assert!(Path::new(&path_of(&first).unwrap()).is_absolute());
}

#[test]
fn otioz_media_policy() {
    let (document, timeline) = simple_timeline(
        &default_media(external("video1.mov")),
        &default_media(generator()),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );

    // error_if_not_file
    {
        let temp = TempDir::new("policy-error");
        create_refs(&document, timeline, temp.path());
        let options = WriteOptions {
            relative_media_base_dir: Some(temp.path().to_path_buf()),
            policy: MediaReferencePolicy::ErrorIfNotFile,
            ..WriteOptions::default()
        };
        assert!(dry_run(&document, timeline, &options).is_err());
        let otioz = temp.path().join("error_if_not_file.otioz");
        assert!(write_otioz(&document, timeline, &otioz, &options).is_err());
    }

    // missing_if_not_file
    {
        let temp = TempDir::new("policy-missing");
        create_refs(&document, timeline, temp.path());
        let options = WriteOptions {
            relative_media_base_dir: Some(temp.path().to_path_buf()),
            policy: MediaReferencePolicy::MissingIfNotFile,
            ..WriteOptions::default()
        };
        assert!(dry_run(&document, timeline, &options).is_ok());
        let otioz = temp.path().join("missing_if_not_file.otioz");
        write_otioz(&document, timeline, &otioz, &options).unwrap();
        let result = read_otioz(&otioz, &ReadOptions::default()).unwrap();
        let Node::MissingReference(reference) = active_reference(&result, "video clip 2") else {
            panic!("the generator was not replaced");
        };
        assert!(
            reference
                .media
                .base
                .metadata
                .contains_key("missing_reference_because")
        );
        assert!(matches!(
            active_reference(&result, "video clip 1"),
            Node::ExternalReference(_)
        ));
    }

    // all_missing
    {
        let temp = TempDir::new("policy-all-missing");
        create_refs(&document, timeline, temp.path());
        let options = WriteOptions {
            relative_media_base_dir: Some(temp.path().to_path_buf()),
            policy: MediaReferencePolicy::AllMissing,
            ..WriteOptions::default()
        };
        assert!(dry_run(&document, timeline, &options).is_ok());
        let otioz = temp.path().join("all_missing.otioz");
        write_otioz(&document, timeline, &otioz, &options).unwrap();
        let result = read_otioz(&otioz, &ReadOptions::default()).unwrap();
        assert!(matches!(
            active_reference(&result, "video clip 1"),
            Node::MissingReference(_)
        ));
        assert!(matches!(
            active_reference(&result, "video clip 2"),
            Node::MissingReference(_)
        ));
    }
}

#[test]
fn otioz_empty() {
    let temp = TempDir::new("otioz-empty");
    let (document, timeline) = empty_timeline();
    let otioz = temp.path().join("empty.otioz");
    write_otioz(&document, timeline, &otioz, &WriteOptions::default()).unwrap();
    let result = read_otioz(&otioz, &ReadOptions::default()).unwrap();
    assert!(
        result
            .find_clips(result.root().unwrap())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn otiod_empty() {
    let temp = TempDir::new("otiod-empty");
    let (document, timeline) = empty_timeline();
    let otiod = temp.path().join("empty.otiod");
    write_otiod(&document, timeline, &otiod, &WriteOptions::default()).unwrap();
    let result = read_otiod(&otiod, &ReadOptions::default()).unwrap();
    assert!(
        result
            .find_clips(result.root().unwrap())
            .unwrap()
            .is_empty()
    );
}

/// Upstream's `test_otioz_error` and `test_otiod_error`, which differ only
/// in the writer.
fn bundle_errors(write: fn(&Document, NodeId, &Path, &WriteOptions) -> otio_bundle::Result<()>) {
    let temp = TempDir::new("errors");
    let options = WriteOptions::default();
    let remove = |path: &Path| {
        let _ = fs::remove_file(path);
        let _ = fs::remove_dir_all(path);
    };

    let (document, timeline) = plain_timeline();
    let path = temp.path().join("error.bundle");
    write(&document, timeline, &path, &options).unwrap();

    // An existing bundle is not overwritten.
    assert!(write(&document, timeline, &path, &options).is_err());
    remove(&path);

    // Missing media.
    let (document, timeline) = simple_timeline(
        &default_media(external("video.mov")),
        &default_media(missing()),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );
    assert!(write(&document, timeline, &path, &options).is_err());
    remove(&path);

    // Two media files with one name.
    let (document, timeline) = simple_timeline(
        &default_media(external("video.mov")),
        &default_media(external("sub_dir/video.mov")),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );
    create_refs(&document, timeline, temp.path());
    let options = WriteOptions {
        relative_media_base_dir: Some(temp.path().to_path_buf()),
        ..WriteOptions::default()
    };
    assert!(write(&document, timeline, &path, &options).is_err());
    remove(&path);

    // A directory that does not exist, for the zip; the directory form
    // creates its parents, so it is refused for the media instead.
    let path = temp.path().join("subdir").join("error.bundle");
    assert!(write(&document, timeline, &path, &options).is_err());
    remove(&path);
}

#[test]
fn otioz_error() {
    bundle_errors(write_otioz);
    let temp = TempDir::new("otioz-no-directory");
    let (document, timeline) = plain_timeline();
    let path = temp.path().join("subdir").join("error.otioz");
    assert!(write_otioz(&document, timeline, &path, &WriteOptions::default()).is_err());
    assert!(!path.exists());
}

#[test]
fn otiod_error() {
    bundle_errors(write_otiod);
}

#[test]
fn only_a_timeline_is_bundled() {
    let temp = TempDir::new("not-a-timeline");
    let document = otio_core::from_str(r#"{"OTIO_SCHEMA": "Clip.2", "name": "a"}"#).unwrap();
    let clip = document.root().unwrap();
    let path = temp.path().join("clip.otioz");
    assert!(matches!(
        write_otioz(&document, clip, &path, &WriteOptions::default()),
        Err(otio_bundle::Error::NotATimeline(_))
    ));
}

/// A zip archive of stored entries, for writing names the bundle writer
/// would never produce.
fn malicious_zip(path: &Path, entries: &[(&str, &[u8])]) {
    fn crc(bytes: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in bytes {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc(data);
        let local = [
            &0x0403_4b50u32.to_le_bytes()[..],
            &20u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &0u32.to_le_bytes(),
            &crc.to_le_bytes(),
            &(data.len() as u32).to_le_bytes(),
            &(data.len() as u32).to_le_bytes(),
            &(name.len() as u16).to_le_bytes(),
            &0u16.to_le_bytes(),
        ]
        .concat();
        out.extend_from_slice(&local);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(
            &[
                &0x0201_4b50u32.to_le_bytes()[..],
                &20u16.to_le_bytes(),
                &20u16.to_le_bytes(),
                &0u16.to_le_bytes(),
                &0u16.to_le_bytes(),
                &0u32.to_le_bytes(),
                &crc.to_le_bytes(),
                &(data.len() as u32).to_le_bytes(),
                &(data.len() as u32).to_le_bytes(),
                &(name.len() as u16).to_le_bytes(),
                &[0u8; 12],
                &offset.to_le_bytes(),
            ]
            .concat(),
        );
        central.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(
        &[
            &0x0605_4b50u32.to_le_bytes()[..],
            &[0u8; 4],
            &(entries.len() as u16).to_le_bytes(),
            &(entries.len() as u16).to_le_bytes(),
            &(central.len() as u32).to_le_bytes(),
            &start.to_le_bytes(),
            &0u16.to_le_bytes(),
        ]
        .concat(),
    );
    fs::write(path, out).unwrap();
}

fn zip_slip(entry: &str, escaped: &Path, temp: &TempDir) {
    let otioz = temp.path().join("zip_slip.otioz");
    malicious_zip(
        &otioz,
        &[
            (VERSION_FILE, VERSION.as_bytes()),
            (TIMELINE_FILE, b"{}"),
            (entry, b""),
        ],
    );
    let extract = temp.path().join("extract");
    let result = read_otioz(
        &otioz,
        &ReadOptions {
            extract_path: Some(extract.clone()),
            ..ReadOptions::default()
        },
    );
    assert!(result.is_err());
    assert!(!escaped.exists());
    assert!(!extract.exists(), "a failed extraction is cleaned up");
}

#[test]
fn otioz_zip_slip_relative() {
    let temp = TempDir::new("zip-slip-relative");
    let escaped = temp.path().join("relative");
    zip_slip("../relative", &escaped, &temp);
}

#[test]
fn otioz_zip_slip_absolute() {
    let temp = TempDir::new("zip-slip-absolute");
    let escaped = temp.path().join("absolute");
    let name = escaped.to_string_lossy().into_owned();
    zip_slip(&name, &escaped, &temp);
}

/// Upstream's `test_otioz_zip64`. It needs about 24 GB of disk, so it only
/// runs when asked for: `cargo test -p otio-bundle -- --ignored`.
#[test]
#[ignore = "writes two 4 GiB media files"]
fn otioz_zip64() {
    let temp = TempDir::new("zip64");
    let (document, timeline) = simple_timeline(
        &default_media(external("video1.mov")),
        &default_media(sequence(65_536)),
        (&default_media(external("audio.wav")), "DEFAULT_MEDIA"),
    );
    create_refs(&document, timeline, temp.path());
    let large = 4u64 * 1024 * 1024 * 1024;
    for name in ["video1.mov", "audio.wav"] {
        fs::File::options()
            .write(true)
            .open(temp.path().join(name))
            .unwrap()
            .set_len(large)
            .unwrap();
    }

    let otioz = temp.path().join("zip64.otioz");
    let options = WriteOptions {
        relative_media_base_dir: Some(temp.path().to_path_buf()),
        ..WriteOptions::default()
    };
    let size = dry_run(&document, timeline, &options).unwrap();
    assert!(size > large * 2);
    write_otioz(&document, timeline, &otioz, &options).unwrap();
    assert!(fs::metadata(&otioz).unwrap().len() >= size);

    let extract = temp.path().join("extract");
    read_otioz(
        &otioz,
        &ReadOptions {
            extract_path: Some(extract.clone()),
            ..ReadOptions::default()
        },
    )
    .unwrap();
    for name in ["video1.mov", "audio.wav"] {
        assert_eq!(
            fs::metadata(extract.join(MEDIA_DIR).join(name))
                .unwrap()
                .len(),
            large
        );
    }
}

#[test]
fn a_url_upstream_cannot_decode_fails_the_bundle() {
    // Upstream decodes each `%` escape of a `file://` URL with
    // `std::stoi(pair, nullptr, 16)`, and does not catch what it throws:
    // `%zz` has no hex digit for `stoi` to read, so writing the bundle
    // fails with `std::invalid_argument("stoi")`, which upstream's Python
    // bindings raise as `ValueError("stoi")`. It decodes the URL before it
    // looks at the policy, so even `AllMissing`, which bundles no media,
    // fails. Nothing is written.
    let temp = TempDir::new("stray-percent");
    for url in ["file:///media/a%zz.mov", "file:///media/100%zz/a.mov"] {
        let (document, timeline) = simple_timeline(
            &default_media(external(url)),
            &default_media(missing()),
            (&default_media(missing()), "DEFAULT_MEDIA"),
        );
        for policy in [
            MediaReferencePolicy::ErrorIfNotFile,
            MediaReferencePolicy::MissingIfNotFile,
            MediaReferencePolicy::AllMissing,
        ] {
            let options = WriteOptions {
                policy,
                ..WriteOptions::default()
            };
            let error = dry_run(&document, timeline, &options).unwrap_err();
            assert!(
                matches!(error, otio_bundle::Error::InvalidEscape(_)),
                "{url} {policy:?}: {error:?}"
            );
            assert_eq!(error.to_string(), "stoi");

            let path = temp.path().join("stray.otioz");
            assert!(write_otioz(&document, timeline, &path, &options).is_err());
            assert!(!path.exists());
            let path = temp.path().join("stray.otiod");
            assert!(write_otiod(&document, timeline, &path, &options).is_err());
            assert!(!path.exists());
        }
    }

    // An image sequence's URLs are decoded the same way, the first image's
    // even when no media is bundled.
    let json = format!(
        r#"{{"OTIO_SCHEMA": "Timeline.1", "tracks": {{"OTIO_SCHEMA": "Stack.1", "children": [
            {{"OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [{}]}}]}}}}"#,
        clip(
            "frames",
            24.0,
            &default_media(format!(
                r#"{{"OTIO_SCHEMA": "ImageSequenceReference.1",
                    "available_range": {},
                    "target_url_base": "file:///media/a%zz/", "name_prefix": "render.",
                    "name_suffix": ".exr", "start_frame": 0, "frame_step": 1, "rate": 24.0,
                    "frame_zero_padding": 0, "missing_frame_policy": "error"}}"#,
                range(2.0, 24.0)
            )),
            "DEFAULT_MEDIA",
        )
    );
    let document = otio_core::from_str(&json).unwrap();
    let timeline = document.root().unwrap();
    let options = WriteOptions {
        policy: MediaReferencePolicy::AllMissing,
        ..WriteOptions::default()
    };
    assert!(matches!(
        dry_run(&document, timeline, &options),
        Err(otio_bundle::Error::InvalidEscape(_))
    ));
}

#[test]
fn a_percent_upstream_can_decode_is_decoded_as_upstream_decodes_it() {
    // `std::stoi` reads as much as it can: one hex digit is enough, so
    // `%4g` is byte 4, and a `%` with fewer than two characters after it is
    // not an escape at all and is kept. Neither fails the bundle; under
    // `AllMissing`, which needs no file, the write goes through.
    for url in ["file:///media/a%4g.mov", "file:///media/100%"] {
        let (document, timeline) = simple_timeline(
            &default_media(external(url)),
            &default_media(missing()),
            (&default_media(missing()), "DEFAULT_MEDIA"),
        );
        let options = WriteOptions {
            policy: MediaReferencePolicy::AllMissing,
            ..WriteOptions::default()
        };
        assert!(dry_run(&document, timeline, &options).is_ok(), "{url}");
    }
    assert_eq!(
        path_of("file:///media/a%4g.mov").as_deref(),
        Some("/media/a\u{4}.mov")
    );
    assert_eq!(
        path_of("file:///media/100%").as_deref(),
        Some("/media/100%")
    );
}

/// A file whose name is not UTF-8, and the `file://` URL that names it.
///
/// `None` where the filesystem refuses such a name, as Apple's APFS does.
#[cfg(unix)]
fn latin1_file(dir: &Path, name: &[u8], contents: &[u8]) -> Option<(PathBuf, String)> {
    use std::os::unix::ffi::OsStrExt;
    let path = dir.join(std::ffi::OsStr::from_bytes(name));
    fs::create_dir_all(path.parent().unwrap()).ok()?;
    if let Err(error) = fs::write(&path, contents) {
        eprintln!("skipped: {}: {error}", path.display());
        return None;
    }
    let mut url = format!("file://{}/", dir.display());
    for byte in name {
        if byte.is_ascii_alphanumeric() || b"._-/".contains(byte) {
            url.push(char::from(*byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    Some((path, url))
}

/// The target URL of the one clip's active reference.
#[cfg(unix)]
fn target_url(document: &Document, clip: &str) -> String {
    match active_reference(document, clip) {
        Node::ExternalReference(external) => external.target_url.clone(),
        other => panic!("{clip}: {}", other.schema_name()),
    }
}

#[test]
#[cfg(unix)]
fn a_media_file_whose_name_is_not_utf8_is_bundled() {
    // Upstream decodes `%E9` in a media URL to the byte 0xE9 and keeps it in
    // a `std::string`, which `std::filesystem::u8path` passes through on
    // POSIX, so it finds the file whose name has that byte in it. The
    // decoded path is carried as bytes here too; it used to be made into a
    // `String`, with the byte replaced, and the writer then looked for a
    // file that does not exist (issue #93).
    //
    // Upstream then bundles the file under its own raw name, flags that zip
    // entry as UTF-8 when it is not, and writes the raw byte into
    // `content.otio`, which is then not valid JSON; Python's `zipfile`
    // refuses the archive and upstream's own Python bindings raise
    // `UnicodeDecodeError` reading the reference back. That is unsound, so
    // the byte is spelled `%E9` in the bundled name instead, which the
    // reference names, which is valid in both the zip and the JSON, and
    // which upstream's reader finds too, as a plain relative path.
    let temp = TempDir::new("latin1");
    let Some((_, url)) = latin1_file(&temp.path().join("src"), b"caf\xe9.mov", b"not a movie")
    else {
        return;
    };
    assert!(url.ends_with("/src/caf%E9.mov"), "{url}");
    let (document, timeline) = simple_timeline(
        &default_media(external(&url)),
        &default_media(missing()),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );
    let options = WriteOptions {
        policy: MediaReferencePolicy::MissingIfNotFile,
        ..WriteOptions::default()
    };
    let bundled = "media/caf%E9.mov";

    // The size is read from the file, so the writer found it.
    assert!(dry_run(&document, timeline, &options).unwrap() > b"not a movie".len() as u64);

    // otioz: the entry and the reference agree, and extracting gives back
    // the file's bytes under that name.
    let otioz = temp.path().join("latin1.otioz");
    write_otioz(&document, timeline, &otioz, &options).unwrap();
    let raw = fs::read(&otioz).unwrap();
    assert!(!raw.windows(8).any(|w| w == b"caf\xe9.mov"));
    let extract = temp.path().join("extract");
    let result = read_otioz(
        &otioz,
        &ReadOptions {
            extract_path: Some(extract.clone()),
            absolute_media_reference_paths: false,
        },
    )
    .unwrap();
    assert_eq!(target_url(&result, "video clip 1"), bundled);
    assert_eq!(
        fs::read(extract.join(bundled)).unwrap(),
        b"not a movie".to_vec()
    );
    assert!(fs::read_to_string(extract.join(TIMELINE_FILE)).is_ok());

    // otiod: the same file under the same name.
    let otiod = temp.path().join("latin1.otiod");
    write_otiod(&document, timeline, &otiod, &options).unwrap();
    let names: Vec<_> = fs::read_dir(otiod.join(MEDIA_DIR))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, [std::ffi::OsString::from("caf%E9.mov")]);
    assert_eq!(
        fs::read(otiod.join(bundled)).unwrap(),
        b"not a movie".to_vec()
    );
    let result = read_otiod(
        &otiod,
        &ReadOptions {
            absolute_media_reference_paths: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    assert!(Path::new(&target_url(&result, "video clip 1")).is_file());

    // A directory whose name is not UTF-8 is read the same way; only the
    // file's own name goes in the bundle.
    let Some((_, url)) = latin1_file(&temp.path().join("src"), b"d\xe9j\xe0/vu.mov", b"x") else {
        return;
    };
    let (document, timeline) = simple_timeline(
        &default_media(external(&url)),
        &default_media(missing()),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );
    let otiod = temp.path().join("dir.otiod");
    write_otiod(&document, timeline, &otiod, &WriteOptions::default()).unwrap();
    assert_eq!(fs::read(otiod.join("media/vu.mov")).unwrap(), b"x".to_vec());
}

#[test]
#[cfg(unix)]
fn names_that_are_not_utf8_clash_only_when_their_bundled_names_do() {
    // Upstream refuses two media files that would land on one name under
    // media/, comparing the names byte for byte (ignoring ASCII case). Two
    // names differing only in bytes that are not UTF-8 are different files,
    // and used to be refused here once both had become U+FFFD.
    let temp = TempDir::new("latin1-clash");
    let Some((_, first)) = latin1_file(&temp.path().join("one"), b"caf\xe9.mov", b"1") else {
        return;
    };
    let (_, second) = latin1_file(&temp.path().join("two"), b"caf\xe8.mov", b"2").unwrap();
    let (document, timeline) = simple_timeline(
        &default_media(external(&first)),
        &default_media(external(&second)),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );
    let otiod = temp.path().join("apart.otiod");
    write_otiod(&document, timeline, &otiod, &WriteOptions::default()).unwrap();
    assert_eq!(fs::read(otiod.join("media/caf%E9.mov")).unwrap(), b"1");
    assert_eq!(fs::read(otiod.join("media/caf%E8.mov")).unwrap(), b"2");

    // The escaped name is a real name too. A file already called
    // `caf%E9.mov` beside `caf\xe9.mov` would take its place in the bundle,
    // so that is refused, as upstream refuses two files of one name.
    let dir = temp.path().join("one");
    let (_, escaped) = latin1_file(&dir, b"caf%E9.mov", b"3").unwrap();
    assert!(escaped.ends_with("caf%25E9.mov"), "{escaped}");
    let (document, timeline) = simple_timeline(
        &default_media(external(&first)),
        &default_media(external(&escaped)),
        (&default_media(missing()), "DEFAULT_MEDIA"),
    );
    for write in [write_otioz, write_otiod] {
        let path = temp.path().join("clash.bundle");
        let error = write(&document, timeline, &path, &WriteOptions::default()).unwrap_err();
        assert!(
            matches!(&error, otio_bundle::Error::FileWrite(message) if message.contains("would overwrite")),
            "{error:?}"
        );
        assert!(!path.exists());
    }
}

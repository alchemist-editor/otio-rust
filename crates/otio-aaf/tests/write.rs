//! Writing a timeline as an AAF, checked byte for byte against upstream.
//!
//! `tests/data/generators/gen_written.py` has upstream's adapter write each
//! input timeline with pyaaf2, and records beside each file every time and
//! identifier that was asked for while it did, in order, with the options it
//! wrote with. Each test here writes the same timeline with this crate,
//! feeding the writer those same values, and asserts three things: that the
//! writer asked for exactly that sequence, that the two files are identical,
//! and that reading ours back gives what upstream reads back from its own.
//!
//! The inputs are the vendored baselines of upstream's samples, which are
//! what upstream reads from them, and two timelines the generator builds
//! for what no sample reaches, saved as OTIO 0.18 JSON.
//!
//! The replaying and the comparing are shared with the `aaf` crate's own
//! write tests.

#[path = "../../aaf/tests/written/mod.rs"]
mod written;

use std::path::{Path, PathBuf};

use otio_aaf::{Error, Sources, WriteOptions};
use written::{Sidecar, assert_identical};

/// The written fixtures.
fn written_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/written")
}

/// The samples written back, whose inputs are the read baselines beside the
/// samples.
const SAMPLES: [&str; 7] = [
    "colored_clips",
    "essence_group",
    "marker-over-transition",
    "misc_speed_effects",
    "nested_audio_dissolve",
    "nesting_test",
    "sector_size_512",
];

/// The timelines the generator built, whose inputs are saved beside what
/// was written from them.
const BUILT: [&str; 2] = ["edit", "options"];

/// The timeline written as fixture `name`.
fn input(name: &str) -> otio_core::Document {
    let path = if BUILT.contains(&name) {
        written_dir().join(format!("{name}.otio.json"))
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/data/{name}.otio.json"))
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    otio_core::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Options set up as the generator's were for the fixture the sidecar
/// describes, drawing from its replay.
fn options_for(sidecar: &Sidecar) -> WriteOptions {
    let mut options = WriteOptions::new()
        .with_prefer_file_mob_id(sidecar.option("prefer_file_mob_id"))
        .with_use_empty_mob_ids(sidecar.option("use_empty_mob_ids"))
        .with_create_edgecode(sidecar.option("create_edgecode"))
        .with_sources(Sources::new(sidecar.replay.clone(), sidecar.replay.clone()))
        // The generator pins Python's sys.platform, which pyaaf2 records.
        .with_platform("linux");
    if let Some(user) = &sidecar.user {
        options = options.with_user(user.clone());
    }
    options
}

/// Writes fixture `name`'s input as upstream did, and checks the file and
/// the sequence of calls against upstream's.
fn check(name: &str) -> Vec<u8> {
    let dir = written_dir();
    let sidecar = Sidecar::read(name, &dir.join(format!("{name}.calls.tsv")));
    let options = options_for(&sidecar);
    let expected = std::fs::read(dir.join(format!("{name}.aaf"))).expect("the fixture exists");

    let ours = otio_aaf::write_to_bytes_with(&input(name), &options)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    sidecar.replay.assert_used_up();
    assert_identical(name, &ours, &expected);
    ours
}

/// Reads a written file back and compares it with what upstream read back
/// from its own, line by line.
fn check_round_trip(name: &str, bytes: &[u8]) {
    check_round_trip_in(&written_dir(), name, bytes);
}

/// [`check_round_trip`], against the baseline in `dir`.
fn check_round_trip_in(dir: &Path, name: &str, bytes: &[u8]) {
    let document = otio_aaf::read(std::io::Cursor::new(bytes)).expect("the written file reads");
    let found =
        otio_core::to_string_pretty(&document, otio_core::DEFAULT_INDENT).expect("it serializes");
    let path = dir.join(format!("{name}.roundtrip.otio.json"));
    let expected = std::fs::read_to_string(&path).expect("the round-trip baseline exists");
    if let Some((line, want, got)) = expected
        .lines()
        .zip(found.lines())
        .enumerate()
        .find(|(_, (want, got))| want != got)
        .map(|(number, (want, got))| (number + 1, want, got))
    {
        panic!("{name}: read back, line {line} differs\n  upstream: {want}\n  ours    : {got}");
    }
    assert_eq!(
        expected.lines().count(),
        found.lines().count(),
        "{name}: read back, the two differ in length"
    );
}

#[test]
fn every_sample_is_written_as_upstream_writes_it() {
    for name in SAMPLES {
        check(name);
    }
}

/// A cut built for what no sample reaches: clips with and without an AAF
/// behind them, a slug, a nested track, a shared master mob, a transition
/// the writer skips, markers old and new, colours, comments, and sound with
/// pan points and a dissolve.
#[test]
fn a_cut_built_from_scratch_is_written_as_upstream_writes_it() {
    check("edit");
}

/// The writer's options: MobIDs taken from the AAF a clip's media names,
/// made up where there is none, and edge code on every master mob.
#[test]
fn the_writers_options_write_as_upstreams_do() {
    check("options");
}

#[test]
fn what_is_written_reads_back_as_upstream_reads_its_own() {
    for name in SAMPLES.iter().chain(&BUILT) {
        let bytes = std::fs::read(written_dir().join(format!("{name}.aaf"))).expect("fixture");
        check_round_trip(name, &bytes);
    }
}

/// Reading our own file back gives what reading upstream's does, which the
/// byte comparison already implies; this checks the reader and writer agree
/// without relying on it.
#[test]
fn a_file_written_here_reads_back_as_upstreams_does() {
    let bytes = check("marker-over-transition");
    check_round_trip("marker-over-transition", &bytes);
}

/// With the same sources, the same timeline is written the same way twice,
/// and without any, the system's clock and random identifiers are used.
#[test]
fn deterministic_sources_write_the_same_bytes_every_time() {
    use aaf::write::{SequentialIds, SteppingClock, Timestamp};
    let start = Timestamp::parse_iso("2024-05-06T07:08:09").expect("a time");
    let write = || {
        let options = WriteOptions::new()
            .with_user("editor")
            .with_sources(Sources::new(
                SteppingClock::new(start),
                SequentialIds::new(0),
            ));
        otio_aaf::write_to_bytes_with(&input("edit"), &options).expect("it writes")
    };
    assert_eq!(write(), write());

    let options = WriteOptions::new().with_user("editor");
    let a = otio_aaf::write_to_bytes_with(&input("edit"), &options).expect("it writes");
    otio_aaf::read(std::io::Cursor::new(a)).expect("it reads back");
}

/// Upstream's `validate_metadata`, which lists everything wrong before
/// writing anything: here, seven clips at 23.976 in a 29.97 timeline, each
/// reported twice, word for word as upstream reports them.
#[test]
fn a_timeline_of_mixed_rates_is_refused_with_everything_wrong_listed() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/2997fps-DFTC.otio.json");
    let text = std::fs::read_to_string(path).expect("the baseline is readable");
    let document = otio_core::from_str(&text).expect("the baseline reads");
    let error = otio_aaf::write_to_bytes(&document).expect_err("it is refused");
    let Error::Invalid(problems) = error else {
        panic!("{error:?}");
    };
    let clip = "58982c7f-78b3-4cf9-8432-729fc4eafd55.mov<class 'opentimelineio._otio.Clip'> Clip";
    let expected: Vec<String> = (0..7)
        .flat_map(|_| {
            ["duration", "start_time"].map(|field| {
                format!(
                    "{clip}.media_reference.available_range.{field}.rate not equal to \
                     29.97002997002997 (expected) != 23.976023976023978 (actual)"
                )
            })
        })
        .collect();
    assert_eq!(problems, expected);
}

#[test]
fn a_document_that_is_not_a_timeline_is_refused() {
    let document = otio_aaf::read_from_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../aaf/tests/data/empty.aaf"),
    )
    .expect("the fixture reads");
    let error = otio_aaf::write_to_bytes(&document).expect_err("it is refused");
    assert!(matches!(error, Error::Unsupported(_)), "{error:?}");
}

#[test]
fn embedding_essence_is_refused_as_not_implemented() {
    let options = WriteOptions::new().with_embed_essence(true);
    let error = otio_aaf::write_to_bytes_with(&input("edit"), &options).expect_err("it is refused");
    let Error::Unsupported(why) = error else {
        panic!("{error:?}");
    };
    assert!(why.contains("embed"), "{why}");
}

/// A clip with no MobID anywhere, and no leave to make one up, stops the
/// write, as upstream's does.
#[test]
fn a_clip_with_no_mob_id_is_refused_unless_one_may_be_made_up() {
    let document = input("options");
    let error = otio_aaf::write_to_bytes_with(&document, &WriteOptions::new().with_user("editor"))
        .expect_err("it is refused");
    assert!(error.to_string().contains("Cannot find mob ID"), "{error}");
    // Made up, it writes.
    let options = WriteOptions::new()
        .with_user("editor")
        .with_use_empty_mob_ids(true);
    otio_aaf::write_to_bytes_with(&document, &options).expect("it writes");
}

/// Every sample in upstream's corpus that upstream can write, written byte
/// for byte as upstream writes it and read back as upstream reads it, when the
/// generator has been run with `--all DIR` and `OTIO_AAF_WRITTEN_ALL=DIR` is
/// set. Not run by default: the corpus is not vendored.
#[test]
#[ignore = "needs gen_written.py --all DIR and OTIO_AAF_WRITTEN_ALL=DIR"]
fn every_writable_sample_in_upstreams_corpus_is_written_as_upstream_writes_it() {
    let dir = PathBuf::from(
        std::env::var_os("OTIO_AAF_WRITTEN_ALL").expect("OTIO_AAF_WRITTEN_ALL is set"),
    );
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the directory is readable")
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".calls.tsv").map(str::to_owned)
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "nothing in {}", dir.display());
    let mut failed = Vec::new();
    for name in &names {
        let sidecar = Sidecar::read(name, &dir.join(format!("{name}.calls.tsv")));
        let options = options_for(&sidecar);
        let text = std::fs::read_to_string(dir.join(format!("{name}.otio.json"))).expect("input");
        let document = otio_core::from_str(&text).expect("the input reads");
        let expected = std::fs::read(dir.join(format!("{name}.aaf"))).expect("the file");
        let result = std::panic::catch_unwind(|| {
            let ours = otio_aaf::write_to_bytes_with(&document, &options)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            sidecar.replay.assert_used_up();
            assert_identical(name, &ours, &expected);
            check_round_trip_in(&dir, name, &ours);
        });
        if result.is_err() {
            failed.push(name.clone());
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} differ: {failed:?}",
        failed.len(),
        names.len()
    );
}

//! The format as the rest of the workspace reaches it.
//!
//! `transcribe.rs` checks what this crate reads. This checks that the trait
//! the workspace dispatches on leads to the same place, both ways; `write.rs`
//! checks what it writes.

use otio_aaf::Aaf;
use otio_adapter::Adapter;

/// The AAF files, which belong to the `aaf` crate.
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../aaf/tests/data")
        .join(name)
}

/// The adapter claims the suffix a caller would dispatch on.
#[test]
fn the_adapter_names_the_format_it_reads() {
    assert_eq!(Aaf::NAME, "AAF");
    assert_eq!(Aaf::SUFFIXES, ["aaf"]);
}

/// Both ways in produce the same timeline.
///
/// `read_from_file` is overridden to stream the file rather than read it into
/// memory first, so the two are separate code paths and could drift apart.
#[test]
fn reading_a_file_and_reading_its_bytes_agree() {
    let path = fixture("sector_size_512.aaf");
    let options = <Aaf as Adapter>::ReadOptions::default();

    let from_file = Aaf::read_from_file(&path, &options).expect("the fixture reads");
    let bytes = std::fs::read(&path).expect("the fixture is readable");
    let from_bytes = Aaf::read_from_bytes(&bytes, &options).expect("its bytes read");

    let written = |document| {
        otio_core::to_string_pretty(document, otio_core::DEFAULT_INDENT).expect("it serializes")
    };
    assert_eq!(written(&from_file), written(&from_bytes));
}

/// A file that is not an AAF is a parse failure, not a panic.
#[test]
fn something_that_is_not_an_aaf_is_reported_as_one() {
    let error = Aaf::read_from_bytes(b"this is not a compound file", &Default::default())
        .expect_err("it does not read");
    assert!(
        matches!(error, otio_adapter::Error::Parse { .. }),
        "{error:?}"
    );
}

/// Writing through the trait writes the file the crate's own function does.
///
/// `tests/write.rs` holds that function to upstream byte for byte; this
/// checks the trait reaches it, with the options it was given, and that the
/// file reads back through the trait too.
#[test]
fn writing_through_the_adapter_writes_what_the_crate_writes() {
    use aaf::write::{SequentialIds, SteppingClock, Timestamp};

    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/written/edit.otio.json");
    let document =
        otio_core::from_str(&std::fs::read_to_string(path).expect("the input is readable"))
            .expect("the input reads");
    let options = || {
        let start = Timestamp::parse_iso("2024-05-06T07:08:09").expect("a time");
        otio_aaf::WriteOptions::new()
            .with_user("editor")
            .with_sources(otio_aaf::Sources::new(
                SteppingClock::new(start),
                SequentialIds::new(0),
            ))
    };

    let through_trait = Aaf::write_to_bytes(&document, &options()).expect("it writes");
    let direct = otio_aaf::write_to_bytes_with(&document, &options()).expect("it writes");
    assert_eq!(through_trait, direct);

    let back = Aaf::read_from_bytes(&through_trait, &Default::default()).expect("it reads back");
    assert!(back.root().is_some());
}

/// What the writer cannot write is reported as unsupported through the
/// trait: an AAF holding nothing reads as a collection, and upstream writes
/// only a timeline.
#[test]
fn writing_something_that_is_not_a_timeline_is_reported_as_unsupported() {
    let document =
        Aaf::read_from_file(fixture("empty.aaf"), &Default::default()).expect("the fixture reads");
    let error = Aaf::write_to_bytes(&document, &Default::default()).expect_err("it does not write");
    assert!(
        matches!(error, otio_adapter::Error::Unsupported { .. }),
        "{error:?}"
    );
}

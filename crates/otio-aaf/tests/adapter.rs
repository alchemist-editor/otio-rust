//! The format as the rest of the workspace reaches it.
//!
//! `transcribe.rs` checks what this crate reads. This checks that the trait
//! the workspace dispatches on leads to the same place, and that the write
//! half says so rather than producing something wrong.

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

/// Writing reports that the format cannot be written.
///
/// Upstream's adapter has a write half; this crate has not ported it. Saying
/// so is the honest answer, and it is what [`otio_adapter::Error::Unsupported`]
/// is for.
#[test]
fn writing_an_aaf_reports_that_it_is_not_implemented() {
    let document =
        Aaf::read_from_file(fixture("empty.aaf"), &Default::default()).expect("the fixture reads");
    let error = Aaf::write_to_bytes(&document, &Default::default()).expect_err("it does not write");
    assert!(
        matches!(error, otio_adapter::Error::Unsupported { .. }),
        "{error:?}"
    );
}

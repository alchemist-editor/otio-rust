//! The [`Adapter`] implementation, which is how the rest of the workspace
//! reaches this format.
//!
//! AAF is the reason [`Adapter`] is stated over bytes rather than over `&str`:
//! it is a compound file, not text, so it implements [`Adapter`] and not
//! [`otio_adapter::TextAdapter`].
//!
//! Reading from bytes means holding the whole file in memory. That is what the
//! trait asks for, and [`crate::read`] takes anything that reads and seeks, so
//! [`Adapter::read_from_file`] is overridden to hand it the file itself and
//! leave a large AAF on disk where it is.

use std::path::Path;

use otio_adapter::{Adapter, Error, Result};
use otio_core::Document;

/// The Advanced Authoring Format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aaf;

/// What a caller can ask for when reading an AAF.
///
/// Nothing yet. Upstream takes `simplify`, `attach_markers` and
/// `bake_keyframed_properties`, and each belongs to a pass this crate has not
/// ported; they arrive here as fields when those do. The type exists now so
/// that adding one is not a change to every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ReadOptions {}

/// What a caller can ask for when writing an AAF.
///
/// Writing is not implemented, so there is nothing to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct WriteOptions {}

impl Adapter for Aaf {
    type ReadOptions = ReadOptions;
    type WriteOptions = WriteOptions;

    const NAME: &'static str = "AAF";
    const SUFFIXES: &'static [&'static str] = &["aaf"];

    fn read_from_bytes(input: &[u8], _options: &Self::ReadOptions) -> Result<Document> {
        crate::read(std::io::Cursor::new(input)).map_err(into_adapter_error)
    }

    /// Reads straight from the file rather than from its bytes.
    ///
    /// An AAF is read by seeking around it, so there is no reason to copy a
    /// file that may be hundreds of megabytes into memory first.
    fn read_from_file(path: impl AsRef<Path>, _options: &Self::ReadOptions) -> Result<Document> {
        crate::read_from_file(path).map_err(into_adapter_error)
    }

    fn write_to_bytes(_document: &Document, _options: &Self::WriteOptions) -> Result<Vec<u8>> {
        Err(Error::unsupported(
            "writing an AAF is not implemented; this adapter reads only",
        ))
    }
}

/// This crate's error as the one the trait reports.
///
/// A malformed file is a parse failure whatever layer noticed it, since to a
/// caller of the adapter there is one operation and it did not work.
fn into_adapter_error(error: crate::Error) -> Error {
    match error {
        crate::Error::Io(error) => Error::Io(error),
        crate::Error::Otio(error) => Error::Core(error),
        other => Error::parse(other.to_string()),
    }
}

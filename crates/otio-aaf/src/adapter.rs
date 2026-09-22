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
/// Upstream's `read_from_file` arguments, with upstream's defaults: both
/// passes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReadOptions {
    /// Collapse the nesting AAF has and OTIO does not need.
    ///
    /// Off, the timeline keeps AAF's shape: a track per slot, a stack per
    /// nested composition, a track per sequence inside it.
    pub simplify: bool,
    /// Move each marker from the slot that carries it onto the item it
    /// points at.
    ///
    /// Off, markers stay on the tracks AAF keeps them on, with their
    /// positions in those tracks' time.
    pub attach_markers: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            simplify: true,
            attach_markers: true,
        }
    }
}

impl ReadOptions {
    /// Upstream's defaults: both passes on.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The structural transcription alone, with neither pass: what upstream
    /// calls reading with `simplify=False` and `attach_markers=False`.
    #[must_use]
    pub const fn structural() -> Self {
        Self {
            simplify: false,
            attach_markers: false,
        }
    }

    /// These options with `simplify` set.
    #[must_use]
    pub const fn with_simplify(mut self, simplify: bool) -> Self {
        self.simplify = simplify;
        self
    }

    /// These options with `attach_markers` set.
    #[must_use]
    pub const fn with_attach_markers(mut self, attach_markers: bool) -> Self {
        self.attach_markers = attach_markers;
        self
    }
}

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

    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document> {
        crate::read_with(std::io::Cursor::new(input), options).map_err(into_adapter_error)
    }

    /// Reads straight from the file rather than from its bytes.
    ///
    /// An AAF is read by seeking around it, so there is no reason to copy a
    /// file that may be hundreds of megabytes into memory first.
    fn read_from_file(path: impl AsRef<Path>, options: &Self::ReadOptions) -> Result<Document> {
        crate::read_from_file_with(path, options).map_err(into_adapter_error)
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

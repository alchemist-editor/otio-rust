//! Avid Log Exchange (ALE), read and written as an OpenTimelineIO document.
//!
//! An ALE is a tab-separated shot log: a `Heading` of key/value pairs, a
//! `Column` line naming the fields, and a `Data` section with one row per
//! clip. Avid writes them, colourists read them, and everything in between
//! passes them around, so they carry whatever columns the tool that wrote
//! them felt like carrying.
//!
//! A file reads as a [`SerializableCollection`](otio_core::schema::SerializableCollection)
//! of clips. Columns this adapter understands — `Name`, `Start`, `Duration`,
//! `End`, `Source File`, and the colour-decision columns — become real fields
//! on the clip; every other column is kept verbatim under the clip's
//! `metadata["ALE"]`, and the file's heading and column order under the
//! collection's. That is what makes a round trip lossless.
//!
//! ```
//! use otio_adapter::{Adapter, TextAdapter};
//! use otio_ale::Ale;
//!
//! let input = "Heading\nFPS\t24\n\nColumn\nName\tStart\tEnd\n\nData\nshot_01\t01:00:00:00\t01:00:01:00\n";
//! let document = Ale::read_from_str(input, &Default::default())?;
//!
//! let clips = document.find_clips(document.root().expect("a root"))?;
//! assert_eq!(document.try_get(clips[0])?.name(), "shot_01");
//! # Ok::<(), otio_adapter::Error>(())
//! ```
//!
//! # Fidelity to upstream
//!
//! This is a port of OpenTimelineIO's `otio-ale-adapter`, and it reproduces
//! that adapter's behaviour rather than improving on it. Where upstream does
//! something surprising, the surprise is kept and the test that pins it says
//! why. Two worth knowing about:
//!
//! - A value containing a tab is written out as-is, so it silently becomes two
//!   columns when read back. Upstream means to replace tabs with spaces and
//!   discards the result of doing so.
//! - The `Name` column stays in `metadata["ALE"]` as well as becoming the
//!   clip's name, so it appears twice in the document and once in the file.

mod read;
mod video_format;
mod write;

use otio_adapter::{Adapter, Error, Result, TextAdapter};
use otio_core::Document;

pub use video_format::{DEFAULT_VIDEO_FORMAT, video_format_for};

/// The frame rate assumed when neither the file nor the caller states one.
pub const DEFAULT_FPS: f64 = 24.0;

/// The column a clip's name is read from, unless the caller says otherwise.
pub const DEFAULT_NAME_COLUMN: &str = "Name";

/// What to do while reading an ALE.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadOptions {
    /// The rate timecode is read at, when the file's heading has no `FPS`.
    ///
    /// A heading that states `FPS` wins: an ALE is written for a particular
    /// rate, and the file knows it better than the caller does.
    pub fps: f64,

    /// The column a clip takes its name from.
    ///
    /// Some facilities put the useful name in `Tape` or `Source File` rather
    /// than in `Name`.
    pub name_column: String,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            fps: DEFAULT_FPS,
            name_column: DEFAULT_NAME_COLUMN.to_string(),
        }
    }
}

/// What to do while writing an ALE.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WriteOptions {
    /// The columns to write, in order.
    ///
    /// `None` takes the order the document carries under `metadata["ALE"]`
    /// and appends any column a clip has that the order does not mention. The
    /// five columns this adapter derives from real fields are always written,
    /// whichever way the order is arrived at.
    pub columns: Option<Vec<String>>,

    /// The rate timecode is written at.
    ///
    /// `None` takes the heading's `FPS`, or [`DEFAULT_FPS`] if it has none, in
    /// which case the heading gains one.
    pub fps: Option<f64>,

    /// The `VIDEO_FORMAT` to state in the heading.
    ///
    /// `None` keeps the heading's own, and guesses one from the clips' `Image
    /// Size` columns if the heading has none.
    pub video_format: Option<String>,
}

/// The ALE adapter.
///
/// This is a marker: everything it does is on [`Adapter`] and [`TextAdapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ale;

impl Adapter for Ale {
    type ReadOptions = ReadOptions;
    type WriteOptions = WriteOptions;

    const NAME: &'static str = "ale";
    const SUFFIXES: &'static [&'static str] = &["ale"];

    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document> {
        Self::read_from_str(std::str::from_utf8(input)?, options)
    }

    fn write_to_bytes(document: &Document, options: &Self::WriteOptions) -> Result<Vec<u8>> {
        Self::write_to_string(document, options).map(String::into_bytes)
    }
}

impl TextAdapter for Ale {
    fn read_from_str(input: &str, options: &Self::ReadOptions) -> Result<Document> {
        read::read(input, options)
    }

    fn write_to_string(document: &Document, options: &Self::WriteOptions) -> Result<String> {
        write::write(document, options)
    }
}

/// Returns the document's root, failing if it has none.
fn root(document: &Document) -> Result<otio_core::NodeId> {
    document
        .root()
        .ok_or_else(|| Error::unsupported("the document has no root object to write"))
}

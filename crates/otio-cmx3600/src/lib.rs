//! CMX 3600 Edit Decision Lists, read and written as OpenTimelineIO documents.
//!
//! An EDL is the oldest interchange format in post, and the most widely
//! understood: a title, then a numbered list of events, each one or two lines
//! of timecode with free-form comments beneath.
//!
//! ```text
//! TITLE: Example_Screening.01
//!
//! 001  ZZ100_50 V     C        01:00:04:05 01:00:05:12 00:59:53:11 00:59:54:18
//! * FROM CLIP NAME:  take_1
//! * FROM FILE: S:/path/to/ZZ100_501.take_1.0001.exr
//! ```
//!
//! A file reads as a [`Timeline`](otio_core::schema::Timeline). Each channel
//! the events name becomes a track, cuts and dissolves become clips and
//! transitions, holes in the record timecode become gaps, `LOC` comments
//! become markers, and anything this adapter does not recognize is kept on
//! the clip's `metadata["cmx_3600"]` so that writing the timeline back out
//! reproduces it.
//!
//! ```
//! use otio_adapter::TextAdapter;
//! use otio_cmx3600::Cmx3600;
//!
//! let edl = "TITLE: A cut\n\n001  AX V C 01:00:00:00 01:00:01:00 00:00:00:00 00:00:01:00\n";
//! let document = Cmx3600::read_from_str(edl, &Default::default())?;
//!
//! let root = document.root().expect("a root");
//! assert_eq!(document.try_get(root)?.name(), "A cut");
//! # Ok::<(), otio_adapter::Error>(())
//! ```
//!
//! # Rates
//!
//! An EDL does not say what rate its timecode is at. There is no way to infer
//! it either, so [`ReadOptions::rate`] has to be right, and a file read at
//! the wrong rate produces a timeline whose events are all in the wrong
//! place. [`DEFAULT_RATE`] is the usual guess, not a safe one.
//!
//! # What this adapter will not do
//!
//! Writing is narrower than reading, as it is upstream:
//!
//! - Only one video track. An EDL describes a single strand of picture, so a
//!   timeline with two video tracks has no EDL form.
//! - Only the first track of the timeline is written, whichever it is.
//! - Dissolves, but not wipes. A wipe reads, and writes back out as a
//!   dissolve.
//! - One timing effect per clip, and only a speed change or a freeze frame.

mod comment;
mod image_sequence;
mod path;
mod read;
mod statement;
mod write;

use std::fmt;
use std::str::FromStr;

use otio_adapter::{Adapter, Error, Result, TextAdapter};
use otio_core::Document;

/// The rate timecode is read at when the caller says nothing.
pub const DEFAULT_RATE: f64 = 24.0;

/// How many characters a reel name is padded or truncated to, by default.
pub const DEFAULT_REELNAME_LEN: usize = 8;

/// Which system's conventions to write for.
///
/// The three disagree about the comment that names a clip's media, and each
/// is unreadable to the others. Upstream takes this as a string and raises at
/// run time for one it does not know; here an unknown dialect cannot be
/// spelled, and [`Style::from_str`] is where a name from outside the library
/// is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Avid Media Composer: names media with `* FROM CLIP:`.
    #[default]
    Avid,
    /// Nucoda: names media with `* FROM FILE:`.
    Nucoda,
    /// Adobe Premiere Pro, which names no media at all.
    ///
    /// Premiere reads a `FROM` comment as meaning the clip has no name, and
    /// calls it `UNKNOWN`, so this dialect uses `AX` as every reel and puts
    /// the path in an `* OTIO REFERENCE` comment that Premiere ignores and
    /// this adapter can read back.
    Premiere,
}

impl Style {
    /// Returns the word this dialect is named by.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Avid => "avid",
            Self::Nucoda => "nucoda",
            Self::Premiere => "premiere",
        }
    }

    /// Returns the word this dialect names media with, if it names it at all.
    const fn media_comment(self) -> Option<&'static str> {
        match self {
            Self::Avid => Some("CLIP"),
            Self::Nucoda => Some("FILE"),
            Self::Premiere => None,
        }
    }
}

impl fmt::Display for Style {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Style {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "avid" => Ok(Self::Avid),
            "nucoda" => Ok(Self::Nucoda),
            "premiere" => Ok(Self::Premiere),
            other => Err(Error::unsupported(format!(
                "the EDL style '{other}' is not supported"
            ))),
        }
    }
}

/// What to do while reading an EDL.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadOptions {
    /// The rate the file's timecode is at.
    ///
    /// Nothing in the file states this, and nothing can infer it.
    pub rate: f64,

    /// Whether to accept a file whose record timecode does not add up.
    ///
    /// An event's source and record spans should be the same length, and each
    /// event should start where the one before it ended. Plenty of real files
    /// break both rules. With this set, the source timecode is believed and
    /// events are slid along to keep the track in order; without it, such a
    /// file is refused.
    pub ignore_timecode_mismatch: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            rate: DEFAULT_RATE,
            ignore_timecode_mismatch: false,
        }
    }
}

/// What to do while writing an EDL.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteOptions {
    /// The rate to write timecode at.
    ///
    /// `None` takes the rate of the timeline's first track.
    pub rate: Option<f64>,

    /// Which system's conventions to write for.
    pub style: Style,

    /// How many characters to pad or truncate a reel name to.
    ///
    /// `None` writes the name in full, which most systems will not read but
    /// keeps the information. A truncated name is recorded in an
    /// `* OTIO TRUNCATED REEL NAME FROM:` comment so that reading the file
    /// back gets the original.
    pub reelname_len: Option<usize>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            rate: None,
            style: Style::default(),
            reelname_len: Some(DEFAULT_REELNAME_LEN),
        }
    }
}

/// The CMX 3600 EDL adapter.
///
/// This is a marker: everything it does is on [`Adapter`] and [`TextAdapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cmx3600;

impl Adapter for Cmx3600 {
    type ReadOptions = ReadOptions;
    type WriteOptions = WriteOptions;

    const NAME: &'static str = "cmx_3600";
    const SUFFIXES: &'static [&'static str] = &["edl"];

    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document> {
        Self::read_from_str(std::str::from_utf8(input)?, options)
    }

    fn write_to_bytes(document: &Document, options: &Self::WriteOptions) -> Result<Vec<u8>> {
        Self::write_to_string(document, options).map(String::into_bytes)
    }
}

impl TextAdapter for Cmx3600 {
    fn read_from_str(input: &str, options: &Self::ReadOptions) -> Result<Document> {
        read::read(input, options)
    }

    fn write_to_string(document: &Document, options: &Self::WriteOptions) -> Result<String> {
        write::write(document, options)
    }
}

#[cfg(test)]
mod tests {
    use super::Style;
    use std::str::FromStr as _;

    #[test]
    fn a_dialect_round_trips_through_its_name() {
        for style in [Style::Avid, Style::Nucoda, Style::Premiere] {
            assert_eq!(Style::from_str(style.as_str()).expect("known"), style);
        }
    }

    #[test]
    fn an_unknown_dialect_is_refused() {
        let error = Style::from_str("bogus").expect_err("unknown");
        assert!(error.to_string().contains("bogus"));
    }
}

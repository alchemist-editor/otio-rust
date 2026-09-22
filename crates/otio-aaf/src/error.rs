//! What can go wrong turning an AAF file into a timeline, or a timeline into
//! an AAF file.

use std::fmt;

/// The result of reading an AAF as OTIO, or writing OTIO as an AAF.
pub type Result<T> = std::result::Result<T, Error>;

/// Why an AAF file could not be read as a timeline, or a timeline written as
/// one.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The file could not be read as an AAF at all.
    Aaf(aaf::Error),
    /// The file was read, but the timeline could not be built from it.
    Otio(otio_core::Error),
    /// The file could not be opened.
    Io(std::io::Error),
    /// The content storage holds a source mob where a composition belongs.
    ///
    /// A source mob describes essence rather than an edit, so reaching one
    /// from the top of the walk means the file's structure is not what the
    /// format says it should be.
    UnexpectedSourceMob,
    /// A component's length disagrees with the range transcribed for it.
    ///
    /// Both come from the same file, so they disagreeing means this crate
    /// built the range wrongly rather than that the file is odd.
    WrongDuration {
        /// The duration the transcribed item ended up with.
        found: f64,
        /// The length the component says it has.
        expected: i64,
    },
    /// The file describes something upstream's adapter refuses too.
    ///
    /// Each of these is a case upstream raises an error or fails on, such as
    /// a muted selector with other than one alternate.
    Malformed(&'static str),
    /// The AAF being written could not take what it was given.
    ///
    /// pyaaf2 checks each value against the type its property declares, and
    /// so does the writer underneath this crate. A value upstream's adapter
    /// would have handed pyaaf2 and had refused ends up here.
    Write(aaf::Error),
    /// The timeline holds something upstream's writer does not support: a
    /// top level that is not a timeline, a track that is neither video nor
    /// audio, or a generator other than slug.
    Unsupported(String),
    /// The timeline lacks what an AAF composition needs, as upstream's
    /// `validate_metadata` checks it: a rate every item agrees on, media
    /// with a known extent, and on each transition the AAF metadata the
    /// reader leaves there.
    ///
    /// Each message names one item and what it lacks.
    Invalid(Vec<String>),
    /// The timeline is one upstream's writer fails on for another reason:
    /// a clip with no MobID to use, say, or a value of a kind it cannot
    /// store.
    Unwritable(String),
    /// A clip's media was to be embedded, and there is no file where its
    /// URL points: upstream's `FileNotFoundError`.
    MissingEssence {
        /// The path upstream makes of the media's URL.
        path: String,
    },
    /// A clip's media could not be embedded, for one of the reasons
    /// upstream's writer raises `AAFAdapterError` for: a file of a kind it
    /// cannot embed, or an AAF without the master mob the clip names or
    /// the essence behind it.
    ///
    /// The message is upstream's.
    Embed(String),
    /// A clip on an audio track has a `.dnx` or `.wav` file to embed.
    ///
    /// Upstream imports media only on video tracks: the audio transcriber
    /// has no import of its own, and the one it inherits returns nothing,
    /// so writing fails with a `TypeError`.
    EmbedOnAudioTrack {
        /// The path upstream makes of the media's URL.
        path: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aaf(error) => write!(f, "this is not a readable AAF: {error}"),
            Self::Otio(error) => write!(f, "the timeline could not be built: {error}"),
            Self::Io(error) => write!(f, "the file could not be read: {error}"),
            Self::UnexpectedSourceMob => {
                write!(f, "a source mob sits where a composition should be")
            }
            Self::WrongDuration { found, expected } => {
                write!(f, "a duration of {found} should have been {expected}")
            }
            Self::Malformed(what) => write!(f, "the file cannot be read as an edit: {what}"),
            Self::Write(error) => write!(f, "the AAF could not be written: {error}"),
            Self::Unsupported(what) => {
                write!(f, "the timeline cannot be written as an AAF: {what}")
            }
            Self::Invalid(problems) => write!(
                f,
                "the timeline lacks what an AAF needs:\n{}",
                problems.join("\n")
            ),
            Self::Unwritable(what) => write!(f, "the timeline could not be written: {what}"),
            Self::MissingEssence { path } => {
                write!(f, "Cannot find file to embed essence from: '{path}'")
            }
            Self::Embed(what) => f.write_str(what),
            Self::EmbedOnAudioTrack { path } => write!(
                f,
                "cannot embed '{path}' on an audio track: upstream imports essence \
                 only on video tracks, and fails with \
                 'cannot unpack non-iterable NoneType object'"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Aaf(error) | Self::Write(error) => Some(error),
            Self::Otio(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<aaf::Error> for Error {
    fn from(error: aaf::Error) -> Self {
        Self::Aaf(error)
    }
}

impl From<otio_core::Error> for Error {
    fn from(error: otio_core::Error) -> Self {
        Self::Otio(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

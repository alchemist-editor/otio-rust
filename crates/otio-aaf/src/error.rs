//! What can go wrong turning an AAF file into a timeline.

use std::fmt;

/// The result of reading an AAF as OTIO.
pub type Result<T> = std::result::Result<T, Error>;

/// Why an AAF file could not be read as a timeline.
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
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Aaf(error) => Some(error),
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

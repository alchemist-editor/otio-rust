//! The error every adapter reports.

use std::fmt;

/// Something went wrong reading or writing a file format.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The file could not be read from or written to disk.
    Io(std::io::Error),

    /// A text format's bytes were not valid UTF-8.
    ///
    /// Upstream's adapters read these formats with Python's default text
    /// encoding, so a file that is not UTF-8 fails there too, just later and
    /// less clearly.
    Encoding(std::str::Utf8Error),

    /// The document could not be built or traversed.
    Core(otio_core::Error),

    /// A timecode or time string could not be understood.
    Time(opentime::TimeError),

    /// The input was not valid for this format.
    Parse {
        /// What was wrong.
        message: String,
        /// The line it was wrong on, counting from one, if known.
        line: Option<usize>,
    },

    /// The document holds something this format cannot express.
    ///
    /// An EDL carries a single video track, for instance, so writing a
    /// timeline with two of them has no answer.
    Unsupported {
        /// What the format cannot express.
        message: String,
    },
}

impl Error {
    /// Builds a [`Error::Parse`] that does not name a line.
    #[must_use]
    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse {
            message: message.into(),
            line: None,
        }
    }

    /// Builds a [`Error::Parse`] at a line, counting from one.
    #[must_use]
    pub fn parse_at(line: usize, message: impl Into<String>) -> Self {
        Self::Parse {
            message: message.into(),
            line: Some(line),
        }
    }

    /// Builds an [`Error::Unsupported`].
    #[must_use]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported {
            message: message.into(),
        }
    }

    /// Returns this error with `line` attached, if it is a parse error that
    /// does not already name one.
    ///
    /// This lets a parser work on a single line without knowing where that
    /// line came from, and have the caller that does know say so.
    #[must_use]
    pub fn at_line(self, line: usize) -> Self {
        match self {
            Self::Parse {
                message,
                line: None,
            } => Self::Parse {
                message,
                line: Some(line),
            },
            other => other,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Encoding(error) => write!(f, "the file is not valid UTF-8: {error}"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Time(error) => write!(f, "{error}"),
            Self::Parse {
                message,
                line: Some(line),
            } => write!(f, "line {line}: {message}"),
            Self::Parse {
                message,
                line: None,
            } => write!(f, "{message}"),
            Self::Unsupported { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Core(error) => Some(error),
            Self::Time(error) => Some(error),
            Self::Parse { .. } | Self::Unsupported { .. } => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(error: std::str::Utf8Error) -> Self {
        Self::Encoding(error)
    }
}

impl From<otio_core::Error> for Error {
    fn from(error: otio_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<opentime::TimeError> for Error {
    fn from(error: opentime::TimeError) -> Self {
        Self::Time(error)
    }
}

/// Shorthand for a result carrying an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

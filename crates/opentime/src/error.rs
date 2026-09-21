//! Errors produced by fallible time conversions.

use std::fmt;

/// An error produced by a fallible `opentime` conversion.
///
/// Upstream OpenTimelineIO reports these through an out-parameter
/// (`opentime::ErrorStatus`) whose `Outcome` enumeration this mirrors. Here the
/// same information is carried by `Result`, with the offending value attached
/// so the message can be rendered without the caller re-supplying context.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TimeError {
    /// The rate is not one of the rates SMPTE timecode is defined for.
    InvalidTimecodeRate {
        /// The rejected rate, in frames per second.
        rate: f64,
    },

    /// The timecode string is not of the form `HH:MM:SS:FF` (or `HH:MM:SS;FF`).
    InvalidTimecodeString {
        /// The rejected timecode.
        timecode: String,
    },

    /// The time string is not of the form `[-]HH:MM:SS.sss`.
    InvalidTimeString {
        /// The rejected time string.
        time_string: String,
    },

    /// The timecode carries a frame number the rate cannot represent.
    TimecodeRateMismatch {
        /// The rejected timecode.
        timecode: String,
        /// The highest frame number valid at this rate.
        max_frame: i32,
    },

    /// Timecode cannot represent a negative time.
    NegativeValue,

    /// Drop-frame timecode was requested at a rate that has no drop-frame form.
    ///
    /// Only 30000/1001 and 60000/1001 are drop-frame rates.
    InvalidRateForDropFrameTimecode {
        /// The rate that has no drop-frame form.
        rate: f64,
    },
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimecodeRate { rate } => {
                write!(f, "invalid timecode rate: {rate}")
            }
            Self::InvalidTimecodeString { timecode } => {
                write!(f, "invalid timecode string: '{timecode}'")
            }
            Self::InvalidTimeString { time_string } => {
                write!(f, "invalid time string: '{time_string}'")
            }
            Self::TimecodeRateMismatch {
                timecode,
                max_frame,
            } => write!(
                f,
                "frame rate mismatch: timecode '{timecode}' has frames beyond {max_frame}"
            ),
            Self::NegativeValue => {
                write!(f, "timecode cannot represent a negative value")
            }
            Self::InvalidRateForDropFrameTimecode { rate } => write!(
                f,
                "rate {rate} is not a valid drop frame rate; only 30000/1001 and 60000/1001 are"
            ),
        }
    }
}

impl std::error::Error for TimeError {}

/// Shorthand for a result carrying a [`TimeError`].
pub type Result<T> = std::result::Result<T, TimeError>;

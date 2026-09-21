//! Errors produced by fallible time conversions.

use std::fmt;

use crate::cfmt::format_g;

/// Why a time string was rejected.
///
/// Upstream renders both cases through one message shape,
/// `Error: '<string>' - <outcome>`, so the outcome travels alongside the
/// string rather than being a variant of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimeStringProblem {
    /// The rate is not one SMPTE timecode is defined for.
    Rate,

    /// The string is not of the form `[-]HH:MM:SS.sss`.
    Form,
}

impl fmt::Display for TimeStringProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Rate => "SMPTE timecode does not support this rate",
            Self::Form => "invalid time string",
        })
    }
}

/// An error produced by a fallible `opentime` conversion.
///
/// Upstream OpenTimelineIO reports these through an out-parameter
/// (`opentime::ErrorStatus`) whose `Outcome` enumeration this mirrors. Here the
/// same information is carried by `Result`, with the offending value attached
/// so the message can be rendered without the caller re-supplying context.
///
/// # On the wording
///
/// Each message below is upstream's, character for character, including the
/// two spaces after "mismatch." — upstream's bindings hand these strings
/// straight to Python as a `ValueError`, one of its tests compares one of them
/// exactly, and code in the wild matches on them. That makes the text part of
/// the observable behaviour rather than a detail, so it is reproduced rather
/// than improved.
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

    /// The time string is not of the form `[-]HH:MM:SS.sss`, or was offered at
    /// a rate timecode does not support.
    InvalidTimeString {
        /// The rejected time string.
        time_string: String,
        /// What was wrong with it.
        problem: TimeStringProblem,
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
        /// The timecode whose `;` divider asked for drop-frame form, where a
        /// timecode was being read. `None` where one was being written, which
        /// is the case upstream reports without naming a timecode.
        timecode: Option<String>,
    },
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimecodeRate { .. } => {
                f.write_str("SMPTE timecode does not support this rate")
            }
            Self::InvalidTimecodeString { timecode } => {
                write!(f, "Input timecode '{timecode}' is an invalid timecode")
            }
            Self::InvalidTimeString {
                time_string,
                problem,
            } => write!(f, "Error: '{time_string}' - {problem}"),
            Self::TimecodeRateMismatch {
                timecode,
                max_frame,
            } => write!(
                f,
                "Frame rate mismatch.  Timecode '{timecode}' has frames beyond {max_frame}"
            ),
            Self::NegativeValue => f.write_str("value cannot be negative here"),
            Self::InvalidRateForDropFrameTimecode {
                rate,
                timecode: Some(timecode),
            } => write!(
                f,
                "Timecode '{timecode}' indicates drop frame rate due to the ';' frame divider. \
                 Passed in rate {} is not a valid drop frame rate.",
                format_g(*rate, 6)
            ),
            Self::InvalidRateForDropFrameTimecode { .. } => {
                f.write_str("rate is not valid for drop frame timecode")
            }
        }
    }
}

impl std::error::Error for TimeError {}

/// Shorthand for a result carrying a [`TimeError`].
pub type Result<T> = std::result::Result<T, TimeError>;

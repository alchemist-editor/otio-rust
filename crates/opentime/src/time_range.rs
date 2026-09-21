//! A span of time, expressed as a start time and a duration.

use std::fmt;

use crate::rational_time::{RationalTime, max, min};

/// The default tolerance, in seconds, for the time range relations.
///
/// Twice 192kHz, the fastest commonly used audio rate, giving a resolution of
/// half a sample at 192kHz.
pub const DEFAULT_EPSILON_S: f64 = 1.0 / (2.0 * 192_000.0);

/// A span of time, inclusive of its start and exclusive of its end.
///
/// A range may be constructed with a negative duration, but the relations
/// below are written as though the duration is positive and are not meaningful
/// otherwise.
#[derive(Debug, Clone, Copy, Default)]
pub struct TimeRange {
    start_time: RationalTime,
    duration: RationalTime,
}

impl TimeRange {
    /// Construct a range from a start time and a duration.
    #[must_use]
    pub const fn new(start_time: RationalTime, duration: RationalTime) -> Self {
        Self {
            start_time,
            duration,
        }
    }

    /// Construct a zero-length range at `start_time`.
    #[must_use]
    pub fn at(start_time: RationalTime) -> Self {
        Self::new(start_time, RationalTime::new(0.0, start_time.rate()))
    }

    /// Construct a range from a start, a duration and a rate shared by both.
    #[must_use]
    pub fn from_values(start_time: f64, duration: f64, rate: f64) -> Self {
        Self::new(
            RationalTime::new(start_time, rate),
            RationalTime::new(duration, rate),
        )
    }

    /// Construct a range spanning from `start_time` up to, but excluding,
    /// `end_time_exclusive`.
    #[must_use]
    pub fn range_from_start_end_time(
        start_time: RationalTime,
        end_time_exclusive: RationalTime,
    ) -> Self {
        Self::new(
            start_time,
            RationalTime::duration_from_start_end_time(start_time, end_time_exclusive),
        )
    }

    /// Construct a range spanning from `start_time` through
    /// `end_time_inclusive`.
    #[must_use]
    pub fn range_from_start_end_time_inclusive(
        start_time: RationalTime,
        end_time_inclusive: RationalTime,
    ) -> Self {
        Self::new(
            start_time,
            RationalTime::duration_from_start_end_time_inclusive(start_time, end_time_inclusive),
        )
    }

    /// Returns the start time.
    #[must_use]
    pub const fn start_time(self) -> RationalTime {
        self.start_time
    }

    /// Returns the duration.
    #[must_use]
    pub const fn duration(self) -> RationalTime {
        self.duration
    }

    /// Returns true if either endpoint is invalid, or the duration is negative.
    #[must_use]
    pub fn is_invalid_range(self) -> bool {
        self.start_time.is_invalid_time()
            || self.duration.is_invalid_time()
            || self.duration.value() < 0.0
    }

    /// Returns true if both endpoints are valid and the duration is not
    /// negative.
    #[must_use]
    pub fn is_valid_range(self) -> bool {
        self.start_time.is_valid_time()
            && self.duration.is_valid_time()
            && self.duration.value() >= 0.0
    }

    /// Returns the first instant after the end of this range.
    #[must_use]
    pub fn end_time_exclusive(self) -> RationalTime {
        self.duration + self.start_time.rescaled_to_time(self.duration)
    }

    /// Returns the last instant within this range.
    #[must_use]
    pub fn end_time_inclusive(self) -> RationalTime {
        let end = self.end_time_exclusive();

        if (end - self.start_time.rescaled_to_time(self.duration)).value() > 1.0 {
            if self.duration.value() == self.duration.value().floor() {
                end - RationalTime::new(1.0, self.duration.rate())
            } else {
                // A fractional duration ends partway through a frame, so the
                // last whole instant is the floor of the exclusive end.
                end.floor()
            }
        } else {
            self.start_time
        }
    }

    /// Returns this range with its duration extended by `other`.
    #[must_use]
    pub fn duration_extended_by(self, other: RationalTime) -> Self {
        Self::new(self.start_time, self.duration + other)
    }

    /// Returns the smallest range covering both this range and `other`.
    #[must_use]
    pub fn extended_by(self, other: Self) -> Self {
        let new_start_time = min(self.start_time, other.start_time);
        let new_end_time = max(self.end_time_exclusive(), other.end_time_exclusive());
        Self::new(
            new_start_time,
            RationalTime::duration_from_start_end_time(new_start_time, new_end_time),
        )
    }

    /// Returns `other` clamped into this range.
    #[must_use]
    pub fn clamped_time(self, other: RationalTime) -> RationalTime {
        min(max(other, self.start_time), self.end_time_inclusive())
    }

    /// Returns `other` clamped into this range.
    #[must_use]
    pub fn clamped_range(self, other: Self) -> Self {
        let clamped = Self::new(max(other.start_time, self.start_time), other.duration);
        let end = min(clamped.end_time_exclusive(), self.end_time_exclusive());
        Self::new(clamped.start_time, end - clamped.start_time)
    }

    /// Returns whether `other` falls within this range.
    ///
    /// ```text
    ///                    other
    ///                      |
    ///              [      this      ]
    /// ```
    #[must_use]
    pub fn contains_time(self, other: RationalTime) -> bool {
        self.start_time <= other && other < self.end_time_exclusive()
    }

    /// Returns whether `other` falls strictly within this range.
    ///
    /// ```text
    ///                   [ other ]
    ///              [      this      ]
    /// ```
    #[must_use]
    pub fn contains_range(self, other: Self, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        greater_than(other_start, this_start, epsilon_s)
            && lesser_than(other_end, this_end, epsilon_s)
    }

    /// Returns whether `other` falls within this range.
    ///
    /// An alias of [`TimeRange::contains_time`], matching upstream.
    #[must_use]
    pub fn overlaps_time(self, other: RationalTime) -> bool {
        self.contains_time(other)
    }

    /// Returns whether this range overlaps the start of `other`.
    ///
    /// ```text
    ///              [ this ]
    ///                  [ other ]
    /// ```
    #[must_use]
    pub fn overlaps_range(self, other: Self, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        lesser_than(this_start, other_start, epsilon_s)
            && greater_than(this_end, other_start, epsilon_s)
            && greater_than(other_end, this_end, epsilon_s)
    }

    /// Returns whether this range ends before `other` begins.
    ///
    /// ```text
    ///              [ this ]    [ other ]
    /// ```
    #[must_use]
    pub fn before_range(self, other: Self, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        greater_than(other_start, this_end, epsilon_s)
    }

    /// Returns whether this range ends before `other`.
    ///
    /// ```text
    ///                        other
    ///                          |
    ///              [ this ]    *
    /// ```
    #[must_use]
    pub fn before_time(self, other: RationalTime, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        lesser_than(this_end, other.to_seconds(), epsilon_s)
    }

    /// Returns whether this range ends exactly where `other` begins.
    ///
    /// ```text
    ///              [this][other]
    /// ```
    #[must_use]
    pub fn meets(self, other: Self, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        other_start - this_end <= epsilon_s && other_start - this_end >= 0.0
    }

    /// Returns whether this range starts with `other` and ends before it does.
    ///
    /// ```text
    ///              [ this ]
    ///              [    other    ]
    /// ```
    #[must_use]
    pub fn begins_range(self, other: Self, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        (other_start - this_start).abs() <= epsilon_s && lesser_than(this_end, other_end, epsilon_s)
    }

    /// Returns whether this range begins at `other`.
    ///
    /// ```text
    ///            other
    ///              |
    ///              [ this ]
    /// ```
    #[must_use]
    pub fn begins_time(self, other: RationalTime, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        (other.to_seconds() - this_start).abs() <= epsilon_s
    }

    /// Returns whether this range ends with `other` and begins after it does.
    ///
    /// ```text
    ///                      [ this ]
    ///              [     other    ]
    /// ```
    #[must_use]
    pub fn finishes_range(self, other: Self, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        (this_end - other_end).abs() <= epsilon_s
            && greater_than(this_start, other_start, epsilon_s)
    }

    /// Returns whether this range ends at `other`.
    ///
    /// ```text
    ///                   other
    ///                     |
    ///              [ this ]
    /// ```
    #[must_use]
    pub fn finishes_time(self, other: RationalTime, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        (this_end - other.to_seconds()).abs() <= epsilon_s
    }

    /// Returns whether this range and `other` share any span of time.
    ///
    /// ```text
    ///         [    this    ]        OR      [    other    ]
    ///              [     other    ]                [     this    ]
    /// ```
    #[must_use]
    pub fn intersects(self, other: Self, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        lesser_than(this_start, other_end, epsilon_s)
            && greater_than(this_end, other_start, epsilon_s)
    }
}

/// Returns whether `lhs` exceeds `rhs` by at least `epsilon`.
fn greater_than(lhs: f64, rhs: f64, epsilon: f64) -> bool {
    lhs - rhs >= epsilon
}

/// Returns whether `rhs` exceeds `lhs` by at least `epsilon`.
fn lesser_than(lhs: f64, rhs: f64, epsilon: f64) -> bool {
    rhs - lhs >= epsilon
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TimeRange({}, {})", self.start_time, self.duration)
    }
}

impl PartialEq for TimeRange {
    /// Compares start and duration in seconds, within [`DEFAULT_EPSILON_S`].
    fn eq(&self, other: &Self) -> bool {
        let start = self.start_time - other.start_time;
        let duration = self.duration - other.duration;
        start.to_seconds().abs() < DEFAULT_EPSILON_S
            && duration.to_seconds().abs() < DEFAULT_EPSILON_S
    }
}

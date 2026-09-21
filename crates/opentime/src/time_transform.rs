//! A one-dimensional transform of time: offset, scale and rate.

use std::fmt;

use crate::rational_time::RationalTime;
use crate::time_range::TimeRange;

/// A one-dimensional transform applied to times and ranges.
///
/// A rate of -1 means "keep whatever rate the input had".
#[derive(Debug, Clone, Copy)]
pub struct TimeTransform {
    offset: RationalTime,
    scale: f64,
    rate: f64,
}

impl Default for TimeTransform {
    /// The identity transform: no offset, unit scale, and no rate change.
    fn default() -> Self {
        Self {
            offset: RationalTime::new(0.0, 1.0),
            scale: 1.0,
            rate: -1.0,
        }
    }
}

impl TimeTransform {
    /// Construct a transform from an offset, a scale and a rate.
    ///
    /// Pass a rate of -1 to leave the input's rate unchanged.
    #[must_use]
    pub const fn new(offset: RationalTime, scale: f64, rate: f64) -> Self {
        Self {
            offset,
            scale,
            rate,
        }
    }

    /// Returns the offset.
    #[must_use]
    pub const fn offset(self) -> RationalTime {
        self.offset
    }

    /// Returns the scale.
    #[must_use]
    pub const fn scale(self) -> f64 {
        self.scale
    }

    /// Returns the rate, or -1 if the transform does not change the rate.
    #[must_use]
    pub const fn rate(self) -> f64 {
        self.rate
    }

    /// Applies this transform to a time.
    #[must_use]
    pub fn applied_to_time(self, other: RationalTime) -> RationalTime {
        let result = RationalTime::new(other.value() * self.scale, other.rate()) + self.offset;
        let target_rate = if self.rate > 0.0 {
            self.rate
        } else {
            other.rate()
        };
        if target_rate > 0.0 {
            result.rescaled_to(target_rate)
        } else {
            result
        }
    }

    /// Applies this transform to a range.
    #[must_use]
    pub fn applied_to_range(self, other: TimeRange) -> TimeRange {
        TimeRange::range_from_start_end_time(
            self.applied_to_time(other.start_time()),
            self.applied_to_time(other.end_time_exclusive()),
        )
    }

    /// Composes this transform with another.
    #[must_use]
    pub fn applied_to_transform(self, other: Self) -> Self {
        Self::new(
            self.offset + other.offset,
            self.scale * other.scale,
            if self.rate > 0.0 {
                self.rate
            } else {
                other.rate
            },
        )
    }
}

impl fmt::Display for TimeTransform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TimeTransform({}, {}, {})",
            self.offset, self.scale, self.rate
        )
    }
}

impl PartialEq for TimeTransform {
    fn eq(&self, other: &Self) -> bool {
        self.offset == other.offset && self.scale == other.scale && self.rate == other.rate
    }
}

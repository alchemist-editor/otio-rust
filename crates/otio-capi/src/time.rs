//! `opentime` for C: times, ranges and transforms as plain structs.
//!
//! A `RationalTime` is two doubles and a `TimeRange` is two of those, so they
//! cross the boundary by value rather than behind a handle. Everything here
//! is a thin call onto the `opentime` crate; where a function can fail it
//! fails for the same reason upstream OpenTimelineIO's does, and the message
//! it writes to `out_error` carries upstream's wording.

use std::ffi::c_char;

use opentime::{DropFrame, RationalTime, TimeRange, TimeTransform};

use crate::buffer::OtioBuffer;
use crate::handle::{text, write_out};
use crate::status::{OtioStatus, guard, guard_value};

/// A measure of time, as a `value` in units of `1 / rate` seconds.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioRationalTime {
    /// How many units.
    pub value: f64,
    /// How many units make a second.
    pub rate: f64,
}

/// A span of time: where it starts and how long it lasts.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioTimeRange {
    /// Where the span begins.
    pub start_time: OtioRationalTime,
    /// How long it lasts.
    pub duration: OtioRationalTime,
}

/// An offset, a speed change and a rate change, applied together.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioTimeTransform {
    /// How far to shift.
    pub offset: OtioRationalTime,
    /// How much to stretch: 2.0 plays twice as fast.
    pub scale: f64,
    /// The rate to resolve to, or a negative number to keep the input's.
    pub rate: f64,
}

/// Whether a timecode is written in drop-frame form.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioDropFrame {
    /// Use drop-frame form if the rate is a drop-frame rate.
    InferFromRate = 0,
    /// Never use drop-frame form.
    ForceNo = 1,
    /// Use drop-frame form, failing if the rate has none.
    ForceYes = 2,
}

impl From<OtioRationalTime> for RationalTime {
    fn from(time: OtioRationalTime) -> Self {
        Self::new(time.value, time.rate)
    }
}

impl From<RationalTime> for OtioRationalTime {
    fn from(time: RationalTime) -> Self {
        Self {
            value: time.value(),
            rate: time.rate(),
        }
    }
}

impl From<OtioTimeRange> for TimeRange {
    fn from(range: OtioTimeRange) -> Self {
        Self::new(range.start_time.into(), range.duration.into())
    }
}

impl From<TimeRange> for OtioTimeRange {
    fn from(range: TimeRange) -> Self {
        Self {
            start_time: range.start_time().into(),
            duration: range.duration().into(),
        }
    }
}

impl From<OtioTimeTransform> for TimeTransform {
    fn from(transform: OtioTimeTransform) -> Self {
        Self::new(transform.offset.into(), transform.scale, transform.rate)
    }
}

impl From<TimeTransform> for OtioTimeTransform {
    fn from(transform: TimeTransform) -> Self {
        Self {
            offset: transform.offset().into(),
            scale: transform.scale(),
            rate: transform.rate(),
        }
    }
}

impl From<OtioDropFrame> for DropFrame {
    fn from(drop_frame: OtioDropFrame) -> Self {
        match drop_frame {
            OtioDropFrame::InferFromRate => Self::InferFromRate,
            OtioDropFrame::ForceNo => Self::ForceNo,
            OtioDropFrame::ForceYes => Self::ForceYes,
        }
    }
}

/// The tolerance, in seconds, that the range predicates use by default.
///
/// Upstream's C++ takes this as a default argument; C has none, so the value
/// is a call rather than a constant in the header, which keeps the two from
/// drifting apart.
#[unsafe(no_mangle)]
pub extern "C" fn otio_default_epsilon_s() -> f64 {
    opentime::DEFAULT_EPSILON_S
}

// ---------------------------------------------------------------------------
// RationalTime
// ---------------------------------------------------------------------------

/// Returns whether a time is usable: both parts finite and the rate positive.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_is_valid(time: OtioRationalTime) -> bool {
    guard_value(false, || RationalTime::from(time).is_valid_time())
}

/// Returns the same instant expressed at another rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_rescaled_to(
    time: OtioRationalTime,
    rate: f64,
) -> OtioRationalTime {
    guard_value(time, || RationalTime::from(time).rescaled_to(rate).into())
}

/// Returns the same instant expressed at another time's rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_rescaled_to_time(
    time: OtioRationalTime,
    other: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(time, || {
        RationalTime::from(time)
            .rescaled_to_time(other.into())
            .into()
    })
}

/// Returns what this time's value would be at another rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_value_rescaled_to(time: OtioRationalTime, rate: f64) -> f64 {
    guard_value(f64::NAN, || {
        RationalTime::from(time).value_rescaled_to(rate)
    })
}

/// Returns whether two times are within `delta` of each other.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_almost_equal(
    left: OtioRationalTime,
    right: OtioRationalTime,
    delta: f64,
) -> bool {
    guard_value(false, || {
        RationalTime::from(left).almost_equal(right.into(), delta)
    })
}

/// Returns whether two times are the same instant.
///
/// Times at different rates that name the same instant compare equal; this is
/// upstream's `==`, not a field-by-field comparison. For that, use
/// [`otio_rational_time_strictly_equal`].
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_equal(
    left: OtioRationalTime,
    right: OtioRationalTime,
) -> bool {
    guard_value(false, || {
        RationalTime::from(left) == RationalTime::from(right)
    })
}

/// Returns whether two times have the same value and the same rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_strictly_equal(
    left: OtioRationalTime,
    right: OtioRationalTime,
) -> bool {
    guard_value(false, || {
        RationalTime::from(left).strictly_equal(right.into())
    })
}

/// Orders two times: -1 if `left` is earlier, 1 if later, 0 if the same.
///
/// A comparison involving a NaN has no answer, and reports 0.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_compare(
    left: OtioRationalTime,
    right: OtioRationalTime,
) -> i32 {
    guard_value(0, || {
        match RationalTime::from(left).partial_cmp(&RationalTime::from(right)) {
            Some(std::cmp::Ordering::Less) => -1,
            Some(std::cmp::Ordering::Greater) => 1,
            _ => 0,
        }
    })
}

/// Returns the sum of two times, at the higher of the two rates.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_add(
    left: OtioRationalTime,
    right: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(left, || {
        (RationalTime::from(left) + RationalTime::from(right)).into()
    })
}

/// Returns the difference of two times, at the higher of the two rates.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_subtract(
    left: OtioRationalTime,
    right: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(left, || {
        (RationalTime::from(left) - RationalTime::from(right)).into()
    })
}

/// Returns the time with its sign flipped.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_negate(time: OtioRationalTime) -> OtioRationalTime {
    guard_value(time, || (-RationalTime::from(time)).into())
}

/// Returns the time rounded towards negative infinity.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_floor(time: OtioRationalTime) -> OtioRationalTime {
    guard_value(time, || RationalTime::from(time).floor().into())
}

/// Returns the time rounded towards positive infinity.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_ceil(time: OtioRationalTime) -> OtioRationalTime {
    guard_value(time, || RationalTime::from(time).ceil().into())
}

/// Returns the time rounded to the nearest whole value, halves away from zero.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_round(time: OtioRationalTime) -> OtioRationalTime {
    guard_value(time, || RationalTime::from(time).round().into())
}

/// Returns how long it is from one instant to another, the end excluded.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_duration_from_start_end_time(
    start_time: OtioRationalTime,
    end_time_exclusive: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(start_time, || {
        RationalTime::duration_from_start_end_time(start_time.into(), end_time_exclusive.into())
            .into()
    })
}

/// Returns how long it is from one instant to another, the end included.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_duration_from_start_end_time_inclusive(
    start_time: OtioRationalTime,
    end_time_inclusive: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(start_time, || {
        RationalTime::duration_from_start_end_time_inclusive(
            start_time.into(),
            end_time_inclusive.into(),
        )
        .into()
    })
}

/// Builds a time from a frame number at a rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_from_frames(frame: f64, rate: f64) -> OtioRationalTime {
    guard_value(
        OtioRationalTime {
            value: 0.0,
            rate: 1.0,
        },
        || RationalTime::from_frames(frame, rate).into(),
    )
}

/// Builds a time from a number of seconds at a rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_from_seconds_at_rate(
    seconds: f64,
    rate: f64,
) -> OtioRationalTime {
    guard_value(
        OtioRationalTime {
            value: 0.0,
            rate: 1.0,
        },
        || RationalTime::from_seconds_at_rate(seconds, rate).into(),
    )
}

/// Builds a time from a number of seconds, at a rate of one.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_from_seconds(seconds: f64) -> OtioRationalTime {
    guard_value(
        OtioRationalTime {
            value: 0.0,
            rate: 1.0,
        },
        || RationalTime::from_seconds(seconds).into(),
    )
}

/// Returns the frame number this time falls on.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_to_frames(time: OtioRationalTime) -> i32 {
    guard_value(0, || RationalTime::from(time).to_frames())
}

/// Returns the frame number this time falls on at another rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_to_frames_at_rate(time: OtioRationalTime, rate: f64) -> i32 {
    guard_value(0, || RationalTime::from(time).to_frames_at_rate(rate))
}

/// Returns the time in seconds.
#[unsafe(no_mangle)]
pub extern "C" fn otio_rational_time_to_seconds(time: OtioRationalTime) -> f64 {
    guard_value(f64::NAN, || RationalTime::from(time).to_seconds())
}

/// Returns whether a rate is one SMPTE timecode is defined for.
#[unsafe(no_mangle)]
pub extern "C" fn otio_is_smpte_timecode_rate(rate: f64) -> bool {
    guard_value(false, || RationalTime::is_smpte_timecode_rate(rate))
}

/// Returns the SMPTE timecode rate closest to a rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_nearest_smpte_timecode_rate(rate: f64) -> f64 {
    guard_value(rate, || RationalTime::nearest_smpte_timecode_rate(rate))
}

/// Returns whether a rate is a drop-frame rate.
#[unsafe(no_mangle)]
pub extern "C" fn otio_is_drop_frame_rate(rate: f64) -> bool {
    guard_value(false, || RationalTime::is_drop_frame_rate(rate))
}

/// Reads a time from a `HH:MM:SS:FF` timecode at a rate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_rational_time_from_timecode(
    timecode: *const c_char,
    rate: f64,
    out_time: *mut OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let timecode = unsafe { text(timecode, "timecode") }?;
        let time = RationalTime::from_timecode(timecode, rate)?;
        unsafe { write_out(out_time, time.into(), "out_time") }
    })
}

/// Reads a time from a `[-]HH:MM:SS.sss` time string at a rate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_rational_time_from_time_string(
    time_string: *const c_char,
    rate: f64,
    out_time: *mut OtioRationalTime,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let time_string = unsafe { text(time_string, "time_string") }?;
        let time = RationalTime::from_time_string(time_string, rate)?;
        unsafe { write_out(out_time, time.into(), "out_time") }
    })
}

/// Writes a time as a timecode at its own rate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_rational_time_to_timecode(
    time: OtioRationalTime,
    out_timecode: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let timecode = RationalTime::from(time).to_timecode()?;
        unsafe {
            write_out(
                out_timecode,
                OtioBuffer::from_str(&timecode),
                "out_timecode",
            )
        }
    })
}

/// Writes a time as a timecode at a given rate and drop-frame setting.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_rational_time_to_timecode_at(
    time: OtioRationalTime,
    rate: f64,
    drop_frame: OtioDropFrame,
    out_timecode: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let timecode = RationalTime::from(time).to_timecode_at(rate, drop_frame.into())?;
        unsafe {
            write_out(
                out_timecode,
                OtioBuffer::from_str(&timecode),
                "out_timecode",
            )
        }
    })
}

/// Writes a time as a timecode, rounding to the nearest frame first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_rational_time_to_nearest_timecode_at(
    time: OtioRationalTime,
    rate: f64,
    drop_frame: OtioDropFrame,
    out_timecode: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let timecode = RationalTime::from(time).to_nearest_timecode_at(rate, drop_frame.into())?;
        unsafe {
            write_out(
                out_timecode,
                OtioBuffer::from_str(&timecode),
                "out_timecode",
            )
        }
    })
}

/// Writes a time as a `HH:MM:SS.sss` time string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_rational_time_to_time_string(
    time: OtioRationalTime,
    out_string: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let value = RationalTime::from(time).to_time_string();
        unsafe { write_out(out_string, OtioBuffer::from_str(&value), "out_string") }
    })
}

// ---------------------------------------------------------------------------
// TimeRange
// ---------------------------------------------------------------------------

/// Builds a range from its start and the instant after its end.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_from_start_end_time(
    start_time: OtioRationalTime,
    end_time_exclusive: OtioRationalTime,
) -> OtioTimeRange {
    guard_value(
        OtioTimeRange {
            start_time,
            duration: start_time,
        },
        || {
            TimeRange::range_from_start_end_time(start_time.into(), end_time_exclusive.into())
                .into()
        },
    )
}

/// Builds a range from its start and its last instant.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_from_start_end_time_inclusive(
    start_time: OtioRationalTime,
    end_time_inclusive: OtioRationalTime,
) -> OtioTimeRange {
    guard_value(
        OtioTimeRange {
            start_time,
            duration: start_time,
        },
        || {
            TimeRange::range_from_start_end_time_inclusive(
                start_time.into(),
                end_time_inclusive.into(),
            )
            .into()
        },
    )
}

/// Returns whether a range is usable: valid times and a duration of at least
/// zero.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_is_valid(range: OtioTimeRange) -> bool {
    guard_value(false, || TimeRange::from(range).is_valid_range())
}

/// Returns the instant just after the range's end.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_end_time_exclusive(range: OtioTimeRange) -> OtioRationalTime {
    guard_value(range.start_time, || {
        TimeRange::from(range).end_time_exclusive().into()
    })
}

/// Returns the last instant the range covers.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_end_time_inclusive(range: OtioTimeRange) -> OtioRationalTime {
    guard_value(range.start_time, || {
        TimeRange::from(range).end_time_inclusive().into()
    })
}

/// Returns the range with its duration lengthened.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_duration_extended_by(
    range: OtioTimeRange,
    by: OtioRationalTime,
) -> OtioTimeRange {
    guard_value(range, || {
        TimeRange::from(range)
            .duration_extended_by(by.into())
            .into()
    })
}

/// Returns the smallest range covering both of two ranges.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_extended_by(
    range: OtioTimeRange,
    other: OtioTimeRange,
) -> OtioTimeRange {
    guard_value(range, || {
        TimeRange::from(range).extended_by(other.into()).into()
    })
}

/// Returns the instant pulled inside the range, if it lies outside it.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_clamped_time(
    range: OtioTimeRange,
    time: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(time, || {
        TimeRange::from(range).clamped_time(time.into()).into()
    })
}

/// Returns the range pulled inside this one, where it lies outside it.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_clamped_range(
    range: OtioTimeRange,
    other: OtioTimeRange,
) -> OtioTimeRange {
    guard_value(other, || {
        TimeRange::from(range).clamped_range(other.into()).into()
    })
}

/// Returns whether an instant falls inside the range, the end excluded.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_contains_time(
    range: OtioTimeRange,
    time: OtioRationalTime,
) -> bool {
    guard_value(false, || TimeRange::from(range).contains_time(time.into()))
}

/// Returns whether another range falls entirely inside this one.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_contains_range(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).contains_range(other.into(), epsilon_s)
    })
}

/// Returns whether an instant falls inside the range, the ends included.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_overlaps_time(
    range: OtioTimeRange,
    time: OtioRationalTime,
) -> bool {
    guard_value(false, || TimeRange::from(range).overlaps_time(time.into()))
}

/// Returns whether two ranges share any time at all.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_overlaps_range(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).overlaps_range(other.into(), epsilon_s)
    })
}

/// Returns whether this range ends before another begins.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_before_range(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).before_range(other.into(), epsilon_s)
    })
}

/// Returns whether this range ends before an instant.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_before_time(
    range: OtioTimeRange,
    time: OtioRationalTime,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).before_time(time.into(), epsilon_s)
    })
}

/// Returns whether this range ends exactly where another begins.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_meets(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).meets(other.into(), epsilon_s)
    })
}

/// Returns whether another range starts where this one does.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_begins_range(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).begins_range(other.into(), epsilon_s)
    })
}

/// Returns whether this range starts at an instant.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_begins_time(
    range: OtioTimeRange,
    time: OtioRationalTime,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).begins_time(time.into(), epsilon_s)
    })
}

/// Returns whether another range ends where this one does.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_finishes_range(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).finishes_range(other.into(), epsilon_s)
    })
}

/// Returns whether this range ends at an instant.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_finishes_time(
    range: OtioTimeRange,
    time: OtioRationalTime,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).finishes_time(time.into(), epsilon_s)
    })
}

/// Returns whether two ranges share more than a single boundary instant.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_range_intersects(
    range: OtioTimeRange,
    other: OtioTimeRange,
    epsilon_s: f64,
) -> bool {
    guard_value(false, || {
        TimeRange::from(range).intersects(other.into(), epsilon_s)
    })
}

// ---------------------------------------------------------------------------
// TimeTransform
// ---------------------------------------------------------------------------

/// Returns the transform applied to an instant.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_transform_applied_to_time(
    transform: OtioTimeTransform,
    time: OtioRationalTime,
) -> OtioRationalTime {
    guard_value(time, || {
        TimeTransform::from(transform)
            .applied_to_time(time.into())
            .into()
    })
}

/// Returns the transform applied to a span.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_transform_applied_to_range(
    transform: OtioTimeTransform,
    range: OtioTimeRange,
) -> OtioTimeRange {
    guard_value(range, || {
        TimeTransform::from(transform)
            .applied_to_range(range.into())
            .into()
    })
}

/// Returns the transform applied to another transform.
#[unsafe(no_mangle)]
pub extern "C" fn otio_time_transform_applied_to_transform(
    transform: OtioTimeTransform,
    other: OtioTimeTransform,
) -> OtioTimeTransform {
    guard_value(other, || {
        TimeTransform::from(transform)
            .applied_to_transform(other.into())
            .into()
    })
}

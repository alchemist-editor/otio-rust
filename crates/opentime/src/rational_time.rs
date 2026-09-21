//! A measure of time as a value at a rate.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use crate::cfmt::format_g;
use crate::error::{Result, TimeError};

/// The frame rates SMPTE timecode is defined for.
///
/// From ST 12-1:2014, *SMPTE Standard - Time and Control Code*.
///
/// Note that upstream's C++ declares this array with room for eleven entries
/// but supplies ten, leaving a trailing `0.0`. That makes
/// `is_smpte_timecode_rate(0.0)` report true and lets a rate below ~11.988
/// snap to a nearest rate of zero. Both look unintended and neither is covered
/// by upstream's tests, so this port carries only the ten real rates.
const SMPTE_TIMECODE_RATES: [f64; 10] = [
    24000.0 / 1001.0,
    24.0,
    25.0,
    30000.0 / 1001.0,
    30.0,
    48000.0 / 1001.0,
    48.0,
    50.0,
    60000.0 / 1001.0,
    60.0,
];

/// The two rates that have a drop-frame timecode form.
const DROP_FRAME_TIMECODE_RATES: [f64; 2] = [30000.0 / 1001.0, 60000.0 / 1001.0];

/// Whether timecode should be rendered in drop-frame form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DropFrame {
    /// Use drop-frame form if the rate is a drop-frame rate.
    #[default]
    InferFromRate,
    /// Never use drop-frame form.
    ForceNo,
    /// Use drop-frame form, failing if the rate has none.
    ForceYes,
}

/// A measure of time, expressed as a `value` in units of `1 / rate` seconds.
///
/// A `RationalTime` of value 24 at rate 24 is one second; so is a value of 48
/// at rate 48. Arithmetic between times at different rates resolves to the
/// higher of the two rates, so no precision is lost.
///
/// # Comparison
///
/// `==` rescales the right-hand side to the left-hand rate before comparing,
/// so `RationalTime::new(24.0, 24.0) == RationalTime::new(48.0, 48.0)`. Use
/// [`RationalTime::strictly_equal`] to compare value and rate without
/// rescaling.
#[derive(Debug, Clone, Copy)]
pub struct RationalTime {
    value: f64,
    rate: f64,
}

impl Default for RationalTime {
    /// A time of zero at rate 1.
    fn default() -> Self {
        Self {
            value: 0.0,
            rate: 1.0,
        }
    }
}

impl RationalTime {
    /// Construct a time from a value and a rate.
    #[must_use]
    pub const fn new(value: f64, rate: f64) -> Self {
        Self { value, rate }
    }

    /// Returns the time value.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    /// Returns the time rate.
    #[must_use]
    pub const fn rate(self) -> f64 {
        self.rate
    }

    /// Returns true if the value or rate is NaN, or the rate is not positive.
    #[must_use]
    pub fn is_invalid_time(self) -> bool {
        !self.is_valid_time()
    }

    /// Returns true if the value and rate are numbers and the rate is positive.
    #[must_use]
    pub fn is_valid_time(self) -> bool {
        !self.rate.is_nan() && !self.value.is_nan() && self.rate > 0.0
    }

    /// Returns this time converted to `new_rate`.
    #[must_use]
    pub fn rescaled_to(self, new_rate: f64) -> Self {
        Self::new(self.value_rescaled_to(new_rate), new_rate)
    }

    /// Returns this time converted to the rate of `other`.
    #[must_use]
    pub fn rescaled_to_time(self, other: Self) -> Self {
        self.rescaled_to(other.rate)
    }

    /// Returns this time's value expressed at `new_rate`.
    ///
    /// Returns zero if this time's rate is not positive, matching upstream.
    #[must_use]
    pub fn value_rescaled_to(self, new_rate: f64) -> f64 {
        if new_rate == self.rate {
            self.value
        } else if self.rate > 0.0 {
            (self.value * new_rate) / self.rate
        } else {
            0.0
        }
    }

    /// Returns this time's value expressed at the rate of `other`.
    #[must_use]
    pub fn value_rescaled_to_time(self, other: Self) -> f64 {
        self.value_rescaled_to(other.rate)
    }

    /// Returns whether this time is within `delta` of `other`.
    ///
    /// The comparison is made at the rate of `other`.
    #[must_use]
    pub fn almost_equal(self, other: Self, delta: f64) -> bool {
        (self.value_rescaled_to(other.rate) - other.value).abs() <= delta
    }

    /// Returns whether this time's value *and* rate equal those of `other`.
    ///
    /// Unlike `==`, this does not rescale before comparing.
    #[must_use]
    pub fn strictly_equal(self, other: Self) -> bool {
        self.value == other.value && self.rate == other.rate
    }

    /// Returns this time with its value rounded down to an integer.
    #[must_use]
    pub fn floor(self) -> Self {
        Self::new(self.value.floor(), self.rate)
    }

    /// Returns this time with its value rounded up to an integer.
    #[must_use]
    pub fn ceil(self) -> Self {
        Self::new(self.value.ceil(), self.rate)
    }

    /// Returns this time with its value rounded to the nearest integer.
    ///
    /// Halfway cases round away from zero.
    #[must_use]
    pub fn round(self) -> Self {
        Self::new(self.value.round(), self.rate)
    }

    /// The duration of the samples from `start_time` up to, but excluding,
    /// `end_time_exclusive`.
    ///
    /// The duration of a clip from frame 10 to frame 15 is 5 frames. The result
    /// is at the rate of `start_time`.
    #[must_use]
    pub fn duration_from_start_end_time(start_time: Self, end_time_exclusive: Self) -> Self {
        if start_time.rate == end_time_exclusive.rate {
            Self::new(end_time_exclusive.value - start_time.value, start_time.rate)
        } else {
            Self::new(
                end_time_exclusive.value_rescaled_to_time(start_time) - start_time.value,
                start_time.rate,
            )
        }
    }

    /// The duration of the samples from `start_time` through
    /// `end_time_inclusive`.
    ///
    /// The duration of a clip from frame 10 to frame 15 is 6 frames. The result
    /// is at the rate of `start_time`.
    #[must_use]
    pub fn duration_from_start_end_time_inclusive(
        start_time: Self,
        end_time_inclusive: Self,
    ) -> Self {
        if start_time.rate == end_time_inclusive.rate {
            Self::new(
                end_time_inclusive.value - start_time.value + 1.0,
                start_time.rate,
            )
        } else {
            Self::new(
                end_time_inclusive.value_rescaled_to_time(start_time) - start_time.value + 1.0,
                start_time.rate,
            )
        }
    }

    /// Returns whether `rate` is a rate SMPTE timecode is defined for.
    #[must_use]
    pub fn is_smpte_timecode_rate(rate: f64) -> bool {
        SMPTE_TIMECODE_RATES.contains(&rate)
    }

    /// Returns the SMPTE timecode rate closest to `rate`.
    ///
    /// An exact match is returned unchanged. Ties go to the earlier rate in
    /// SMPTE order.
    #[must_use]
    pub fn nearest_smpte_timecode_rate(rate: f64) -> f64 {
        let mut nearest_rate = 0.0;
        let mut min_diff = f64::MAX;
        for smpte_rate in SMPTE_TIMECODE_RATES {
            if smpte_rate == rate {
                return rate;
            }
            let diff = (rate - smpte_rate).abs();
            if diff >= min_diff {
                continue;
            }
            min_diff = diff;
            nearest_rate = smpte_rate;
        }
        nearest_rate
    }

    /// Returns whether `rate` has a drop-frame timecode form.
    #[must_use]
    pub fn is_drop_frame_rate(rate: f64) -> bool {
        DROP_FRAME_TIMECODE_RATES.contains(&rate)
    }

    /// Construct a time from a frame number at `rate`.
    ///
    /// The frame number is truncated toward zero. Note that upstream truncates
    /// through a C `int`, so this saturates at `i32` bounds to match.
    #[must_use]
    pub fn from_frames(frame: f64, rate: f64) -> Self {
        Self::new(f64::from(frame as i32), rate)
    }

    /// Construct a time of `seconds` seconds, expressed at `rate`.
    #[must_use]
    pub fn from_seconds_at_rate(seconds: f64, rate: f64) -> Self {
        Self::new(seconds, 1.0).rescaled_to(rate)
    }

    /// Construct a time of `seconds` seconds at rate 1.
    #[must_use]
    pub fn from_seconds(seconds: f64) -> Self {
        Self::new(seconds, 1.0)
    }

    /// Returns the frame number at this time's own rate.
    #[must_use]
    pub fn to_frames(self) -> i32 {
        self.value as i32
    }

    /// Returns the frame number this time falls on at `rate`.
    #[must_use]
    pub fn to_frames_at_rate(self, rate: f64) -> i32 {
        self.value_rescaled_to(rate) as i32
    }

    /// Returns this time in seconds.
    #[must_use]
    pub fn to_seconds(self) -> f64 {
        self.value_rescaled_to(1.0)
    }

    /// Parse a SMPTE timecode string such as `"01:00:00:00"`.
    ///
    /// A `;` before the frames field marks drop-frame timecode, which is only
    /// valid at 30000/1001 and 60000/1001.
    ///
    /// # Errors
    ///
    /// Returns an error if `rate` is not a SMPTE rate, if the string is not
    /// four two-digit fields, if drop-frame is indicated at a rate that has no
    /// drop-frame form, or if the frames field exceeds what the rate allows.
    pub fn from_timecode(timecode: &str, rate: f64) -> Result<Self> {
        if !Self::is_smpte_timecode_rate(rate) {
            return Err(TimeError::InvalidTimecodeRate { rate });
        }

        let mut rate_is_dropframe = Self::is_drop_frame_rate(rate);
        if timecode.contains(';') {
            if !rate_is_dropframe {
                return Err(TimeError::InvalidRateForDropFrameTimecode { rate });
            }
        } else {
            rate_is_dropframe = false;
        }

        let invalid = || TimeError::InvalidTimecodeString {
            timecode: timecode.to_string(),
        };

        // Upstream slices fixed two-character fields at offsets 0, 3, 6 and 9
        // and runs each through `std::stoi`, so any separator character is
        // accepted and a short string is rejected.
        let bytes = timecode.as_bytes();
        let mut fields = [0_i32; 4];
        for (index, field) in fields.iter_mut().enumerate() {
            let start = index * 3;
            if start > bytes.len() {
                return Err(invalid());
            }
            let end = (start + 2).min(bytes.len());
            *field = parse_leading_int(&bytes[start..end]).ok_or_else(invalid)?;
        }
        let [hours, minutes, seconds, frames] = fields;

        let nominal_fps = rate.ceil() as i32;
        if frames >= nominal_fps {
            return Err(TimeError::TimecodeRateMismatch {
                timecode: timecode.to_string(),
                max_frame: nominal_fps - 1,
            });
        }

        let dropframes = if rate_is_dropframe {
            drop_frames_per_minute(rate)
        } else {
            0
        };

        let total_minutes = hours * 60 + minutes;
        let value = ((total_minutes * 60) + seconds) * nominal_fps + frames
            - (dropframes * (total_minutes - (total_minutes / 10)));

        Ok(Self::new(f64::from(value), rate))
    }

    /// Parse a time string of the form `"[-]HH:MM:SS.sss"`.
    ///
    /// Hours and minutes may be omitted from the left. Seconds may carry a
    /// fractional part.
    ///
    /// # Errors
    ///
    /// Returns an error if `rate` is not a SMPTE rate, if the string has more
    /// than three fields, if a field contains anything but digits and a
    /// decimal point, or if a minutes or seconds field is 60 or greater.
    ///
    /// # Compatibility
    ///
    /// Upstream documents a leading `-` as supported but its parser rejects
    /// one, so `to_time_string` output for a negative time does not round-trip
    /// there. This port accepts the sign, which makes every string upstream
    /// accepts parse identically here and additionally round-trips negative
    /// times.
    pub fn from_time_string(time_string: &str, rate: f64) -> Result<Self> {
        if !Self::is_smpte_timecode_rate(rate) {
            return Err(TimeError::InvalidTimecodeRate { rate });
        }

        let invalid = || TimeError::InvalidTimeString {
            time_string: time_string.to_string(),
        };

        /// Seconds per unit at each field position, counting from the right.
        const POWER: [f64; 3] = [1.0, 60.0, 3600.0];

        // The sign belongs to the whole time, not to the leftmost field:
        // "-00:00:01.0" is minus one second, and carrying the sign on the
        // hours field would lose it whenever that field is zero.
        let (sign, body) = match time_string.strip_prefix('-') {
            Some(rest) => (-1.0, rest),
            None => (1.0, time_string.strip_prefix('+').unwrap_or(time_string)),
        };

        let fields: Vec<&str> = body.split(':').collect();
        if fields.len() > POWER.len() {
            return Err(invalid());
        }

        let mut accumulator = 0.0;
        for (index, field) in fields.iter().enumerate() {
            // The rightmost field is seconds, the next minutes, then hours.
            let radix = fields.len() - 1 - index;

            // An empty field contributes nothing; upstream skips it rather
            // than failing, so trailing and interior colons are tolerated.
            if field.is_empty() {
                continue;
            }

            if !field.as_bytes()[0].is_ascii_digit() {
                return Err(invalid());
            }
            let value = parse_leading_float(field.as_bytes()).ok_or_else(invalid)?;

            // Only the leftmost field may reach 60: "90:00" is 90 minutes,
            // but "01:90" is not a valid minute-and-second pair.
            if index != 0 && radix < 2 && value >= 60.0 {
                return Err(invalid());
            }

            accumulator += value * POWER[radix];
        }

        Ok(Self::from_seconds(sign * accumulator).rescaled_to(rate))
    }

    /// Render this time as SMPTE timecode at its own rate.
    ///
    /// # Errors
    ///
    /// See [`RationalTime::to_timecode_at`].
    pub fn to_timecode(self) -> Result<String> {
        self.to_timecode_at(self.rate, DropFrame::InferFromRate)
    }

    /// Render this time as SMPTE timecode at `rate`.
    ///
    /// Rates within 0.1 of a SMPTE rate are snapped to it, so the commonly
    /// written 29.97 is accepted for 30000/1001.
    ///
    /// # Errors
    ///
    /// Returns an error if the time is negative, if `rate` is not within 0.1
    /// of a SMPTE rate, or if drop-frame is forced at a rate that has no
    /// drop-frame form.
    pub fn to_timecode_at(self, rate: f64, drop_frame: DropFrame) -> Result<String> {
        let frames_in_target_rate = self.value_rescaled_to(rate);
        if frames_in_target_rate < 0.0 {
            return Err(TimeError::NegativeValue);
        }

        // It is common practice to write truncated rates like 29.97 rather
        // than the exact 30000/1001, so snap to the nearest SMPTE rate when
        // one is close enough.
        let nearest_smpte_rate = Self::nearest_smpte_timecode_rate(rate);
        if (nearest_smpte_rate - rate).abs() > 0.1 {
            return Err(TimeError::InvalidTimecodeRate { rate });
        }
        let mut rate = nearest_smpte_rate;

        let mut rate_is_dropframe = Self::is_drop_frame_rate(rate);
        if drop_frame == DropFrame::ForceYes && !rate_is_dropframe {
            return Err(TimeError::InvalidRateForDropFrameTimecode { rate });
        }
        if drop_frame != DropFrame::InferFromRate {
            rate_is_dropframe = drop_frame == DropFrame::ForceYes;
        }

        let mut dropframes = 0;
        let mut divider = ':';
        if rate_is_dropframe {
            dropframes = drop_frames_per_minute(rate);
            divider = ';';
        } else if rate.round() == 24.0 {
            rate = 24.0;
        }

        let frames_per_hour = (rate * 60.0 * 60.0).round() as i32;
        let frames_per_24_hours = frames_per_hour * 24;
        let frames_per_10_minutes = (rate * 60.0 * 10.0).round() as i32;
        let frames_per_minute = ((rate.round() * 60.0) - f64::from(dropframes)) as i32;

        // Timecode rolls over after 24 hours.
        let mut value = frames_in_target_rate % f64::from(frames_per_24_hours);

        if rate_is_dropframe {
            let ten_minute_chunks = (value / f64::from(frames_per_10_minutes)).floor() as i32;
            let frames_over_ten_minutes = (value % f64::from(frames_per_10_minutes)) as i32;

            if frames_over_ten_minutes > dropframes {
                value += f64::from(
                    (dropframes * 9 * ten_minute_chunks)
                        + dropframes * ((frames_over_ten_minutes - dropframes) / frames_per_minute),
                );
            } else {
                value += f64::from(dropframes * 9 * ten_minute_chunks);
            }
        }

        let nominal_fps = rate.ceil() as i32;
        let frames = (value % f64::from(nominal_fps)) as i32;
        let seconds_total = (value / f64::from(nominal_fps)).floor() as i32;
        let seconds = seconds_total % 60;
        let minutes = (seconds_total / 60) % 60;
        let hours = (seconds_total / 60) / 60;

        Ok(format!(
            "{hours:02}:{minutes:02}:{seconds:02}{divider}{frames:02}"
        ))
    }

    /// Render this time as SMPTE timecode, snapping `rate` to the nearest
    /// SMPTE rate first.
    ///
    /// Unlike [`RationalTime::to_timecode_at`], which only tolerates rates
    /// within 0.1 of a SMPTE rate, this accepts any rate.
    ///
    /// # Errors
    ///
    /// Returns an error if the time is negative, or if drop-frame is forced at
    /// a rate that has no drop-frame form.
    pub fn to_nearest_timecode_at(self, rate: f64, drop_frame: DropFrame) -> Result<String> {
        self.to_timecode_at(Self::nearest_smpte_timecode_rate(rate), drop_frame)
    }

    /// Render this time as SMPTE timecode at the nearest SMPTE rate to its own.
    ///
    /// # Errors
    ///
    /// See [`RationalTime::to_nearest_timecode_at`].
    pub fn to_nearest_timecode(self) -> Result<String> {
        self.to_nearest_timecode_at(self.rate, DropFrame::InferFromRate)
    }

    /// Render this time as `"[-]HH:MM:SS.sss"`.
    ///
    /// Negative times are rendered with a leading `-`, for compatibility with
    /// ffmpeg. The fractional seconds carry up to microsecond resolution.
    #[must_use]
    pub fn to_time_string(self) -> String {
        const SECONDS_PER_MINUTE: f64 = 60.0;
        const SECONDS_PER_HOUR: f64 = SECONDS_PER_MINUTE * 60.0;
        const SECONDS_PER_DAY: f64 = SECONDS_PER_HOUR * 24.0;
        /// Width of the fractional part, including its leading '.'.
        const FRACTION_WIDTH: usize = 7;

        let mut total_seconds = self.to_seconds();
        // Compute with a positive number and reattach the sign at the end.
        let is_negative = total_seconds.is_sign_negative();
        if is_negative {
            total_seconds = total_seconds.abs();
        }

        let hour_units = total_seconds % SECONDS_PER_DAY;
        let hours = (hour_units / SECONDS_PER_HOUR).floor() as i32;
        let minute_units = hour_units % SECONDS_PER_HOUR;
        let minutes = (minute_units / SECONDS_PER_MINUTE).floor() as i32;
        let seconds = minute_units % SECONDS_PER_MINUTE;

        let whole_seconds = seconds.trunc();
        let fraction = seconds - whole_seconds;

        let seconds_str = format!("{:02}", whole_seconds as i32);

        // `%.7g` of a value in [0, 1) renders as "0.xxxxxxx"; dropping the
        // leading zero leaves the ".xxxxxxx" that gets appended to the
        // seconds. A fraction of exactly zero renders as "0", which leaves
        // nothing, and the minimum rendering is ".0".
        let fraction_str = format_g(fraction, 7);
        let fraction_str = &fraction_str[1..];
        let fraction_str = if fraction_str.is_empty() {
            ".0"
        } else {
            &fraction_str[..fraction_str.len().min(FRACTION_WIDTH)]
        };

        let sign = if is_negative { "-" } else { "" };
        format!("{sign}{hours:02}:{minutes:02}:{seconds_str}{fraction_str}")
    }
}

/// The number of frames dropped per minute at a drop-frame rate.
fn drop_frames_per_minute(rate: f64) -> i32 {
    if rate == 30000.0 / 1001.0 || rate == 29.97 {
        2
    } else if rate == 60000.0 / 1001.0 || rate == 59.94 {
        4
    } else {
        0
    }
}

/// Parse a leading integer, the way C's `std::stoi` does.
///
/// Leading whitespace and an optional sign are allowed, at least one digit is
/// required, and trailing characters are ignored.
fn parse_leading_int(bytes: &[u8]) -> Option<i32> {
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    let negative = match bytes.get(index) {
        Some(b'-') => {
            index += 1;
            true
        }
        Some(b'+') => {
            index += 1;
            false
        }
        _ => false,
    };

    let digits_start = index;
    let mut magnitude: i32 = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        magnitude = magnitude
            .checked_mul(10)?
            .checked_add(i32::from(bytes[index] - b'0'))?;
        index += 1;
    }
    if index == digits_start {
        return None;
    }

    Some(if negative { -magnitude } else { magnitude })
}

/// Parse an unsigned decimal number, the way upstream's `parseFloat` does.
///
/// Digits, then optionally a decimal point and more digits. Parsing stops at
/// the first character that does not fit, and an integer part too large to
/// survive the round trip through `f64` is rejected. The sign is handled by
/// the caller, since it belongs to the time as a whole.
fn parse_leading_float(bytes: &[u8]) -> Option<f64> {
    let mut index = 0;

    let mut integer_part: u64 = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        integer_part = integer_part
            .checked_mul(10)?
            .checked_add(u64::from(bytes[index] - b'0'))?;
        index += 1;
    }

    let mut result = integer_part as f64;
    if result as u64 != integer_part {
        // More digits than an f64 can hold exactly.
        return None;
    }

    if index < bytes.len() {
        if bytes[index] != b'.' {
            return None;
        }
        index += 1;

        let mut position_scale = 0.1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            result += f64::from(bytes[index] - b'0') * position_scale;
            index += 1;
            position_scale *= 0.1;
        }
    }

    Some(result)
}

impl fmt::Display for RationalTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RationalTime({}, {})", self.value, self.rate)
    }
}

impl PartialEq for RationalTime {
    /// Compares after rescaling `other` to this time's rate.
    ///
    /// Use [`RationalTime::strictly_equal`] to compare without rescaling.
    fn eq(&self, other: &Self) -> bool {
        self.value_rescaled_to(other.rate) == other.value
    }
}

impl PartialOrd for RationalTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (self.value / self.rate).partial_cmp(&(other.value / other.rate))
    }

    // Upstream defines `<` as `!(>=)` and `<=` as `!(>)`, which differs from
    // the derived behaviour once a NaN is involved. Mirror it.
    fn gt(&self, other: &Self) -> bool {
        (self.value / self.rate) > (other.value / other.rate)
    }

    fn ge(&self, other: &Self) -> bool {
        (self.value / self.rate) >= (other.value / other.rate)
    }

    fn lt(&self, other: &Self) -> bool {
        !self.ge(other)
    }

    fn le(&self, other: &Self) -> bool {
        !self.gt(other)
    }
}

impl Add for RationalTime {
    type Output = Self;

    /// Adds two times, resolving to the higher of the two rates.
    fn add(self, rhs: Self) -> Self {
        if self.rate < rhs.rate {
            Self::new(self.value_rescaled_to(rhs.rate) + rhs.value, rhs.rate)
        } else {
            Self::new(rhs.value_rescaled_to(self.rate) + self.value, self.rate)
        }
    }
}

impl Sub for RationalTime {
    type Output = Self;

    /// Subtracts two times, resolving to the higher of the two rates.
    fn sub(self, rhs: Self) -> Self {
        if self.rate < rhs.rate {
            Self::new(self.value_rescaled_to(rhs.rate) - rhs.value, rhs.rate)
        } else {
            Self::new(self.value - rhs.value_rescaled_to(self.rate), self.rate)
        }
    }
}

impl Neg for RationalTime {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.value, self.rate)
    }
}

impl AddAssign for RationalTime {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for RationalTime {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

/// Returns the lesser of two times, matching C++ `std::min`.
#[must_use]
pub fn min(a: RationalTime, b: RationalTime) -> RationalTime {
    if b < a { b } else { a }
}

/// Returns the greater of two times, matching C++ `std::max`.
#[must_use]
pub fn max(a: RationalTime, b: RationalTime) -> RationalTime {
    if a < b { b } else { a }
}

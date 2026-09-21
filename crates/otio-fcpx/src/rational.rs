//! FCP X's rational time values, and the fraction arithmetic they need.
//!
//! FCP X writes every time as a rational number of seconds — `0s`, `3600s`,
//! `1001/24000s` — rather than as a frame count. Converting back and forth is
//! most of what the adapter does with time, and the rounding upstream applies
//! on the way is load-bearing: it truncates rather than rounds, so a value
//! that lands a hair under a frame boundary falls to the frame below.

use opentime::RationalTime;

/// Reads an FCP X rational time as a number of frames at `fps`.
///
/// Upstream truncates the result rather than rounding it, so `0.9999` frames
/// is zero frames. Reproduced here: rounding instead would move clips by a
/// frame against every file Final Cut has ever written.
#[must_use]
pub fn frames_at(value: Option<&str>, fps: f64) -> f64 {
    let Some(value) = value else {
        return 0.0;
    };
    if value == "0s" {
        return 0.0;
    }

    match value.split_once('/') {
        Some((numerator, denominator)) => {
            let numerator: f64 = numerator.parse().unwrap_or(0.0);
            let denominator: f64 = denominator.trim_end_matches('s').parse().unwrap_or(1.0);
            if denominator == 0.0 {
                return 0.0;
            }
            (numerator / denominator * fps).trunc()
        }
        None => (value.trim_end_matches('s').parse::<f64>().unwrap_or(0.0) * fps).trunc(),
    }
}

/// Reads an FCP X rational time as a [`RationalTime`] at a whole `fps`.
///
/// The rate is whole because that is what upstream's format lookup returns:
/// it divides the frame duration and truncates, so a 23.976 format reports 23.
#[must_use]
pub fn to_rational_time(value: Option<&str>, fps: i64) -> RationalTime {
    #[expect(
        clippy::cast_precision_loss,
        reason = "frame rates are small whole numbers"
    )]
    let rate = fps as f64;
    RationalTime::new(frames_at(value, rate), rate)
}

/// Writes a time as an FCP X rational number of seconds.
///
/// A whole number of seconds is written without a denominator, which is what
/// makes `3600s` — the offset Final Cut gives every gap — come out the way the
/// application writes it.
#[must_use]
pub fn from_rational_time(time: RationalTime) -> String {
    if time.value().trunc() == 0.0 {
        return "0s".to_string();
    }
    let (numerator, denominator) = approximate(time.value() / time.rate());
    if denominator == 1 {
        return format!("{numerator}s");
    }
    format!("{numerator}/{denominator}s")
}

/// Writes a value and rate as an FCP X rational number of seconds.
///
/// Unlike [`from_rational_time`] this always writes a denominator, so a whole
/// number of seconds comes out as `10/1s`. Upstream keeps the two spellings
/// apart in the same way, and Final Cut accepts both.
#[must_use]
pub fn rational_number(value: f64, rate: f64) -> String {
    if value.trunc() == 0.0 {
        return "0s".to_string();
    }
    let (numerator, denominator) = approximate(value / rate);
    format!("{numerator}/{denominator}s")
}

/// The largest denominator [`approximate`] will produce.
///
/// This is Python's `Fraction.limit_denominator` default, which is what
/// upstream calls.
const MAX_DENOMINATOR: i128 = 1_000_000;

/// Returns the closest fraction to `value` whose denominator is at most
/// [`MAX_DENOMINATOR`].
///
/// This is Python's `Fraction(float).limit_denominator()`: the exact binary
/// value of the float, then the best rational approximation within the
/// denominator bound, found by walking the continued fraction. Doing it any
/// other way — rounding to a fixed denominator, say — writes times that Final
/// Cut reads back a frame off.
fn approximate(value: f64) -> (i128, i128) {
    let (mut numerator, mut denominator) = exact_ratio(value);
    if denominator <= MAX_DENOMINATOR {
        return (numerator, denominator);
    }

    let original_denominator = denominator;
    let (mut previous_numerator, mut previous_denominator) = (0i128, 1i128);
    let (mut best_numerator, mut best_denominator) = (1i128, 0i128);

    loop {
        let whole = numerator.div_euclid(denominator);
        let next_denominator = previous_denominator + whole * best_denominator;
        if next_denominator > MAX_DENOMINATOR {
            break;
        }
        let next_numerator = previous_numerator + whole * best_numerator;
        previous_numerator = best_numerator;
        previous_denominator = best_denominator;
        best_numerator = next_numerator;
        best_denominator = next_denominator;

        let remainder = numerator - whole * denominator;
        numerator = denominator;
        denominator = remainder;
        if denominator == 0 {
            break;
        }
    }

    if best_denominator == 0 {
        return (best_numerator, 1);
    }

    // Two candidates bound the value; take whichever is closer, the way
    // Python's own comparison does.
    let steps = (MAX_DENOMINATOR - previous_denominator) / best_denominator;
    let bounded_denominator = previous_denominator + steps * best_denominator;
    if 2 * denominator * bounded_denominator <= original_denominator {
        (best_numerator, best_denominator)
    } else {
        (
            previous_numerator + steps * best_numerator,
            bounded_denominator,
        )
    }
}

/// Returns a float's exact value as a fraction, in lowest terms.
///
/// This is Python's `float.as_integer_ratio()` followed by `Fraction`'s
/// normalization. A float is a binary fraction, so this is exact rather than
/// an approximation.
fn exact_ratio(value: f64) -> (i128, i128) {
    if !value.is_finite() || value == 0.0 {
        return (0, 1);
    }

    let sign = if value < 0.0 { -1i128 } else { 1i128 };
    let mut magnitude = value.abs();
    let mut exponent = 0i32;

    // Scale to an integer. A finite f64 needs at most 1074 halvings to reach
    // its integer significand, but the times in an edit are far from the
    // subnormal range, so this converges in a few dozen steps.
    while magnitude.fract() != 0.0 && exponent < 120 {
        magnitude *= 2.0;
        exponent += 1;
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the loop above runs until the value is integral"
    )]
    let numerator = sign * magnitude as i128;
    let denominator = 1i128 << exponent;

    let divisor = gcd(numerator.abs(), denominator);
    (numerator / divisor, denominator / divisor)
}

fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    if a == 0 { 1 } else { a }
}

#[cfg(test)]
mod tests {
    use opentime::RationalTime;

    use super::{frames_at, from_rational_time, rational_number, to_rational_time};

    #[test]
    fn reads_the_spellings_final_cut_writes() {
        assert_eq!(frames_at(Some("0s"), 30.0), 0.0);
        assert_eq!(frames_at(None, 30.0), 0.0);
        assert_eq!(frames_at(Some("3600s"), 30.0), 108_000.0);
        assert_eq!(frames_at(Some("10s"), 30.0), 300.0);
        assert_eq!(frames_at(Some("100/3000s"), 30.0), 1.0);
        assert_eq!(frames_at(Some("28500/3000s"), 30.0), 285.0);
    }

    /// Upstream truncates rather than rounding, and a file full of values a
    /// hair under a frame boundary depends on it.
    #[test]
    fn a_time_just_short_of_a_frame_falls_to_the_frame_below() {
        // 6480/600 seconds at 30fps is 324 frames exactly; one 600th less is
        // not, and must not round up.
        assert_eq!(frames_at(Some("6479/600s"), 30.0), 323.0);
        assert_eq!(
            to_rational_time(Some("6479/600s"), 30),
            RationalTime::new(323.0, 30.0)
        );
    }

    #[test]
    fn writes_whole_seconds_without_a_denominator() {
        assert_eq!(from_rational_time(RationalTime::new(0.0, 30.0)), "0s");
        assert_eq!(
            from_rational_time(RationalTime::new(108_000.0, 30.0)),
            "3600s"
        );
        assert_eq!(from_rational_time(RationalTime::new(300.0, 30.0)), "10s");
        assert_eq!(from_rational_time(RationalTime::new(1.0, 30.0)), "1/30s");
        assert_eq!(from_rational_time(RationalTime::new(285.0, 30.0)), "19/2s");
    }

    /// The other spelling always carries a denominator, whole seconds
    /// included, which is what upstream's two helpers differ on.
    #[test]
    fn a_duration_always_carries_a_denominator() {
        assert_eq!(rational_number(0.0, 30.0), "0s");
        assert_eq!(rational_number(300.0, 30.0), "10/1s");
        assert_eq!(rational_number(1.0, 30.0), "1/30s");
        assert_eq!(rational_number(6491.0, 600.0), "6491/600s");
    }

    /// The approximation has to reproduce the awkward values Final Cut
    /// writes, not just the tidy ones.
    #[test]
    fn awkward_values_come_back_as_themselves() {
        for (value, rate, expected) in [
            (21_602_243.0, 144_000.0, "21602243/144000s"),
            (32_554_800.0, 720_000.0, "9043/200s"),
            (26_566_800.0, 720_000.0, "22139/600s"),
            (40_880.0, 600.0, "1022/15s"),
        ] {
            assert_eq!(rational_number(value, rate), expected, "{value}/{rate}");
        }
    }
}

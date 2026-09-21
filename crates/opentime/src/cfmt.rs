//! Minimal re-implementation of the C `printf` conversions `opentime` relies on.
//!
//! `RationalTime::to_time_string` formats the fractional part of its seconds
//! field with `%.7g`, and the exact digits it produces are part of the
//! serialized form of a timeline. Rust has no `%g`, so it is reproduced here.
//!
//! It is public because the Python bindings need it too: upstream's
//! `RationalTime.__str__` is written with `%g`, and a binding that prints
//! `100000000000000000000` where upstream prints `1e+20` is not a drop-in
//! replacement.

/// Format `value` the way C's `%.{precision}g` would.
///
/// `%g` picks between `%e` and `%f` based on the decimal exponent the value
/// would have in `%e` form after rounding to `precision` significant digits:
/// scientific if that exponent is below -4 or at least `precision`, fixed
/// otherwise. Either way trailing zeros (and a trailing decimal point) are
/// removed.
pub fn format_g(value: f64, precision: usize) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_string();
    }

    let precision = precision.max(1);

    // Round to `precision` significant digits first, then read the exponent
    // back off, so that a value like 9.9999999 reports exponent 1 rather
    // than 0.
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent_str) = scientific
        .split_once('e')
        .expect("Rust's LowerExp always emits an 'e'");
    let exponent: i32 = exponent_str
        .parse()
        .expect("Rust's LowerExp always emits a parseable exponent");

    if exponent < -4 || exponent >= precision as i32 {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{}{:02}",
            trim_trailing_zeros(mantissa),
            sign,
            exponent.abs()
        )
    } else {
        let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
        let fixed = format!("{value:.decimals$}");
        trim_trailing_zeros(&fixed).to_string()
    }
}

/// Strip trailing zeros, and a trailing decimal point, from a decimal string.
fn trim_trailing_zeros(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::format_g;

    #[test]
    fn matches_c_printf_for_fixed_notation() {
        assert_eq!(format_g(0.0, 7), "0");
        assert_eq!(format_g(0.5, 7), "0.5");
        assert_eq!(format_g(0.041667, 7), "0.041667");
        assert_eq!(format_g(0.3333333333, 7), "0.3333333");
        assert_eq!(format_g(1.0, 7), "1");
        assert_eq!(format_g(123.456, 7), "123.456");
    }

    #[test]
    fn switches_to_scientific_outside_the_fixed_window() {
        // Exponent below -4 goes scientific, as does one at or above the
        // precision.
        assert_eq!(format_g(0.00005, 7), "5e-05");
        assert_eq!(format_g(12345678.0, 7), "1.234568e+07");
    }

    #[test]
    fn handles_non_finite_values() {
        assert_eq!(format_g(f64::NAN, 7), "nan");
        assert_eq!(format_g(f64::INFINITY, 7), "inf");
        assert_eq!(format_g(f64::NEG_INFINITY, 7), "-inf");
    }
}

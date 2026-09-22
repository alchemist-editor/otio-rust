//! The values a writer stores, before they are encoded against a type.

use std::fmt;

use super::ObjRef;
use crate::{Auid, MobId};

/// A value to store in a property.
///
/// This is what pyaaf2 accepts as a Python value, spelled out: the writer
/// encodes it against the type the property declares, the way pyaaf2's type
/// definitions do, so the same value can be stored as more than one type. An
/// [`Int`](Self::Int) becomes an integer of whatever width the property is,
/// the numerator of a rational, or the value of an enumeration element; a
/// [`Str`](Self::Str) becomes a string, the name of an enumeration element,
/// or a rational parsed from `"24000/1001"`.
///
/// Most values are made with `From`: `5.into()`, `"picture".into()`,
/// `some_auid.into()`, `child_object.into()`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WriteValue {
    /// A boolean. Stored as `Boolean`, or as `1` or `0` in an integer.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A floating-point number: a Python `float`.
    ///
    /// Stored as a rational the way `AAFRational(float)` makes one (see
    /// [`Rational::from_f64`]), as `Boolean` by whether it is zero, and as
    /// the element of an enumeration whose value it equals. pyaaf2 cannot
    /// store a `float` as anything else, an integer included, and neither
    /// can this.
    Float(f64),
    /// A rational number.
    Rational(Rational),
    /// A string, or the name of an enumeration element.
    Str(String),
    /// A 16-byte identifier.
    Auid(Auid),
    /// A 32-byte mob identifier.
    MobId(MobId),
    /// A date and time.
    Timestamp(Timestamp),
    /// A record, by member name. Members may be given in any order.
    Record(Vec<(String, WriteValue)>),
    /// The elements of an array or a set, in order.
    ///
    /// A set is written in the order given, without repeats. pyaaf2 writes a
    /// set in whatever order Python iterates it, which for a set of a few
    /// small non-negative integers is ascending order.
    Array(Vec<WriteValue>),
    /// An object, for a strong or weak reference.
    Object(ObjRef),
    /// Objects, for a collection of strong or weak references.
    Objects(Vec<ObjRef>),
    /// A value for an indirect property, stored as the given type rather than
    /// the one pyaaf2 would infer from the value.
    ///
    /// Without this, an indirect value is stored as `aafString`, `Rational`
    /// or `aafInt32`, as pyaaf2 infers from a Python `str`, `Fraction` or
    /// `int`.
    Typed {
        /// The type to store the value as.
        type_id: Auid,
        /// The value.
        value: Box<WriteValue>,
    },
}

impl WriteValue {
    /// A value for an indirect property, stored as `type_id`.
    #[must_use]
    pub fn typed(type_id: Auid, value: impl Into<Self>) -> Self {
        Self::Typed {
            type_id,
            value: Box::new(value.into()),
        }
    }

    /// A record from `(member, value)` pairs.
    #[must_use]
    pub fn record<K: Into<String>, V: Into<Self>>(
        members: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        Self::Record(
            members
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
    }

    /// An array from anything that converts to values.
    #[must_use]
    pub fn array<V: Into<Self>>(items: impl IntoIterator<Item = V>) -> Self {
        Self::Array(items.into_iter().map(Into::into).collect())
    }

    /// What kind of value this is, for error messages.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Bool(_) => "a boolean",
            Self::Int(_) => "an integer",
            Self::Float(_) => "a float",
            Self::Rational(_) => "a rational",
            Self::Str(_) => "a string",
            Self::Auid(_) => "an AUID",
            Self::MobId(_) => "a MobID",
            Self::Timestamp(_) => "a timestamp",
            Self::Record(_) => "a record",
            Self::Array(_) => "an array",
            Self::Object(_) => "an object",
            Self::Objects(_) => "a list of objects",
            Self::Typed { .. } => "a typed value",
        }
    }
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for WriteValue {
            fn from(value: $t) -> Self {
                Self::Int(i64::from(value))
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32);

impl From<f64> for WriteValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for WriteValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<&str> for WriteValue {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for WriteValue {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<Rational> for WriteValue {
    fn from(value: Rational) -> Self {
        Self::Rational(value)
    }
}

impl From<Auid> for WriteValue {
    fn from(value: Auid) -> Self {
        Self::Auid(value)
    }
}

impl From<MobId> for WriteValue {
    fn from(value: MobId) -> Self {
        Self::MobId(value)
    }
}

impl From<Timestamp> for WriteValue {
    fn from(value: Timestamp) -> Self {
        Self::Timestamp(value)
    }
}

impl From<ObjRef> for WriteValue {
    fn from(value: ObjRef) -> Self {
        Self::Object(value)
    }
}

impl From<Vec<ObjRef>> for WriteValue {
    fn from(value: Vec<ObjRef>) -> Self {
        Self::Objects(value)
    }
}

impl From<&[ObjRef]> for WriteValue {
    fn from(value: &[ObjRef]) -> Self {
        Self::Objects(value.to_vec())
    }
}

/// A rational number, as AAF stores one: a numerator over a denominator,
/// never reduced.
///
/// pyaaf2's `AAFRational` deliberately keeps whatever it was given —
/// `48/2` stays `48/2` — and so does this. The constructors mirror the ways
/// `AAFRational` can be built, so that a value built the same way in both is
/// stored the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    /// The numerator.
    pub numerator: i64,
    /// The denominator.
    pub denominator: i64,
}

impl Rational {
    /// A rational from its two parts, kept as given.
    #[must_use]
    pub const fn new(numerator: i64, denominator: i64) -> Self {
        Self {
            numerator,
            denominator,
        }
    }

    /// Parses a rational the way `AAFRational("...")` does.
    ///
    /// That is Python's `Fraction` syntax: `"24000/1001"`, `"25"`,
    /// `"23.976"` (which is `23976/1000`, not reduced), `"1e3"`, with an
    /// optional sign and surrounding whitespace. `"0/0"` is taken as `0/1`,
    /// which pyaaf2 does for files from applications that write it.
    ///
    /// # Errors
    ///
    /// Returns an error if `text` is not in that syntax, or has a zero
    /// denominator over a non-zero numerator.
    pub fn parse(text: &str) -> Result<Self, ParseRationalError> {
        parse_fraction(text).ok_or_else(|| ParseRationalError {
            text: text.to_owned(),
        })
    }

    /// A rational from a floating-point number, the way `AAFRational(float)`
    /// makes one.
    ///
    /// The float is converted exactly and then brought to the closest
    /// fraction whose denominator fits a signed 32-bit integer, which is
    /// Python's `Fraction.from_float(x).limit_denominator(0x7FFFFFFF)`. A
    /// numerator still too large for 32 bits is clamped as pyaaf2 clamps it,
    /// sign and all: pyaaf2 sets it to `0x7FFFFFFF` and scales the
    /// denominator to match, so a large negative number comes out positive.
    /// This keeps that behaviour rather than correcting it, because files
    /// written with the correction would not match pyaaf2's.
    ///
    /// Returns `None` for infinities and NaN, which Python refuses too.
    #[must_use]
    pub fn from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let (mut n, mut d) = float_ratio(value)?;
        const MAX: i128 = 0x7fff_ffff;
        if d > MAX {
            (n, d) = limit_denominator(n, d, MAX);
        }
        if !(-MAX..=MAX).contains(&n) {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            {
                d = (d as f64 * (MAX as f64 / n as f64)) as i128;
            }
            n = MAX;
        }
        Some(Self {
            numerator: i64::try_from(n).ok()?,
            denominator: i64::try_from(d).ok()?,
        })
    }
}

impl From<i64> for Rational {
    fn from(value: i64) -> Self {
        Self::new(value, 1)
    }
}

impl From<i32> for Rational {
    fn from(value: i32) -> Self {
        Self::new(i64::from(value), 1)
    }
}

impl From<u32> for Rational {
    fn from(value: u32) -> Self {
        Self::new(i64::from(value), 1)
    }
}

impl From<(i64, i64)> for Rational {
    fn from((numerator, denominator): (i64, i64)) -> Self {
        Self::new(numerator, denominator)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}

impl std::str::FromStr for Rational {
    type Err = ParseRationalError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// The error returned when a string is not a rational in Python's syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRationalError {
    text: String,
}

impl fmt::Display for ParseRationalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' is not a rational number", self.text)
    }
}

impl std::error::Error for ParseRationalError {}

/// Digits with optional single underscores between them, as Python allows.
fn digits(text: &str) -> Option<i128> {
    if text.is_empty() || text.starts_with('_') || text.ends_with('_') || text.contains("__") {
        return None;
    }
    let mut value: i128 = 0;
    for c in text.chars().filter(|c| *c != '_') {
        let d = c.to_digit(10)?;
        value = value.checked_mul(10)?.checked_add(i128::from(d))?;
    }
    Some(value)
}

/// Python's `_RATIONAL_FORMAT`, applied the way `AAFRational.__new__` does.
fn parse_fraction(text: &str) -> Option<Rational> {
    let s = text.trim();
    let (negative, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    // The lookahead: a digit, or a point and a digit.
    let first = s.chars().next()?;
    if !(first.is_ascii_digit()
        || (first == '.' && s[1..].starts_with(|c: char| c.is_ascii_digit())))
    {
        return None;
    }

    let (mut numerator, mut denominator);
    if let Some((num, den)) = s.split_once('/') {
        numerator = digits(num.trim_end())?;
        denominator = digits(den.trim_start())?;
        if num.trim_end().len() != num.len() && num.trim_end().is_empty() {
            return None;
        }
    } else {
        let (mantissa, exp) = match s.find(['e', 'E']) {
            Some(at) => (&s[..at], Some(&s[at + 1..])),
            None => (s, None),
        };
        let (int, decimal) = match mantissa.split_once('.') {
            Some((int, decimal)) => (int, Some(decimal)),
            None => (mantissa, None),
        };
        numerator = if int.is_empty() { 0 } else { digits(int)? };
        denominator = 1;
        if let Some(decimal) = decimal {
            if !decimal.is_empty() {
                let places = decimal.chars().filter(|c| *c != '_').count();
                let scale = 10i128.checked_pow(u32::try_from(places).ok()?)?;
                numerator = numerator
                    .checked_mul(scale)?
                    .checked_add(digits(decimal)?)?;
                denominator = denominator.checked_mul(scale)?;
            }
        }
        if let Some(exp) = exp {
            let (neg, e) = match exp.as_bytes().first() {
                Some(b'-') => (true, &exp[1..]),
                Some(b'+') => (false, &exp[1..]),
                _ => (false, exp),
            };
            let e = u32::try_from(digits(e)?).ok()?;
            let power = 10i128.checked_pow(e)?;
            if neg {
                denominator = denominator.checked_mul(power)?;
            } else {
                numerator = numerator.checked_mul(power)?;
            }
        }
    }
    if negative {
        numerator = -numerator;
    }
    if denominator == 0 {
        if numerator == 0 {
            return Some(Rational::new(0, 1));
        }
        return None;
    }
    Some(Rational::new(
        i64::try_from(numerator).ok()?,
        i64::try_from(denominator).ok()?,
    ))
}

/// A finite float as an exact fraction in lowest terms, as
/// `Fraction.from_float` gives it, if it fits in 128 bits.
fn float_ratio(value: f64) -> Option<(i128, i128)> {
    if value == 0.0 {
        return Some((0, 1));
    }
    let bits = value.to_bits();
    let negative = bits >> 63 == 1;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mut mantissa, mut exp) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1u64 << 52), exponent - 1075)
    };
    while mantissa & 1 == 0 {
        mantissa >>= 1;
        exp += 1;
    }
    let (n, d) = if exp >= 0 {
        (
            i128::from(mantissa).checked_shl(u32::try_from(exp).ok()?)?,
            1,
        )
    } else {
        let shift = u32::try_from(-exp).ok()?;
        if shift >= 126 {
            return None;
        }
        (i128::from(mantissa), 1i128 << shift)
    };
    if exp >= 0 && n >> u32::try_from(exp).ok()? != i128::from(mantissa) {
        return None;
    }
    Some((if negative { -n } else { n }, d))
}

/// Python's `Fraction.limit_denominator`: the closest fraction to `n/d`
/// whose denominator is at most `max`.
fn limit_denominator(n: i128, d: i128, max: i128) -> (i128, i128) {
    let (mut p0, mut q0, mut p1, mut q1) = (0i128, 1i128, 1i128, 0i128);
    let (mut num, mut den) = (n, d);
    loop {
        let a = num.div_euclid(den);
        let q2 = q0 + a * q1;
        if q2 > max {
            break;
        }
        (p0, q0, p1, q1) = (p1, q1, p0 + a * p1, q2);
        (num, den) = (den, num - a * den);
    }
    let k = (max - q0) / q1;
    // Of the two candidates, take the second bound if it is at least as close.
    let bound1 = (p0 + k * p1, q0 + k * q1);
    let bound2 = (p1, q1);
    // |b2 - x| <= |b1 - x|, compared exactly over a common denominator.
    let diff2 = (bound2.0 * d - n * bound2.1).abs() * bound1.1;
    let diff1 = (bound1.0 * d - n * bound1.1).abs() * bound2.1;
    if diff2 <= diff1 { bound2 } else { bound1 }
}

/// A date and time of day, to the second.
///
/// AAF's `TimeStamp` has a hundredths-of-a-second field too, which pyaaf2
/// always writes as zero, so this has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp {
    /// The year.
    pub year: i16,
    /// The month, 1 to 12.
    pub month: u8,
    /// The day of the month, 1 to 31.
    pub day: u8,
    /// The hour, 0 to 23.
    pub hour: u8,
    /// The minute, 0 to 59.
    pub minute: u8,
    /// The second, 0 to 59.
    pub second: u8,
}

impl Timestamp {
    /// The UTC time `seconds` after the Unix epoch.
    #[must_use]
    pub fn from_unix(seconds: i64) -> Self {
        let days = seconds.div_euclid(86_400);
        let secs = seconds.rem_euclid(86_400);
        // Howard Hinnant's days-from-civil, inverted.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Self {
            year: year as i16,
            month: month as u8,
            day: day as u8,
            hour: (secs / 3600) as u8,
            minute: (secs / 60 % 60) as u8,
            second: (secs % 60) as u8,
        }
    }

    /// The seconds after the Unix epoch, taking this as UTC.
    #[must_use]
    pub fn to_unix(self) -> i64 {
        // Howard Hinnant's days-from-civil.
        let month = i64::from(self.month);
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let yoe = year.rem_euclid(400);
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
    }

    /// Parses `YYYY-MM-DDTHH:MM:SS`.
    #[must_use]
    pub fn parse_iso(text: &str) -> Option<Self> {
        let (date, time) = text.split_once('T')?;
        let mut d = date.splitn(3, '-');
        let mut t = time.splitn(3, ':');
        Some(Self {
            year: d.next()?.parse().ok()?,
            month: d.next()?.parse().ok()?,
            day: d.next()?.parse().ok()?,
            hour: t.next()?.parse().ok()?,
            minute: t.next()?.parse().ok()?,
            second: t.next()?.parse().ok()?,
        })
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rationals_as_python_does() {
        assert_eq!(Rational::parse("16/9"), Ok(Rational::new(16, 9)));
        assert_eq!(Rational::parse(" 24 "), Ok(Rational::new(24, 1)));
        assert_eq!(Rational::parse("23.976"), Ok(Rational::new(23976, 1000)));
        assert_eq!(Rational::parse("-1.5"), Ok(Rational::new(-15, 10)));
        assert_eq!(Rational::parse("1e3"), Ok(Rational::new(1000, 1)));
        assert_eq!(Rational::parse("5E-1"), Ok(Rational::new(5, 10)));
        assert_eq!(Rational::parse(".5"), Ok(Rational::new(5, 10)));
        assert_eq!(Rational::parse("0/0"), Ok(Rational::new(0, 1)));
        assert!(Rational::parse("1/0").is_err());
        assert!(Rational::parse("x").is_err());
        assert!(Rational::parse("").is_err());
    }

    #[test]
    fn converts_floats_as_python_does() {
        // Fraction.from_float(x).limit_denominator(0x7FFFFFFF), from CPython.
        assert_eq!(Rational::from_f64(0.5), Some(Rational::new(1, 2)));
        assert_eq!(Rational::from_f64(24.0), Some(Rational::new(24, 1)));
        assert_eq!(Rational::from_f64(23.976), Some(Rational::new(2_997, 125)));
        assert_eq!(Rational::from_f64(0.1), Some(Rational::new(1, 10)));
        assert_eq!(
            Rational::from_f64(29.97002997002997),
            Some(Rational::new(30_000, 1_001))
        );
        assert_eq!(Rational::from_f64(f64::NAN), None);
    }

    #[test]
    fn timestamps_from_unix_time() {
        let t = Timestamp::from_unix(1_714_979_289);
        assert_eq!(t.to_string(), "2024-05-06T07:08:09");
        assert_eq!(Timestamp::parse_iso("2024-05-06T07:08:09"), Some(t));
        assert_eq!(Timestamp::from_unix(0).to_string(), "1970-01-01T00:00:00");
        assert_eq!(t.to_unix(), 1_714_979_289);
        let leap = Timestamp::parse_iso("2000-02-29T23:59:59").unwrap();
        assert_eq!(Timestamp::from_unix(leap.to_unix()), leap);
    }
}

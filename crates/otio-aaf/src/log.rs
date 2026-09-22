//! Upstream's `transcribe_log`: a line for each thing the reader makes.
//!
//! Upstream prints these to standard output while it reads, indented two
//! spaces a level, and they are mostly Python formatting AAF names and OTIO
//! objects. The lines are written here as Python writes them, so that the
//! log reads the same from either library:
//!
//! - a name as `_encoded_name` gives it, the `repr` of its UTF-8 bytes,
//!   which is why upstream's log is full of `b'...'`;
//! - a marker, when one has no track to go to, as OTIO's `str(marker)`
//!   gives it, with the metadata as Python prints a `dict`.
//!
//! Three kinds of line cannot match, because what upstream prints there is
//! a Python object's `repr` with its memory address in it, or a whole track:
//! a source clip whose mob is found but whose slot is not, a keyframe that
//! cannot be read, and a marker whose target cannot be measured. Those say
//! the same thing in words of their own.

use std::fmt::Write as _;
use std::sync::Arc;

use opentime::{RationalTime, TimeRange};
use otio_core::{Any, AnyDictionary};

/// Where the lines of upstream's `transcribe_log` go.
///
/// Each call is handed what one Python `print` in upstream prints, without
/// its newline. Most are one line; the three that announce which mobs are
/// read have newlines in them, as upstream's do.
///
/// ```
/// use std::sync::{Arc, Mutex};
///
/// let lines = Arc::new(Mutex::new(Vec::new()));
/// let sink = Arc::clone(&lines);
/// let options = otio_aaf::ReadOptions::new().with_transcribe_log(
///     otio_aaf::TranscribeLog::new(move |line| sink.lock().unwrap().push(line.to_owned())),
/// );
/// # let _ = options;
/// ```
#[derive(Clone)]
pub struct TranscribeLog(Arc<dyn Fn(&str) + Send + Sync>);

impl TranscribeLog {
    /// A log handing each line to `sink`.
    pub fn new(sink: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self(Arc::new(sink))
    }

    /// A log printing each line to standard output, as upstream's does.
    #[must_use]
    pub fn stdout() -> Self {
        Self::new(|line| println!("{line}"))
    }

    pub(crate) fn write(&self, line: &str) {
        (self.0)(line);
    }
}

impl std::fmt::Debug for TranscribeLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TranscribeLog").finish_non_exhaustive()
    }
}

/// Upstream's `_encoded_name`, as its log prints it: the `repr` of the
/// name's UTF-8 bytes.
pub(crate) fn bytes_repr(text: &str) -> String {
    let bytes = text.as_bytes();
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(bytes.len() + 3);
    out.push('b');
    out.push(quote);
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            _ if char::from(byte) == quote => {
                out.push('\\');
                out.push(quote);
            }
            0x20..0x7f => out.push(char::from(byte)),
            _ => {
                let _ = write!(out, "\\x{byte:02x}");
            }
        }
    }
    out.push(quote);
    out
}

/// Python's `repr` of a string.
///
/// Python keeps a character that `str.isprintable` accepts and escapes the
/// rest. That needs Unicode's categories, which the standard library does
/// not carry, so this escapes the control characters and the separators
/// and formatting characters that come up in practice, and keeps the rest.
pub(crate) fn str_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if c == quote => {
                out.push('\\');
                out.push(quote);
            }
            _ if !printable(c) => {
                let code = u32::from(c);
                let _ = match code {
                    0..=0xff => write!(out, "\\x{code:02x}"),
                    0x100..=0xffff => write!(out, "\\u{code:04x}"),
                    _ => write!(out, "\\U{code:08x}"),
                };
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Whether Python prints a character as it is, as far as this can tell
/// without Unicode's tables.
fn printable(c: char) -> bool {
    !matches!(u32::from(c),
        0x00..=0x1f | 0x7f..=0xa0 | 0xad
        | 0x0600..=0x0605 | 0x061c | 0x06dd | 0x070f
        | 0x1680 | 0x180e | 0x2000..=0x200f | 0x2028..=0x202f
        | 0x205f..=0x2064 | 0x2066..=0x206f | 0x3000
        | 0xd800..=0xf8ff | 0xfeff | 0xfff9..=0xfffb
        | 0xe0000..=0xe0fff | 0xf0000..)
}

/// Python's `repr` of a float: the shortest digits that read back, with the
/// exponent written as Python writes it.
pub(crate) fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    // Rust's `Debug` picks the same digits and switches to an exponent at
    // the same points, and only spells the exponent differently.
    let text = format!("{value:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            let (sign, digits) = match exponent.strip_prefix('-') {
                Some(digits) => ('-', digits),
                None => ('+', exponent),
            };
            format!("{mantissa}e{sign}{digits:0>2}")
        }
        None => text,
    }
}

/// C's `%g`: six significant digits, trailing zeros dropped. OTIO's
/// `str(RationalTime)` prints both numbers this way.
pub(crate) fn g_format(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    const PRECISION: i32 = 6;
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("`{:e}` always writes an exponent");
    let exponent: i32 = exponent.parse().expect("`{:e}` writes a number");
    if (-4..PRECISION).contains(&exponent) {
        let decimals = usize::try_from(PRECISION - 1 - exponent).unwrap_or_default();
        trim_zeros(&format!("{value:.decimals$}"))
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            trim_zeros(mantissa),
            exponent.unsigned_abs()
        )
    }
}

fn trim_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text.to_owned()
    }
}

/// OTIO's `str(RationalTime)`.
pub(crate) fn time_str(time: RationalTime) -> String {
    format!(
        "RationalTime({}, {})",
        g_format(time.value()),
        g_format(time.rate())
    )
}

/// OTIO's `str(TimeRange)`.
pub(crate) fn range_str(range: TimeRange) -> String {
    format!(
        "TimeRange({}, {})",
        time_str(range.start_time()),
        time_str(range.duration())
    )
}

/// OTIO's `str(marker)`: `Marker(name, range, metadata)`.
pub(crate) fn marker_str(name: &str, range: TimeRange, metadata: &AnyDictionary) -> String {
    format!(
        "Marker({name}, {}, {})",
        range_str(range),
        dict_repr(metadata)
    )
}

/// Python's `repr` of a `dict` of OTIO metadata, which is what OTIO's
/// `str` of one gives.
pub(crate) fn dict_repr(dictionary: &AnyDictionary) -> String {
    let mut out = String::from("{");
    for (index, (key, value)) in dictionary.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&str_repr(key));
        out.push_str(": ");
        out.push_str(&any_repr(value));
    }
    out.push('}');
    out
}

/// Python's `repr` of a metadata value as OTIO hands it to Python.
pub(crate) fn any_repr(value: &Any) -> String {
    match value {
        Any::Null => "None".to_owned(),
        Any::Bool(true) => "True".to_owned(),
        Any::Bool(false) => "False".to_owned(),
        Any::Int(v) => v.to_string(),
        Any::UInt(v) => v.to_string(),
        Any::Double(v) => float_repr(*v),
        Any::String(text) => str_repr(text),
        Any::RationalTime(time) => format!(
            "otio.opentime.RationalTime(value={}, rate={})",
            g_format(time.value()),
            g_format(time.rate())
        ),
        Any::TimeRange(range) => format!(
            "otio.opentime.TimeRange(start_time={}, duration={})",
            any_repr(&Any::RationalTime(range.start_time())),
            any_repr(&Any::RationalTime(range.duration()))
        ),
        Any::Vector(items) => {
            let items: Vec<String> = items.iter().map(any_repr).collect();
            format!("[{}]", items.join(", "))
        }
        Any::Dictionary(dictionary) => dict_repr(dictionary),
        // Nothing the AAF reader puts in a marker's metadata is anything
        // else, so these need only say what they are.
        other => format!("<{}>", any_kind(other)),
    }
}

fn any_kind(value: &Any) -> &'static str {
    match value {
        Any::TimeTransform(_) => "TimeTransform",
        Any::Color(_) => "Color",
        Any::V2d(_) => "V2d",
        Any::Box2d(_) => "Box2d",
        _ => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_print_as_python_prints_their_bytes() {
        assert_eq!(bytes_repr("Sequence"), "b'Sequence'");
        assert_eq!(bytes_repr("it's"), "b\"it's\"");
        assert_eq!(bytes_repr("a'\"b"), "b'a\\'\"b'");
        assert_eq!(bytes_repr("é\n"), "b'\\xc3\\xa9\\n'");
    }

    #[test]
    fn strings_print_as_python_prints_them() {
        assert_eq!(str_repr("it's"), "\"it's\"");
        assert_eq!(str_repr("say \"it's\""), "'say \"it\\'s\"'");
        assert_eq!(str_repr("ñ∑\u{1}"), "'ñ∑\\x01'");
    }

    #[test]
    fn floats_print_as_python_prints_them() {
        assert_eq!(float_repr(54.0), "54.0");
        assert_eq!(float_repr(0.1), "0.1");
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(1e-5), "1e-05");
        assert_eq!(float_repr(1.5e300), "1.5e+300");
    }

    #[test]
    fn times_print_as_c_prints_them_with_percent_g() {
        assert_eq!(g_format(54.0), "54");
        assert_eq!(g_format(23.976), "23.976");
        assert_eq!(g_format(1_234_567.5), "1.23457e+06");
        assert_eq!(g_format(0.000_012_5), "1.25e-05");
        assert_eq!(g_format(0.5), "0.5");
    }
}

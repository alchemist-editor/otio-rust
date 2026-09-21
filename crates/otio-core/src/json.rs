//! A JSON parser and string escaper matching what OpenTimelineIO reads and
//! writes.
//!
//! OTIO files are not quite JSON. Upstream parses with RapidJSON's
//! `kParseNanAndInfFlag` and writes with `kWriteNanAndInfFlag`, so `NaN`,
//! `Inf`, `Infinity` and their negatives appear as bare literals — upstream's
//! own `tests/sample_data/big_int.otio` contains all three. A strict JSON
//! library refuses those files outright, which is why this module exists
//! rather than a dependency.
//!
//! It also preserves the distinction OTIO's data model makes between signed
//! integers, unsigned integers and doubles, which a parser that funnels every
//! number through `f64` would lose.

use std::fmt;

/// A JSON number, keeping the distinction OTIO's data model makes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    /// An integer that fits in an `i64`.
    Int(i64),
    /// A positive integer too large for an `i64`.
    UInt(u64),
    /// Anything else, including integers too large for a `u64`.
    ///
    /// RapidJSON widens an out-of-range integer to a double in the same way.
    Double(f64),
}

impl Number {
    /// Returns this number as an `f64`, for comparing values that may have
    /// been written in different forms.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::UInt(value) => value as f64,
            Self::Double(value) => value,
        }
    }
}

/// A parsed JSON value.
///
/// Object keys keep the order they appeared in, which makes error paths and
/// diffs read the way the file does.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(Number),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object, in source order.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Names this value's type, for error messages.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// Returns the string inside, if this is a [`Value::String`].
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the boolean inside, if this is a [`Value::Bool`].
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the object's entries, if this is a [`Value::Object`].
    #[must_use]
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    /// Returns the array's entries, if this is a [`Value::Array`].
    #[must_use]
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(entries) => Some(entries),
            _ => None,
        }
    }

    /// Looks a key up in an object.
    ///
    /// Returns `None` for a non-object, and for a key that is absent. A
    /// duplicate key resolves to the first occurrence.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Returns whether this is [`Value::Null`].
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// A failure to parse JSON, with the position it happened at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong.
    pub message: String,
    /// The 1-based line the parser stopped on.
    pub line: usize,
    /// The 1-based column the parser stopped on.
    pub column: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for ParseError {}

/// Parses a JSON document, accepting the `NaN` and `Infinity` literals that
/// OpenTimelineIO writes.
///
/// # Errors
///
/// Returns a [`ParseError`] naming the line and column if the input is not
/// well formed, or if anything but whitespace follows the top-level value.
pub fn parse(input: &str) -> Result<Value, ParseError> {
    let mut parser = Parser {
        bytes: input.as_bytes(),
        position: 0,
    };
    parser.skip_whitespace();
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.position < parser.bytes.len() {
        return Err(parser.error("trailing characters after the top-level value"));
    }
    Ok(value)
}

/// Escapes a string as a JSON string literal, quotes included.
///
/// Only the characters JSON requires are escaped; text outside ASCII is left
/// as UTF-8, which is what upstream writes.
#[must_use]
pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < '\u{20}' => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> ParseError {
        let consumed = &self.bytes[..self.position.min(self.bytes.len())];
        let line = consumed.iter().filter(|byte| **byte == b'\n').count() + 1;
        let column = consumed
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(self.position, |index| self.position - index - 1)
            + 1;
        ParseError {
            message: message.to_string(),
            line,
            column,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(byte) = self.peek() {
            if byte.is_ascii_whitespace() {
                self.position += 1;
            } else {
                break;
            }
        }
    }

    /// Consumes `literal` if it is next.
    fn eat(&mut self, literal: &str) -> bool {
        if self.bytes[self.position..].starts_with(literal.as_bytes()) {
            self.position += literal.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ParseError> {
        if self.peek() == Some(byte) {
            self.position += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected '{}'", byte as char)))
        }
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            None => Err(self.error("unexpected end of input")),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Value::String(self.parse_string()?)),
            Some(b't') if self.eat("true") => Ok(Value::Bool(true)),
            Some(b'f') if self.eat("false") => Ok(Value::Bool(false)),
            Some(b'n') if self.eat("null") => Ok(Value::Null),
            // RapidJSON's NaN and infinity extensions, in the spellings it
            // accepts. "Infinity" has to be tried before "Inf".
            Some(b'N') if self.eat("NaN") => Ok(Value::Number(Number::Double(f64::NAN))),
            Some(b'I') if self.eat("Infinity") || self.eat("Inf") => {
                Ok(Value::Number(Number::Double(f64::INFINITY)))
            }
            Some(b'-') if self.looks_like_negative_infinity() => {
                self.position += 1;
                let _ = self.eat("Infinity") || self.eat("Inf");
                Ok(Value::Number(Number::Double(f64::NEG_INFINITY)))
            }
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.parse_number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn looks_like_negative_infinity(&self) -> bool {
        let rest = &self.bytes[self.position + 1..];
        rest.starts_with(b"Infinity") || rest.starts_with(b"Inf")
    }

    fn parse_object(&mut self) -> Result<Value, ParseError> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Value::Object(entries));
        }

        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.parse_value()?;
            entries.push((key, value));

            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Value::Object(entries));
                }
                _ => return Err(self.error("expected ',' or '}' in object")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, ParseError> {
        self.expect(b'[')?;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Value::Array(entries));
        }

        loop {
            self.skip_whitespace();
            entries.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(Value::Array(entries));
                }
                _ => return Err(self.error("expected ',' or ']' in array")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.position += 1;
                    self.parse_escape(&mut out)?;
                }
                _ => {
                    // Copy the whole UTF-8 sequence, not just this byte.
                    let start = self.position;
                    let width = utf8_width(byte);
                    if start + width > self.bytes.len() {
                        return Err(self.error("truncated UTF-8 sequence"));
                    }
                    let text = std::str::from_utf8(&self.bytes[start..start + width])
                        .map_err(|_| self.error("invalid UTF-8 in string"))?;
                    out.push_str(text);
                    self.position += width;
                }
            }
        }
    }

    fn parse_escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        let Some(byte) = self.peek() else {
            return Err(self.error("unterminated escape sequence"));
        };
        self.position += 1;
        match byte {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{08}'),
            b'f' => out.push('\u{0c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = self.parse_hex4()?;
                // A character outside the basic plane is written as a
                // surrogate pair, which has to be recombined.
                let code = if (0xD800..0xDC00).contains(&first) {
                    if !self.eat("\\u") {
                        return Err(self.error("expected a low surrogate"));
                    }
                    let second = self.parse_hex4()?;
                    if !(0xDC00..0xE000).contains(&second) {
                        return Err(self.error("invalid low surrogate"));
                    }
                    0x1_0000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00)
                } else {
                    u32::from(first)
                };
                out.push(
                    char::from_u32(code)
                        .ok_or_else(|| self.error("escape names no Unicode character"))?,
                );
            }
            _ => return Err(self.error("unrecognized escape sequence")),
        }
        Ok(())
    }

    fn parse_hex4(&mut self) -> Result<u16, ParseError> {
        if self.position + 4 > self.bytes.len() {
            return Err(self.error("truncated \\u escape"));
        }
        let digits = std::str::from_utf8(&self.bytes[self.position..self.position + 4])
            .map_err(|_| self.error("invalid \\u escape"))?;
        let value =
            u16::from_str_radix(digits, 16).map_err(|_| self.error("invalid \\u escape"))?;
        self.position += 4;
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, ParseError> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        let mut is_integer = true;
        while let Some(byte) = self.peek() {
            match byte {
                b'0'..=b'9' => self.position += 1,
                b'.' | b'e' | b'E' => {
                    is_integer = false;
                    self.position += 1;
                }
                b'+' | b'-' => self.position += 1,
                _ => break,
            }
        }

        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| self.error("invalid number"))?;

        if is_integer {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Value::Number(Number::Int(value)));
            }
            if let Ok(value) = text.parse::<u64>() {
                return Ok(Value::Number(Number::UInt(value)));
            }
            // Too large for either: widen to a double, as RapidJSON does.
        }

        text.parse::<f64>()
            .map(|value| Value::Number(Number::Double(value)))
            .map_err(|_| self.error("invalid number"))
    }
}

/// Returns the length in bytes of the UTF-8 sequence starting with `byte`.
const fn utf8_width(byte: u8) -> usize {
    match byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::{Number, Value, escape, parse};

    #[test]
    fn parses_the_basics() {
        assert_eq!(parse("null").unwrap(), Value::Null);
        assert_eq!(parse("true").unwrap(), Value::Bool(true));
        assert_eq!(parse(r#""hi""#).unwrap(), Value::String("hi".to_string()));
        assert_eq!(
            parse("[1, 2]").unwrap(),
            Value::Array(vec![
                Value::Number(Number::Int(1)),
                Value::Number(Number::Int(2))
            ])
        );
    }

    #[test]
    fn keeps_object_key_order() {
        let value = parse(r#"{"b": 1, "a": 2}"#).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, _)| key.as_str())
            .collect();
        assert_eq!(keys, ["b", "a"]);
    }

    #[test]
    fn accepts_the_nan_and_infinity_literals_otio_writes() {
        let value =
            parse(r#"{"a": NaN, "b": Inf, "c": Infinity, "d": -Infinity, "e": -Inf}"#).unwrap();
        let number = |key: &str| match value.get(key) {
            Some(Value::Number(Number::Double(inner))) => *inner,
            other => panic!("{key}: {other:?}"),
        };
        assert!(number("a").is_nan());
        assert_eq!(number("b"), f64::INFINITY);
        assert_eq!(number("c"), f64::INFINITY);
        assert_eq!(number("d"), f64::NEG_INFINITY);
        assert_eq!(number("e"), f64::NEG_INFINITY);
    }

    #[test]
    fn keeps_integers_distinct_from_doubles() {
        assert_eq!(parse("1").unwrap(), Value::Number(Number::Int(1)));
        assert_eq!(parse("1.0").unwrap(), Value::Number(Number::Double(1.0)));
        assert_eq!(
            parse("-2147483648").unwrap(),
            Value::Number(Number::Int(-2_147_483_648))
        );
        assert_eq!(
            parse("18446744073709551615").unwrap(),
            Value::Number(Number::UInt(u64::MAX))
        );
    }

    #[test]
    fn widens_an_out_of_range_integer_to_a_double() {
        // As RapidJSON does. upstream's big_int.otio carries a 256-bit value.
        let huge = "57896044618658097711785492504343953926634992332820282019728792003956564819968";
        let Value::Number(Number::Double(value)) = parse(huge).unwrap() else {
            panic!("expected a double");
        };
        assert!(value > 1e76);
    }

    #[test]
    fn handles_unicode_and_escapes() {
        assert_eq!(
            parse(r#""Viel glück und hab spaß!""#).unwrap(),
            Value::String("Viel glück und hab spaß!".to_string())
        );
        // A surrogate pair for U+1F3AC CLAPPER BOARD.
        assert_eq!(
            parse(r#""🎬""#).unwrap(),
            Value::String("\u{1F3AC}".to_string())
        );
        // Raw UTF-8 passes through untouched.
        assert_eq!(
            parse("\"Viel glück\"").unwrap(),
            Value::String("Viel glück".to_string())
        );
    }

    #[test]
    fn escapes_only_what_json_requires() {
        assert_eq!(escape("plain"), r#""plain""#);
        assert_eq!(escape("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(escape("line\nbreak"), r#""line\nbreak""#);
        assert_eq!(escape("\u{1}"), r#""\u0001""#);
        // Non-ASCII stays as UTF-8, matching upstream's output.
        assert_eq!(escape("glück"), "\"glück\"");
    }

    #[test]
    fn reports_where_a_parse_failed() {
        let error = parse("{\n  \"a\": }").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.to_string().contains("line 2"));
    }

    #[test]
    fn rejects_trailing_content() {
        assert!(parse("{} {}").is_err());
    }
}

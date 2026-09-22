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
///
/// The message and the position are RapidJSON's, because upstream hands both
/// to its callers: `message` is what `GetParseError_En` says for the error,
/// and `line` and `column` are where RapidJSON's `CursorStreamWrapper` had
/// got to when it stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What went wrong, in RapidJSON's words, such as `Invalid value.`.
    pub message: String,
    /// The 1-based line the parser stopped on.
    pub line: usize,
    /// How many bytes of that line the parser had read when it stopped:
    /// RapidJSON counts columns from 0, and in bytes rather than characters.
    pub column: usize,
}

impl fmt::Display for ParseError {
    /// Upstream's wording, from `deserialize_json_from_string`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "JSON parse error on input string: {} (line {}, column {})",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for ParseError {}

/// RapidJSON's parse errors, each with the message `GetParseError_En` gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Code {
    DocumentEmpty,
    DocumentRootNotSingular,
    ValueInvalid,
    ObjectMissName,
    ObjectMissColon,
    ObjectMissCommaOrCurlyBracket,
    ArrayMissCommaOrSquareBracket,
    StringUnicodeEscapeInvalidHex,
    StringUnicodeSurrogateInvalid,
    StringEscapeInvalid,
    StringMissQuotationMark,
    StringInvalidEncoding,
    NumberTooBig,
    NumberMissFraction,
    NumberMissExponent,
}

impl Code {
    const fn message(self) -> &'static str {
        match self {
            Self::DocumentEmpty => "The document is empty.",
            Self::DocumentRootNotSingular => {
                "The document root must not be followed by other values."
            }
            Self::ValueInvalid => "Invalid value.",
            Self::ObjectMissName => "Missing a name for object member.",
            Self::ObjectMissColon => "Missing a colon after a name of object member.",
            Self::ObjectMissCommaOrCurlyBracket => "Missing a comma or '}' after an object member.",
            Self::ArrayMissCommaOrSquareBracket => "Missing a comma or ']' after an array element.",
            Self::StringUnicodeEscapeInvalidHex => {
                "Incorrect hex digit after \\u escape in string."
            }
            Self::StringUnicodeSurrogateInvalid => "The surrogate pair in string is invalid.",
            Self::StringEscapeInvalid => "Invalid escape character in string.",
            Self::StringMissQuotationMark => "Missing a closing quotation mark in string.",
            Self::StringInvalidEncoding => "Invalid encoding in string.",
            Self::NumberTooBig => "Number too big to be stored in double.",
            Self::NumberMissFraction => "Miss fraction part in number.",
            Self::NumberMissExponent => "Miss exponent in number.",
        }
    }
}

/// Parses a JSON document, accepting the `NaN` and `Infinity` literals that
/// OpenTimelineIO writes.
///
/// The grammar is RapidJSON's with `kParseNanAndInfFlag`, as upstream parses:
/// the same four whitespace characters, the same number syntax (no leading
/// zeros, a digit on both sides of the point), and a NUL byte ends the input
/// as it ends the C string upstream hands RapidJSON.
///
/// # Errors
///
/// Returns a [`ParseError`] with RapidJSON's message and position if the
/// input is not well formed, or if anything but whitespace follows the
/// top-level value.
pub fn parse(input: &str) -> Result<Value, ParseError> {
    Parser::new(input, false).parse()
}

/// Where each object in a parsed document ends, for error messages.
///
/// Upstream says "near line N" of an object it could not read, where N is the
/// line of the object's closing brace. The parsed [`Value`] does not carry
/// positions, so [`parse_with_object_lines`] records them beside it, keyed by
/// the address of each object's entries.
#[derive(Debug, Default)]
pub(crate) struct ObjectLines(std::collections::HashMap<usize, usize>);

impl ObjectLines {
    /// The line an object's closing brace sits on.
    ///
    /// Only meaningful for an object from the document these were recorded
    /// for, and not for an empty one, whose entries have no address of their
    /// own.
    pub(crate) fn line_of(&self, entries: &[(String, Value)]) -> Option<usize> {
        self.0.get(&(entries.as_ptr() as usize)).copied()
    }
}

/// Parses a document as [`parse`] does, noting where each object ends.
///
/// Slower than [`parse`], so it is only used to explain a document that has
/// already failed to read.
pub(crate) fn parse_with_object_lines(input: &str) -> Result<(Value, ObjectLines), ParseError> {
    let mut parser = Parser::new(input, true);
    let value = parser.parse()?;
    Ok((value, parser.object_lines))
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
                out.push_str(&format!("\\u{:04X}", control as u32));
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
    /// How many newlines have been read. Only whitespace can hold one: a raw
    /// newline inside a string is an error.
    newlines: usize,
    /// Whether to note where each object ends.
    record: bool,
    object_lines: ObjectLines,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, record: bool) -> Self {
        let bytes = input.as_bytes();
        // Upstream hands RapidJSON `input.c_str()`, so a NUL ends the input.
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        Self {
            bytes: &bytes[..end],
            position: 0,
            newlines: 0,
            record,
            object_lines: ObjectLines::default(),
        }
    }

    fn parse(&mut self) -> Result<Value, ParseError> {
        self.skip_whitespace();
        if self.peek() == 0 {
            return Err(self.error(Code::DocumentEmpty));
        }
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.peek() != 0 {
            return Err(self.error(Code::DocumentRootNotSingular));
        }
        Ok(value)
    }

    /// The line the parser is on: one more than the newlines it has read.
    const fn line(&self) -> usize {
        self.newlines + 1
    }

    /// An error at the current position, counted as RapidJSON's
    /// `CursorStreamWrapper` counts: lines from 1, and columns as the number
    /// of bytes read since the last newline.
    fn error(&self, code: Code) -> ParseError {
        let consumed = &self.bytes[..self.position];
        let column = consumed
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(self.position, |index| self.position - index - 1);
        ParseError {
            message: code.message().to_string(),
            line: self.line(),
            column,
        }
    }

    /// The next byte, or 0 at the end, as RapidJSON's streams report it.
    fn peek(&self) -> u8 {
        self.bytes.get(self.position).copied().unwrap_or(0)
    }

    fn take(&mut self) -> u8 {
        let byte = self.peek();
        if self.position < self.bytes.len() {
            self.position += 1;
        }
        byte
    }

    /// Takes `byte` if it is next.
    fn consume(&mut self, byte: u8) -> bool {
        if self.position < self.bytes.len() && self.peek() == byte {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        loop {
            match self.peek() {
                b'\n' => self.newlines += 1,
                b' ' | b'\r' | b'\t' => {}
                _ => return,
            }
            self.position += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            b'n' => self.parse_literal(b"ull", Value::Null),
            b't' => self.parse_literal(b"rue", Value::Bool(true)),
            b'f' => self.parse_literal(b"alse", Value::Bool(false)),
            b'"' => Ok(Value::String(self.parse_string()?)),
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            _ => self.parse_number(),
        }
    }

    /// Reads `null`, `true` or `false`, whose first letter is next.
    fn parse_literal(&mut self, rest: &[u8], value: Value) -> Result<Value, ParseError> {
        self.take();
        for byte in rest {
            if !self.consume(*byte) {
                return Err(self.error(Code::ValueInvalid));
            }
        }
        Ok(value)
    }

    fn parse_object(&mut self) -> Result<Value, ParseError> {
        self.take();
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.consume(b'}') {
            return Ok(Value::Object(entries));
        }

        loop {
            if self.peek() != b'"' {
                return Err(self.error(Code::ObjectMissName));
            }
            let key = self.parse_string()?;
            self.skip_whitespace();
            if !self.consume(b':') {
                return Err(self.error(Code::ObjectMissColon));
            }
            self.skip_whitespace();
            let value = self.parse_value()?;
            entries.push((key, value));
            self.skip_whitespace();

            match self.peek() {
                b',' => {
                    self.take();
                    self.skip_whitespace();
                }
                b'}' => {
                    self.take();
                    if self.record {
                        // Moving the vector into the value keeps its buffer
                        // where it is, so its address names this object.
                        let line = self.line();
                        self.object_lines.0.insert(entries.as_ptr() as usize, line);
                    }
                    return Ok(Value::Object(entries));
                }
                _ => return Err(self.error(Code::ObjectMissCommaOrCurlyBracket)),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, ParseError> {
        self.take();
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.consume(b']') {
            return Ok(Value::Array(entries));
        }

        loop {
            entries.push(self.parse_value()?);
            self.skip_whitespace();
            if self.consume(b',') {
                self.skip_whitespace();
            } else if self.consume(b']') {
                return Ok(Value::Array(entries));
            } else {
                return Err(self.error(Code::ArrayMissCommaOrSquareBracket));
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.take();
        let mut out = Vec::new();
        loop {
            match self.peek() {
                b'\\' => {
                    self.take();
                    let escaped = match self.peek() {
                        b'"' => b'"',
                        b'\\' => b'\\',
                        b'/' => b'/',
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        b'u' => {
                            self.take();
                            let character = self.parse_unicode_escape()?;
                            let mut buffer = [0; 4];
                            out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                            continue;
                        }
                        _ => return Err(self.error(Code::StringEscapeInvalid)),
                    };
                    self.take();
                    out.push(escaped);
                }
                b'"' => {
                    self.take();
                    // Everything copied was either whole UTF-8 from a `str`
                    // or an encoded character, so this cannot fail.
                    return String::from_utf8(out)
                        .map_err(|_| self.error(Code::StringInvalidEncoding));
                }
                0 => return Err(self.error(Code::StringMissQuotationMark)),
                control if control < 0x20 => {
                    return Err(self.error(Code::StringInvalidEncoding));
                }
                byte => {
                    self.take();
                    out.push(byte);
                }
            }
        }
    }

    /// Reads the four hex digits after `\u`, and a second escape when the
    /// first is the high half of a surrogate pair.
    fn parse_unicode_escape(&mut self) -> Result<char, ParseError> {
        let first = self.parse_hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.consume(b'\\') || !self.consume(b'u') {
                    return Err(self.error(Code::StringUnicodeSurrogateInvalid));
                }
                let second = self.parse_hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return Err(self.error(Code::StringUnicodeSurrogateInvalid));
                }
                (((first - 0xD800) << 10) | (second - 0xDC00)) + 0x1_0000
            }
            // A low half on its own.
            0xDC00..=0xDFFF => return Err(self.error(Code::StringUnicodeSurrogateInvalid)),
            code => code,
        };
        char::from_u32(code).ok_or_else(|| self.error(Code::StringUnicodeSurrogateInvalid))
    }

    fn parse_hex4(&mut self) -> Result<u32, ParseError> {
        let mut code = 0;
        for _ in 0..4 {
            let digit = char::from(self.peek())
                .to_digit(16)
                .ok_or_else(|| self.error(Code::StringUnicodeEscapeInvalidHex))?;
            code = code * 16 + digit;
            self.take();
        }
        Ok(code)
    }

    /// Reads a number the way RapidJSON's `ParseNumber` does.
    ///
    /// The bookkeeping — `significand_digits` and `exp_frac` — is RapidJSON's
    /// too. It decides how many exponent digits are read before a number is
    /// declared too big, and so where the error is reported; the value itself
    /// comes from Rust's exact parse of the text read.
    fn parse_number(&mut self) -> Result<Value, ParseError> {
        let start = self.position;
        let minus = self.consume(b'-');

        let mut small: u32 = 0;
        let mut large: u64 = 0;
        let mut use_64bit = false;
        let mut use_double = false;
        let mut significand_digits = 0;
        let is_digit = |byte: u8| byte.is_ascii_digit();

        if self.peek() == b'0' {
            self.take();
        } else if (b'1'..=b'9').contains(&self.peek()) {
            small = u32::from(self.take() - b'0');
            let limit = if minus { 214_748_364 } else { 429_496_729 };
            let last = if minus { b'8' } else { b'5' };
            while is_digit(self.peek()) {
                if small >= limit && (small != limit || self.peek() > last) {
                    large = u64::from(small);
                    use_64bit = true;
                    break;
                }
                small = small * 10 + u32::from(self.take() - b'0');
                significand_digits += 1;
            }
        } else if matches!(self.peek(), b'I' | b'N') {
            let mut value = None;
            if self.consume(b'N') {
                if self.consume(b'a') && self.consume(b'N') {
                    value = Some(f64::NAN);
                }
            } else if self.consume(b'I') && self.consume(b'n') && self.consume(b'f') {
                value = Some(if minus {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                });
                if self.peek() == b'i' && !b"inity".iter().all(|byte| self.consume(*byte)) {
                    return Err(self.error(Code::ValueInvalid));
                }
            }
            return value
                .map(|value| Value::Number(Number::Double(value)))
                .ok_or_else(|| self.error(Code::ValueInvalid));
        } else {
            return Err(self.error(Code::ValueInvalid));
        }

        if use_64bit {
            let limit: u64 = if minus {
                0x0CCC_CCCC_CCCC_CCCC
            } else {
                0x1999_9999_9999_9999
            };
            let last = if minus { b'8' } else { b'5' };
            while is_digit(self.peek()) {
                if large >= limit && (large != limit || self.peek() > last) {
                    use_double = true;
                    break;
                }
                large = large * 10 + u64::from(self.take() - b'0');
                significand_digits += 1;
            }
        }
        if use_double {
            while is_digit(self.peek()) {
                self.take();
            }
        }

        let mut exp_frac: i32 = 0;
        if self.consume(b'.') {
            if !is_digit(self.peek()) {
                return Err(self.error(Code::NumberMissFraction));
            }
            // With a double, RapidJSON counts only the digits that fit in its
            // 17-digit significand.
            let mut significand = if use_double { 1.0 } else { 0.0 };
            if !use_double {
                if !use_64bit {
                    large = u64::from(small);
                }
                while is_digit(self.peek()) {
                    if large > 0x1F_FFFF_FFFF_FFFF {
                        break;
                    }
                    large = large * 10 + u64::from(self.take() - b'0');
                    exp_frac -= 1;
                    if large != 0 {
                        significand_digits += 1;
                    }
                }
                significand = large as f64;
                use_double = true;
            }
            while is_digit(self.peek()) {
                if significand_digits < 17 {
                    significand = significand * 10.0 + f64::from(self.take() - b'0');
                    exp_frac -= 1;
                    if significand > 0.0 {
                        significand_digits += 1;
                    }
                } else {
                    self.take();
                }
            }
        }

        if self.consume(b'e') || self.consume(b'E') {
            use_double = true;
            let exp_minus = if self.consume(b'+') {
                false
            } else {
                self.consume(b'-')
            };
            if !is_digit(self.peek()) {
                return Err(self.error(Code::NumberMissExponent));
            }
            let mut exp = i32::from(self.take() - b'0');
            if exp_minus {
                let max_exp = (exp_frac + 2_147_483_639) / 10;
                while is_digit(self.peek()) {
                    exp = exp * 10 + i32::from(self.take() - b'0');
                    if exp > max_exp {
                        while is_digit(self.peek()) {
                            self.take();
                        }
                    }
                }
            } else {
                let max_exp = 308 - exp_frac;
                while is_digit(self.peek()) {
                    exp = exp * 10 + i32::from(self.take() - b'0');
                    if exp > max_exp {
                        return Err(self.error(Code::NumberTooBig));
                    }
                }
            }
        }

        // Only ASCII digits, signs, points and exponents were read.
        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| self.error(Code::ValueInvalid))?;

        if !use_double {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Value::Number(Number::Int(value)));
            }
            if let Ok(value) = text.parse::<u64>() {
                return Ok(Value::Number(Number::UInt(value)));
            }
        }
        let value: f64 = text.parse().map_err(|_| self.error(Code::ValueInvalid))?;
        if value.is_infinite() {
            return Err(self.error(Code::NumberTooBig));
        }
        Ok(Value::Number(Number::Double(value)))
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
        // Upstream writes the hex digits in upper case.
        assert_eq!(escape("\u{1f}"), r#""\u001F""#);
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

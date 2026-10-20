//! JSON kept as the text it arrived as.
//!
//! Furnace's `otiodiff` never re-encodes a value it did not change: a patch
//! carries the bytes of the node or value it was cut from, and Go's encoder
//! only compacts them on the way out. Doing the same here is what lets Rust
//! and Furnace write identical bytes for the same patch, so this module works
//! on JSON text rather than on [`otio_core::json::Value`], which would forget
//! how a number was spelled.
//!
//! Every function here except [`is_valid`] expects text that [`is_valid`] has
//! already accepted, and does not check it again.

use std::collections::BTreeMap;

use otio_core::json::{self, Number, Value};

/// Returns whether `text` is one JSON document, as Go's `json.Valid` decides.
///
/// otio-core's parser accepts the `NaN` and `Infinity` literals that
/// OpenTimelineIO writes and stops at a NUL byte, as upstream does; Go
/// accepts neither, so both are refused here.
pub(crate) fn is_valid(text: &str) -> bool {
    if text.as_bytes().contains(&0) {
        return false;
    }
    match json::parse(text) {
        Ok(value) => all_finite(&value),
        Err(_) => false,
    }
}

fn all_finite(value: &Value) -> bool {
    match value {
        Value::Number(Number::Double(number)) => number.is_finite(),
        Value::Array(values) => values.iter().all(all_finite),
        Value::Object(entries) => entries.iter().all(|(_, value)| all_finite(value)),
        _ => true,
    }
}

/// Skips JSON whitespace from `i`.
pub(crate) fn skip_space(data: &[u8], mut i: usize) -> usize {
    while i < data.len() && matches!(data[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// Returns the index just past the string literal that starts at `i`.
pub(crate) fn skip_string(data: &[u8], mut i: usize) -> Option<usize> {
    if i >= data.len() || data[i] != b'"' {
        return None;
    }
    i += 1;
    while i < data.len() {
        match data[i] {
            b'\\' => {
                i += 1;
                if i >= data.len() {
                    return None;
                }
            }
            b'"' => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Returns the index just past the value that starts at `i`.
pub(crate) fn skip_value(data: &[u8], mut i: usize) -> Option<usize> {
    if i >= data.len() {
        return None;
    }
    if data[i] == b'"' {
        return skip_string(data, i);
    }
    if data[i] != b'{' && data[i] != b'[' {
        while i < data.len() {
            match data[i] {
                b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n' => return Some(i),
                _ => i += 1,
            }
        }
        return Some(i);
    }
    let mut depth = 0usize;
    while i < data.len() {
        match data[i] {
            b'"' => {
                i = skip_string(data, i)?;
                continue;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Finds the bytes `key`'s value occupies in a JSON object.
///
/// Like Furnace, this recognises only the plain quoted spelling of `key`; a
/// key spelled with escapes misses this fast path and is compared
/// semantically instead.
pub(crate) fn top_level_value_bounds(data: &[u8], key: &str) -> Option<(usize, usize)> {
    let mut i = skip_space(data, 0);
    if i >= data.len() || data[i] != b'{' {
        return None;
    }
    i += 1;
    loop {
        i = skip_space(data, i);
        if i >= data.len() || data[i] == b'}' {
            return None;
        }
        let key_start = i;
        let key_end = skip_string(data, i)?;
        i = skip_space(data, key_end);
        if i >= data.len() || data[i] != b':' {
            return None;
        }
        let value_start = skip_space(data, i + 1);
        let value_end = skip_value(data, value_start)?;
        let spelled = &data[key_start..key_end];
        if spelled.len() == key.len() + 2
            && spelled[0] == b'"'
            && spelled[spelled.len() - 1] == b'"'
            && &spelled[1..spelled.len() - 1] == key.as_bytes()
        {
            return Some((value_start, value_end));
        }
        i = skip_space(data, value_end);
        if i >= data.len() || data[i] != b',' {
            return None;
        }
        i += 1;
    }
}

/// The value in `text`, without the whitespace around it.
pub(crate) fn trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}

/// An object's members in order, keys decoded and values as text, or `None`
/// if `text` is not an object.
pub(crate) fn members(text: &str) -> Option<Vec<(String, &str)>> {
    let data = text.as_bytes();
    let mut i = skip_space(data, 0);
    if data.get(i) != Some(&b'{') {
        return None;
    }
    let mut out = Vec::new();
    i = skip_space(data, i + 1);
    if data.get(i) == Some(&b'}') {
        return Some(out);
    }
    loop {
        i = skip_space(data, i);
        let key_end = skip_string(data, i)?;
        let key = decode_string(&text[i..key_end]);
        i = skip_space(data, key_end);
        if data.get(i) != Some(&b':') {
            return None;
        }
        let value_start = skip_space(data, i + 1);
        let value_end = skip_value(data, value_start)?;
        out.push((key, &text[value_start..value_end]));
        i = skip_space(data, value_end);
        match data.get(i) {
            Some(b',') => i += 1,
            Some(b'}') => return Some(out),
            _ => return None,
        }
    }
}

/// An array's elements in order, as text, or `None` if `text` is not an
/// array.
pub(crate) fn elements(text: &str) -> Option<Vec<&str>> {
    let data = text.as_bytes();
    let mut i = skip_space(data, 0);
    if data.get(i) != Some(&b'[') {
        return None;
    }
    let mut out = Vec::new();
    i = skip_space(data, i + 1);
    if data.get(i) == Some(&b']') {
        return Some(out);
    }
    loop {
        i = skip_space(data, i);
        let end = skip_value(data, i)?;
        out.push(&text[i..end]);
        i = skip_space(data, end);
        match data.get(i) {
            Some(b',') => i += 1,
            Some(b']') => return Some(out),
            _ => return None,
        }
    }
}

/// An object's members as a map, the last of a repeated key winning, as Go
/// decodes into a `map[string]json.RawMessage`.
pub(crate) fn member_map(text: &str) -> Option<BTreeMap<String, &str>> {
    Some(members(text)?.into_iter().collect())
}

/// Decodes a string literal, quotes included.
///
/// A lone surrogate becomes U+FFFD, as Go decodes it.
pub(crate) fn decode_string(literal: &str) -> String {
    let inner = &literal[1..literal.len() - 1];
    if !inner.contains('\\') {
        return inner.to_owned();
    }
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('b') => out.push('\u{08}'),
            Some('f') => out.push('\u{0c}'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let unit = hex4(&mut chars);
                if (0xD800..0xDC00).contains(&unit) {
                    let mut lookahead = chars.clone();
                    if lookahead.next() == Some('\\') && lookahead.next() == Some('u') {
                        let low = hex4(&mut lookahead);
                        if (0xDC00..0xE000).contains(&low) {
                            chars = lookahead;
                            let code = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                            continue;
                        }
                    }
                    out.push('\u{FFFD}');
                } else {
                    out.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

fn hex4(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u32 {
    let mut unit = 0;
    for _ in 0..4 {
        let digit = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0);
        unit = unit * 16 + digit;
    }
    unit
}

/// A JSON value as Go decodes it with `UseNumber`: a number is the text it
/// was written as, so `1`, `1.0` and `1e0` are three different numbers and a
/// frame count of any size keeps every digit.
#[derive(Debug, PartialEq)]
enum Semantic<'a> {
    Null,
    Bool(bool),
    Number(&'a str),
    String(String),
    Array(Vec<Semantic<'a>>),
    Object(BTreeMap<String, Semantic<'a>>),
}

fn semantic(text: &str) -> Semantic<'_> {
    let text = trim(text);
    match text.as_bytes().first() {
        Some(b'{') => Semantic::Object(
            members(text)
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key, semantic(value)))
                .collect(),
        ),
        Some(b'[') => Semantic::Array(
            elements(text)
                .unwrap_or_default()
                .into_iter()
                .map(semantic)
                .collect(),
        ),
        Some(b'"') => Semantic::String(decode_string(text)),
        Some(b't') => Semantic::Bool(true),
        Some(b'f') => Semantic::Bool(false),
        Some(b'n') => Semantic::Null,
        _ => Semantic::Number(text),
    }
}

/// Compares two valid JSON documents as Furnace does: object members in any
/// order, arrays in order, numbers by the text they were written as.
pub(crate) fn semantic_eq(a: &str, b: &str) -> bool {
    a == b || semantic(a) == semantic(b)
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Appends `text` with insignificant whitespace removed and `<`, `>`, `&`,
/// U+2028 and U+2029 escaped, as Go's encoder writes a `json.RawMessage`.
pub(crate) fn compact_into(out: &mut String, text: &str) {
    let mut in_string = false;
    let mut escaped = false;
    for c in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                ' ' | '\t' | '\r' | '\n' => continue,
                '"' => in_string = true,
                _ => {}
            }
        }
        push_html_safe(out, c, c);
    }
}

/// Pushes `c`, escaping it if Go's HTML-safe encoder would; `fallback` is
/// what to push otherwise.
fn push_html_safe(out: &mut String, c: char, fallback: char) {
    match c {
        '<' | '>' | '&' => {
            let byte = c as u8;
            out.push_str("\\u00");
            out.push(HEX[usize::from(byte >> 4)] as char);
            out.push(HEX[usize::from(byte & 0xF)] as char);
        }
        '\u{2028}' => out.push_str("\\u2028"),
        '\u{2029}' => out.push_str("\\u2029"),
        _ => out.push(fallback),
    }
}

/// `text` compacted, as [`compact_into`] writes it.
#[cfg(test)]
pub(crate) fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    compact_into(&mut out, text);
    out
}

/// Appends `value` as a JSON string literal, escaped as Go's encoder escapes
/// it: the HTML-sensitive characters too, and other control characters as
/// `\u00XX` in lower-case hex.
pub(crate) fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < '\u{20}' => {
                let byte = control as u8;
                out.push_str("\\u00");
                out.push(HEX[usize::from(byte >> 4)] as char);
                out.push(HEX[usize::from(byte & 0xF)] as char);
            }
            other => push_html_safe(out, other, other),
        }
    }
    out.push('"');
}

/// Re-indents compact JSON as Go's `json.MarshalIndent(v, "", "  ")` does.
pub(crate) fn indent(compact: &str) -> String {
    let mut out = String::with_capacity(compact.len() * 2);
    let mut need_indent = false;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        for _ in 0..depth {
            out.push_str("  ");
        }
    };
    for c in compact.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if need_indent && c != '}' && c != ']' {
            need_indent = false;
            depth += 1;
            newline(&mut out, depth);
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '{' | '[' => {
                need_indent = true;
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            '}' | ']' => {
                if need_indent {
                    need_indent = false;
                } else {
                    depth = depth.saturating_sub(1);
                    newline(&mut out, depth);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Folds a key for Go's case-insensitive match of an object member to a
/// struct field: ASCII letters to upper case, anything else through lower
/// case and back up, so that the Kelvin sign matches `k` and the long s
/// matches `s` as they do in Go.
pub(crate) fn fold_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_uppercase());
        } else {
            let lower = single(c.to_lowercase()).unwrap_or(c);
            out.push(single(lower.to_uppercase()).unwrap_or(lower));
        }
    }
    out
}

fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

/// Quotes a string as Go's `%q` does, for error messages.
pub(crate) fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{07}' => out.push_str("\\a"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0b}' => out.push_str("\\v"),
            control if control.is_control() => {
                let code = control as u32;
                if code < 0x80 {
                    out.push_str(&format!("\\x{code:02x}"));
                } else if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validity_matches_go() {
        assert!(is_valid(r#" {"a": [1, 2.5, -0, 1e3]} "#));
        assert!(!is_valid(""));
        assert!(!is_valid("{"));
        assert!(!is_valid(r#"{"a": NaN}"#));
        assert!(!is_valid(r#"{"a": Infinity}"#));
        assert!(!is_valid("{}\u{0}"));
        assert!(!is_valid("{} {}"));
    }

    #[test]
    fn numbers_compare_as_written() {
        assert!(semantic_eq(r#"{"a":1,"b":2}"#, r#"{ "b": 2, "a": 1 }"#));
        assert!(!semantic_eq("1", "1.0"));
        assert!(!semantic_eq("[1,2]", "[2,1]"));
        assert!(!semantic_eq("18446744073709551616", "18446744073709551617"));
        assert!(semantic_eq(r#""\u00e9""#, r#""é""#));
        assert!(semantic_eq(r#"{"a":1,"a":2}"#, r#"{"a":2}"#));
    }

    #[test]
    fn compacts_and_escapes_as_go_does() {
        assert_eq!(
            compact("{ \"a<\" : [ 1 , \"x & y\" ] ,\n \"b\": \"\u{2028}\" }"),
            r#"{"a\u003c":[1,"x \u0026 y"],"b":"\u2028"}"#
        );
        let mut out = String::new();
        push_string(&mut out, "a\"\\\u{1}\u{8}<é");
        assert_eq!(out, r#""a\"\\\u0001\b\u003cé""#);
    }

    #[test]
    fn indents_as_go_does() {
        assert_eq!(
            indent(r#"[{"a":[],"b":{},"c":[1,{"d":"[,:]"}]}]"#),
            "[\n  {\n    \"a\": [],\n    \"b\": {},\n    \"c\": [\n      1,\n      {\n        \"d\": \"[,:]\"\n      }\n    ]\n  }\n]"
        );
    }

    #[test]
    fn decodes_strings() {
        assert_eq!(decode_string(r#""\ud83c\udfac""#), "\u{1F3AC}");
        assert_eq!(decode_string(r#""\ud83c x""#), "\u{FFFD} x");
        assert_eq!(decode_string(r#""a\/b\n""#), "a/b\n");
    }

    #[test]
    fn folds_names() {
        assert_eq!(fold_name("tracKs"), "TRACKS");
        assert_eq!(fold_name("trac\u{212A}s"), "TRACKS");
        assert_eq!(fold_name("\u{17F}ync"), "SYNC");
    }
}

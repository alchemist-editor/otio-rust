//! Helpers upstream keeps with its `otioz` and `otiod` bundle code.
//!
//! Only [`file_from_url`] is here so far. Upstream's `url_utils` module calls
//! it to turn a media reference's `file://` URL back into a path, and it is
//! useful on its own for anything that has to find the file a reference
//! names.

use std::fmt;

/// A `%` escape in a URL did not start with a hexadecimal digit.
///
/// Upstream decodes each escape with `std::stoi(…, 16)`, which throws
/// `std::invalid_argument` for this; the message is that exception's, which
/// is what upstream's Python bindings raise as a `ValueError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidEscape;

impl fmt::Display for InvalidEscape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stoi")
    }
}

impl std::error::Error for InvalidEscape {}

/// Turns a `file://` URL into a filesystem path, as upstream's
/// `bundle::file_from_url` does.
///
/// - A bare path with no scheme comes back unchanged.
/// - A URL with any other scheme gives `None`.
/// - `file://C:/x` and `file:///C:/x` give `C:/x`; `file://host/share/x`
///   gives the UNC path `//host/share/x`; `file:///x` and
///   `file://localhost/x` give `/x`.
///
/// The path is percent-decoded and any query or fragment dropped; the host
/// is not decoded. Backslashes become forward slashes throughout.
///
/// The result is bytes rather than a string because a percent escape can
/// spell any byte at all, including ones that are not UTF-8.
///
/// # Errors
///
/// [`InvalidEscape`] when a `%` that has at least two characters after it
/// is not followed by a hexadecimal digit. A `%` too near the end to be an
/// escape is kept as it is, as upstream keeps it.
pub fn file_from_url(url: &str) -> Result<Option<Vec<u8>>, InvalidEscape> {
    const FILE_PREFIX: &str = "file://";
    let bytes = url.as_bytes();
    if bytes.len() < FILE_PREFIX.len()
        || !bytes[..FILE_PREFIX.len()].eq_ignore_ascii_case(FILE_PREFIX.as_bytes())
    {
        if url.contains("://") {
            return Ok(None);
        }
        return Ok(Some(url.as_bytes().to_vec()));
    }

    // Split what follows the scheme into the authority and the path.
    let rest = &bytes[FILE_PREFIX.len()..];
    let (netloc, path) = match rest.iter().position(|byte| *byte == b'/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, &[][..]),
    };

    // Drop the query, then the fragment.
    let mut path = path;
    if let Some(query) = path.iter().position(|byte| *byte == b'?') {
        path = &path[..query];
    }
    if let Some(fragment) = path.iter().position(|byte| *byte == b'#') {
        path = &path[..fragment];
    }

    let path = percent_decode(path)?;

    let is_drive =
        |text: &[u8]| text.len() == 2 && text[0].is_ascii_alphabetic() && text[1] == b':';

    let mut result = if is_drive(netloc) {
        // file://X:/path gives X:/path.
        [netloc, &path].concat()
    } else if path.len() >= 3 && path[0] == b'/' && is_drive(&path[1..3]) {
        // file://host/X:/path gives X:/path, dropping the host.
        path[1..].to_vec()
    } else if !netloc.is_empty() && !netloc.eq_ignore_ascii_case(b"localhost") {
        // file://host/path gives the UNC path //host/path.
        [&b"//"[..], netloc, &path].concat()
    } else {
        // file:///path and file://localhost/path give /path.
        path
    };

    for byte in &mut result {
        if *byte == b'\\' {
            *byte = b'/';
        }
    }
    Ok(Some(result))
}

/// Decodes `%XX` escapes, as upstream's `percent_decode` does.
///
/// An escape is only recognised with at least two characters after the `%`,
/// and the pair is read the way `std::stoi(pair, nullptr, 16)` reads it:
/// leading whitespace and a sign are allowed, and it stops at the first
/// character that is not a hex digit, so `%4g` decodes to byte 4 and `%-1`
/// to byte 255. Both characters are consumed whatever was read.
fn percent_decode(text: &[u8]) -> Result<Vec<u8>, InvalidEscape> {
    let mut result = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index] == b'%' && index + 2 < text.len() {
            let value = stoi_hex(&text[index + 1..index + 3])?;
            // `static_cast<char>` keeps the low byte, as two's complement.
            result.push(value.to_le_bytes()[0]);
            index += 3;
        } else {
            result.push(text[index]);
            index += 1;
        }
    }
    Ok(result)
}

/// Reads a hexadecimal number the way `std::stoi(text, nullptr, 16)` does.
fn stoi_hex(text: &[u8]) -> Result<i32, InvalidEscape> {
    // C's isspace: space, \t, \n, \v, \f and \r.
    let mut rest = text
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .map_or(&[][..], |start| &text[start..]);

    let negative = match rest.first() {
        Some(b'-') => {
            rest = &rest[1..];
            true
        }
        Some(b'+') => {
            rest = &rest[1..];
            false
        }
        _ => false,
    };

    // strtol skips a "0x" prefix only when a hex digit follows it; with two
    // characters to work from, that never happens, and "0x" reads as 0.
    let digits: Vec<u32> = rest
        .iter()
        .map_while(|byte| char::from(*byte).to_digit(16))
        .collect();
    if digits.is_empty() {
        return Err(InvalidEscape);
    }
    let magnitude = digits.iter().fold(0_i32, |value, digit| {
        // Two digits at most, so this cannot overflow.
        value * 16 + i32::try_from(*digit).unwrap_or(0)
    });
    Ok(if negative { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(url: &str) -> Option<String> {
        file_from_url(url)
            .unwrap()
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn a_bare_path_comes_back_unchanged() {
        assert_eq!(path("media/a b.mov").as_deref(), Some("media/a b.mov"));
        assert_eq!(path("/var/tmp/a.mov").as_deref(), Some("/var/tmp/a.mov"));
    }

    #[test]
    fn another_scheme_is_not_a_file() {
        assert_eq!(path("http://example.com/foo"), None);
        assert_eq!(path("s3://bucket/key"), None);
    }

    #[test]
    fn the_scheme_is_matched_without_regard_to_case() {
        assert_eq!(path("FILE:///tmp/a.mov").as_deref(), Some("/tmp/a.mov"));
    }

    #[test]
    fn hosts_and_drives_are_read_as_upstream_reads_them() {
        assert_eq!(
            path("file:///tmp/a%20b.mov").as_deref(),
            Some("/tmp/a b.mov")
        );
        assert_eq!(
            path("file://localhost/tmp/a.mov").as_deref(),
            Some("/tmp/a.mov")
        );
        assert_eq!(
            path("file://LocalHost/tmp/a.mov").as_deref(),
            Some("/tmp/a.mov")
        );
        assert_eq!(
            path("file://C:/media/a.mov").as_deref(),
            Some("C:/media/a.mov")
        );
        assert_eq!(
            path("file:///C:/media/a.mov").as_deref(),
            Some("C:/media/a.mov")
        );
        assert_eq!(
            path("file://server/share/a.mov").as_deref(),
            Some("//server/share/a.mov")
        );
        assert_eq!(path("file://server").as_deref(), Some("//server"));
        assert_eq!(path("file://C:").as_deref(), Some("C:"));
    }

    #[test]
    fn a_query_and_fragment_are_dropped_and_backslashes_turned_round() {
        assert_eq!(
            path("file:///tmp/a.mov?x=1#y").as_deref(),
            Some("/tmp/a.mov")
        );
        assert_eq!(path("file:///tmp/a.mov#y?x").as_deref(), Some("/tmp/a.mov"));
        assert_eq!(
            path("file:///C:%5Cmedia%5Ca.mov").as_deref(),
            Some("C:/media/a.mov")
        );
        assert_eq!(path("a\\b").as_deref(), Some("a\\b"));
    }

    #[test]
    fn escapes_are_read_as_stoi_reads_them() {
        // Too near the end to be an escape: kept as it is.
        assert_eq!(path("file:///a%2").as_deref(), Some("/a%2"));
        assert_eq!(path("file:///a%").as_deref(), Some("/a%"));
        // One hex digit is enough, and both characters are consumed.
        assert_eq!(path("file:///a%4g/").as_deref(), Some("/a\u{4}/"));
        assert_eq!(path("file:///a% 9/").as_deref(), Some("/a\t/"));
        assert_eq!(path("file:///a%+9/").as_deref(), Some("/a\t/"));
        assert_eq!(path("file:///a%0x/").as_deref(), Some("/a\0/"));
        assert_eq!(
            file_from_url("file:///a%-1/").unwrap(),
            Some(vec![b'/', b'a', 0xff, b'/'])
        );
        assert_eq!(file_from_url("file:///a%zz/"), Err(InvalidEscape));
        assert_eq!(InvalidEscape.to_string(), "stoi");
        // Only the path is decoded.
        assert_eq!(path("file://a%20b/c").as_deref(), Some("//a%20b/c"));
    }
}

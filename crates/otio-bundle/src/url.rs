//! Turning a media reference's URL into a path on disk.

/// Decodes `%XX` escapes, as upstream's `percent_decode` does.
///
/// Upstream parses the two characters after a `%` with `std::stoi`, which
/// throws on text that is not hexadecimal at all and stops at the first
/// non-hex character otherwise. A `%` followed by nothing hexadecimal is kept
/// as it is here rather than failing the whole bundle, and the escape is
/// otherwise decoded the same way.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let digits = &bytes[i + 1..i + 3];
            let hex = |b: u8| (b as char).to_digit(16);
            match (hex(digits[0]), hex(digits[1])) {
                (Some(high), Some(low)) => {
                    out.push((high * 16 + low) as u8);
                    i += 3;
                    continue;
                }
                (Some(high), None) => {
                    out.push(high as u8);
                    i += 3;
                    continue;
                }
                _ => {}
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `text` starts with `prefix`, ignoring the case of `text`.
fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()]
            .iter()
            .zip(prefix.as_bytes())
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn is_drive(bytes: &[u8]) -> bool {
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Returns the filesystem path a URL names, or `None` if it names none.
///
/// This is upstream's `bundle::file_from_url`. A `file://` URL becomes a
/// path: a Windows drive in the authority (`file://C:/...`) or at the start of
/// the path (`file://host/C:/...`) is kept, any other host but `localhost`
/// becomes a UNC path (`//host/...`), and percent escapes are decoded. A bare
/// path, with no scheme at all, is returned unchanged. Any other scheme
/// (`http://...`) names no file.
///
/// ```
/// use otio_bundle::file_from_url;
///
/// assert_eq!(file_from_url("file:///media/a%20b.mov").as_deref(), Some("/media/a b.mov"));
/// assert_eq!(file_from_url("relative/clip.mov").as_deref(), Some("relative/clip.mov"));
/// assert_eq!(file_from_url("https://example.com/clip.mov"), None);
/// ```
#[must_use]
pub fn file_from_url(url: &str) -> Option<String> {
    const FILE_PREFIX: &str = "file://";
    if !starts_with_ignoring_case(url, FILE_PREFIX) {
        if url.contains("://") {
            return None;
        }
        return Some(url.to_string());
    }

    // Split "file://" + authority + path.
    let rest = &url[FILE_PREFIX.len()..];
    let (netloc, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };

    // Strip the query, then the fragment.
    let path = path.split('?').next().unwrap_or_default();
    let path = path.split('#').next().unwrap_or_default();
    let path = percent_decode(path);

    let result = if is_drive(netloc.as_bytes()) {
        // file://X:/path -> X:/path
        format!("{netloc}{path}")
    } else if path.len() >= 3 && path.starts_with('/') && is_drive(&path.as_bytes()[1..3]) {
        // file://host/X:/path -> X:/path
        path[1..].to_string()
    } else if !netloc.is_empty() && !netloc.eq_ignore_ascii_case("localhost") {
        // file://host/path -> //host/path
        format!("//{netloc}{path}")
    } else {
        // file:///path or file://localhost/path -> /path
        path
    };
    Some(result.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::{file_from_url, percent_decode};

    #[test]
    fn upstreams_cases() {
        for (url, path) in [
            ("file://host/S%3a/path/file.ext", "S:/path/file.ext"),
            ("file://S:/path/file.ext", "S:/path/file.ext"),
            (
                "file://unc/path/sub%20dir/file.ext",
                "//unc/path/sub dir/file.ext",
            ),
            (
                "file://unc/path/sub dir/file.ext",
                "//unc/path/sub dir/file.ext",
            ),
            (
                "file://localhost/path/sub dir/file.ext",
                "/path/sub dir/file.ext",
            ),
            ("file:///path/sub%20dir/file.ext", "/path/sub dir/file.ext"),
            ("file:///path/sub dir/file.ext", "/path/sub dir/file.ext"),
        ] {
            assert_eq!(file_from_url(url).as_deref(), Some(path), "{url}");
        }
    }

    #[test]
    fn the_scheme_is_matched_without_regard_to_case() {
        assert_eq!(file_from_url("FILE:///a/b").as_deref(), Some("/a/b"));
    }

    #[test]
    fn queries_and_fragments_are_dropped() {
        assert_eq!(
            file_from_url("file:///a/b.mov?x=1#t").as_deref(),
            Some("/a/b.mov")
        );
    }

    #[test]
    fn a_stray_percent_is_kept() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
        assert_eq!(percent_decode("%41%42"), "AB");
    }
}

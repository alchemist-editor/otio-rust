//! Guessing an Avid project format from the clips' image sizes.

/// The format stated when nothing in the document says what the images are.
pub const DEFAULT_VIDEO_FORMAT: &str = "1080";

/// Returns the Avid project format for an image size.
///
/// Avid names a handful of formats by their height, and calls everything else
/// `CUSTOM`. The 2K DCI case is the one that needs the width too: it is 1080
/// tall but wider than HD, so it is not the `1080` format.
#[must_use]
pub fn video_format_for(width: u64, height: u64) -> &'static str {
    match height {
        1080 if width > 1920 => "CUSTOM",
        1080 => "1080",
        720 => "720",
        576 => "PAL",
        486 => "NTSC",
        _ => "CUSTOM",
    }
}

/// Reads a width and height out of an `Image Size` value.
///
/// The column is free-form, so `1920x1080`, `1920 x 1080` and `1280x 720` all
/// appear in real files; the first `<digits> x <digits>` anywhere in the value
/// wins. Returns `None` if there is no such pair.
#[must_use]
pub fn parse_image_size(value: &str) -> Option<(u64, u64)> {
    let bytes = value.as_bytes();

    for start in 0..bytes.len() {
        // Match from the first digit of a run, so that a run of digits is
        // taken whole rather than from its second character on.
        if !bytes[start].is_ascii_digit() || (start > 0 && bytes[start - 1].is_ascii_digit()) {
            continue;
        }
        let mut at = start;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        let width_end = at;

        while bytes.get(at) == Some(&b' ') || bytes.get(at) == Some(&b'\t') {
            at += 1;
        }
        if !matches!(bytes.get(at), Some(&b'x' | &b'X')) {
            continue;
        }
        at += 1;
        while bytes.get(at) == Some(&b' ') || bytes.get(at) == Some(&b'\t') {
            at += 1;
        }

        let height_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == height_start {
            continue;
        }

        let width = value[start..width_end].parse().ok()?;
        let height = value[height_start..at].parse().ok()?;
        return Some((width, height));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{parse_image_size, video_format_for};

    #[test]
    fn reads_the_shapes_real_files_use() {
        assert_eq!(parse_image_size("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_image_size("1920 x 1080"), Some((1920, 1080)));
        assert_eq!(parse_image_size("1280x 720"), Some((1280, 720)));
        assert_eq!(parse_image_size("4096 x 2304"), Some((4096, 2304)));
        assert_eq!(parse_image_size(""), None);
        assert_eq!(parse_image_size("1080p"), None);
    }

    #[test]
    fn names_the_avid_formats() {
        assert_eq!(video_format_for(720, 486), "NTSC");
        assert_eq!(video_format_for(720, 576), "PAL");
        assert_eq!(video_format_for(1280, 720), "720");
        assert_eq!(video_format_for(1920, 1080), "1080");
        assert_eq!(video_format_for(4096, 2304), "CUSTOM");
    }

    #[test]
    fn dci_2k_is_not_the_1080_format() {
        // Same height as HD, wider frame, so Avid does not call it 1080.
        assert_eq!(video_format_for(2048, 1080), "CUSTOM");
    }
}

//! Image sequences, which an EDL names by their frame range.
//!
//! A comment such as
//!
//! ```text
//! * FROM CLIP: /media/path/my_shot.[1025-1060].ext
//! ```
//!
//! names a numbered sequence of files rather than one file. The bracketed
//! range is not part of any filename: it stands for the numbers in between.

use opentime::{RationalTime, TimeRange};
use otio_core::schema::ImageSequenceReference;

use crate::path::{basename, dirname};

/// What a bracketed URL says about a sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The directory holding the frames.
    pub directory: String,
    /// The part of each filename before the frame number.
    pub prefix: String,
    /// The part of each filename after the frame number.
    pub suffix: String,
    /// The first frame's number.
    pub start: i64,
    /// The last frame's number.
    pub end: i64,
    /// How many digits the frame numbers are written with.
    pub padding: i64,
}

/// Reads a bracketed URL, or returns `None` if it names a single file.
///
/// The shape is a `.`, a bracketed range of two frame numbers, a `.`, and an
/// extension of word characters at the end. A URL that merely holds a number,
/// such as `my_file.1025.ext`, names one file and is not a sequence; so is a
/// bracketed single number, which states no range.
#[must_use]
pub fn parse_url(url: &str) -> Option<Parsed> {
    let last_dot = url.rfind('.')?;
    let extension = &url[last_dot + 1..];
    if extension.is_empty()
        || !extension
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
    {
        return None;
    }

    let head = &url[..last_dot];
    if !head.ends_with(']') {
        return None;
    }
    let open = head.rfind('[')?;
    if open == 0 || !head[..open].ends_with('.') {
        return None;
    }

    let (start, end) = head[open + 1..head.len() - 1].split_once('-')?;
    if start.is_empty() || end.is_empty() {
        return None;
    }
    let start_frame: i64 = start.parse().ok()?;
    let end_frame: i64 = end.parse().ok()?;

    let range = &head[open..];
    let name = basename(url);
    let (prefix, suffix) = name.split_once(range)?;

    Some(Parsed {
        directory: dirname(url).to_string(),
        prefix: prefix.to_string(),
        suffix: suffix.to_string(),
        start: start_frame,
        end: end_frame,
        padding: i64::try_from(start.len()).unwrap_or(0),
    })
}

/// Returns the sequence's URL with `symbol` standing in for the frame number.
///
/// A directory that does not end in a separator gains one, so a reference
/// built by reading a URL produces the same URL again.
#[must_use]
pub fn abstract_url(reference: &ImageSequenceReference, symbol: &str) -> String {
    let base = &reference.target_url_base;
    let separator = if base.is_empty() || base.ends_with('/') {
        ""
    } else {
        "/"
    };
    format!(
        "{base}{separator}{}{symbol}{}",
        reference.name_prefix, reference.name_suffix
    )
}

/// Returns the number of the frame showing at a time.
#[must_use]
pub fn frame_for_time(reference: &ImageSequenceReference, time: RationalTime) -> i64 {
    let Some(available) = reference.media.available_range else {
        return reference.start_frame;
    };
    let rate = reference.rate;
    let offset = time.value_rescaled_to(rate) - available.start_time().value_rescaled_to(rate);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a frame offset beyond i64 is past any real sequence"
    )]
    let offset = offset.floor() as i64;
    reference.start_frame + offset * reference.frame_step
}

/// Returns the first and last frame numbers a span of time covers.
///
/// The last is inclusive: a span of one frame covers one number, not two.
#[must_use]
pub fn frame_range_for_time_range(
    reference: &ImageSequenceReference,
    range: TimeRange,
) -> (i64, i64) {
    (
        frame_for_time(reference, range.start_time()),
        frame_for_time(reference, range.end_time_inclusive()),
    )
}

/// Returns the URL of a sequence covering `range`, with the range spelled out.
#[must_use]
pub fn url_for_range(reference: &ImageSequenceReference, range: TimeRange) -> String {
    let (start, end) = frame_range_for_time_range(reference, range);
    abstract_url(reference, &format!("[{start}-{end}]"))
}

#[cfg(test)]
mod tests {
    use super::{Parsed, parse_url};

    #[test]
    fn reads_a_bracketed_range() {
        assert_eq!(
            parse_url("/media/path/my_image_sequence.[1025-1060].ext"),
            Some(Parsed {
                directory: "/media/path".to_string(),
                prefix: "my_image_sequence.".to_string(),
                suffix: ".ext".to_string(),
                start: 1025,
                end: 1060,
                padding: 4,
            })
        );
    }

    #[test]
    fn a_single_numbered_file_is_not_a_sequence() {
        assert_eq!(parse_url("/media/path/my_image_file.1025.ext"), None);
        // One number in brackets states no range.
        assert_eq!(parse_url("/media/path/my_image_file.[1025].ext"), None);
    }
}

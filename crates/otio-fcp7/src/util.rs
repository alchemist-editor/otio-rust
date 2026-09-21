//! The inheritance context, and the small conversions both directions need.

use opentime::RationalTime;
use otio_xml::Element;

use otio_adapter::Result;

use crate::err;

/// A stack of elements a value may be inherited from.
///
/// FCP XML lets an element leave out a value and take its parent's instead: a
/// `clipitem` with no `rate` of its own runs at the rate of the `track` it
/// sits on, or the `sequence` above that. A lookup walks the stack from the
/// top down and takes the first hit.
///
/// The stack is immutable. Pushing returns a new context, so a branch of the
/// document cannot disturb the context its siblings are read under.
///
/// Dereferencing by `id` happens before anything reaches here: an element in
/// the stack is always the one the file spelled out in full, never the stub
/// that refers to it.
#[derive(Debug, Clone, Default)]
pub struct Context<'a> {
    elements: Vec<&'a Element>,
}

impl<'a> Context<'a> {
    /// An empty context, inheriting nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    /// Returns a new context with `element` on top.
    ///
    /// # Errors
    ///
    /// Returns an error if the element is already in the stack, which would
    /// mean the document contains itself.
    pub fn pushing(&self, element: &'a Element) -> Result<Self> {
        if self
            .elements
            .iter()
            .any(|existing| std::ptr::eq(*existing, element))
        {
            return Err(err::circular_inheritance(element));
        }

        let mut elements = self.elements.clone();
        elements.push(element);
        Ok(Self { elements })
    }

    /// Finds the nearest element reachable by following `path` down from
    /// somewhere in the stack.
    #[must_use]
    pub fn find_path(&self, path: &[&str]) -> Option<&'a Element> {
        self.elements
            .iter()
            .rev()
            .find_map(|element| element.find_path(path))
    }

    /// Returns the rate that applies here, if one does.
    ///
    /// `timebase` and `ntsc` are looked up independently, as upstream does, so
    /// a sequence's `ntsc` flag still applies to a track that overrides only
    /// the timebase.
    ///
    /// # Errors
    ///
    /// Returns an error if the timebase is not a number.
    pub fn rate(&self) -> Result<Option<f64>> {
        let Some(timebase) = self.find_path(&["rate", "timebase"]) else {
            return Ok(None);
        };
        let ntsc = self.find_path(&["rate", "ntsc"]).map(bool_value);
        Ok(Some(otio_rate(parse_f64("timebase", timebase)?, ntsc)))
    }

    /// Returns the rate that applies here, failing if none does.
    ///
    /// # Errors
    ///
    /// Returns an error if nothing in the stack carries one.
    pub fn require_rate(&self, element: &Element) -> Result<f64> {
        self.rate()?.ok_or_else(|| err::no_rate(element))
    }
}

/// Converts an FCP timebase and NTSC flag into a frame rate.
///
/// A timebase of 30 with the NTSC flag set is 30000/1001, the rate the
/// industry writes as 29.97.
#[must_use]
pub fn otio_rate(timebase: f64, ntsc: Option<bool>) -> f64 {
    if ntsc == Some(true) {
        timebase * 1000.0 / 1001.0
    } else {
        timebase
    }
}

/// Reads an element's text as a boolean.
///
/// FCP writes `TRUE` and `FALSE`; anything else is false, as upstream has it.
#[must_use]
pub fn bool_value(element: &Element) -> bool {
    element.text_or_empty().eq_ignore_ascii_case("true")
}

/// Names an element for an error message, the way upstream's
/// `_element_identification_string` does.
#[must_use]
pub fn identify(element: &Element) -> String {
    match element.attributes.get("id") {
        Some(id) => format!("tag: {} id: {}", element.tag, id),
        None => format!("tag: {}", element.tag),
    }
}

/// Returns the text of an element's `name` child, or the empty string.
#[must_use]
pub fn name_from_element(element: &Element) -> String {
    element
        .find("name")
        .map(|name| name.text_or_empty().to_string())
        .unwrap_or_default()
}

/// Parses an element's text as a float.
///
/// # Errors
///
/// Returns an error if it is not one.
pub fn parse_f64(tag: &str, element: &Element) -> Result<f64> {
    element
        .text_or_empty()
        .trim()
        .parse()
        .map_err(|_| err::not_a_number(tag, element.text_or_empty()))
}

/// Parses an element's text as a whole number of frames.
///
/// FCP writes these as integers, and upstream reads them with `int()`, so a
/// fractional value is a malformed file rather than something to round.
///
/// # Errors
///
/// Returns an error if it is not one.
pub fn parse_i64(tag: &str, element: &Element) -> Result<i64> {
    element
        .text_or_empty()
        .trim()
        .parse()
        .map_err(|_| err::not_a_number(tag, element.text_or_empty()))
}

/// Finds a required direct child.
///
/// # Errors
///
/// Returns an error if it is not there.
pub fn require_child<'a>(parent: &'a Element, tag: &str) -> Result<&'a Element> {
    parent.find(tag).ok_or_else(|| err::missing(tag, parent))
}

/// Returns the path component of a URL.
///
/// This is Python's `urlparse(url).path`, which is what upstream uses to get a
/// filename out of a `pathurl`. A string with no scheme is all path, so a
/// plain `/var/tmp/take1.mov` comes back unchanged.
#[must_use]
pub fn url_path(url: &str) -> &str {
    let mut rest = url;

    // A scheme is a letter followed by letters, digits, `+`, `-` or `.`, then
    // a colon. Anything else before a colon — a Windows drive letter, say — is
    // part of the path.
    if let Some(colon) = rest.find(':') {
        let scheme = &rest[..colon];
        if !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            rest = &rest[colon + 1..];
        }
    }

    if let Some(after_slashes) = rest.strip_prefix("//") {
        let end = after_slashes
            .find(['/', '?', '#'])
            .unwrap_or(after_slashes.len());
        rest = &after_slashes[end..];
    }

    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    &rest[..end]
}

/// Returns the last path component of a URL's path, as `os.path.basename`
/// would.
#[must_use]
pub fn url_basename(url: &str) -> &str {
    let path = url_path(url);
    match path.rfind('/') {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

/// Formats a time as a whole number of frames, the way upstream's `'%.0f'`
/// does.
///
/// Python rounds half to even here, and the values being written are whole
/// frames in practice, so the tie-breaking rule only shows up on malformed
/// input. Matching it costs nothing.
#[must_use]
pub fn frame_text(time: RationalTime) -> String {
    format_frames(time.value())
}

/// Formats a frame count the way Python's `f"{value:.0f}"` does.
#[must_use]
pub fn format_frames(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }

    // Round half to even, which is what Python's format spec does and what
    // Rust's `round` does not.
    let rounded = {
        let nearest = value.round();
        if (value - value.trunc()).abs() == 0.5 && nearest % 2.0 != 0.0 {
            nearest - value.signum()
        } else {
            nearest
        }
    };

    // `-0` is what the naive formatting of a small negative value gives, and
    // it is not what Python writes for zero.
    if rounded == 0.0 {
        return "0".to_string();
    }
    format!("{rounded:.0}")
}

#[cfg(test)]
mod tests {
    use opentime::RationalTime;
    use otio_xml::parse;

    use super::{Context, bool_value, format_frames, otio_rate, url_basename, url_path};

    #[test]
    fn ntsc_rates_match_the_industry_values() {
        assert_eq!(otio_rate(24.0, Some(false)), 24.0);
        assert_eq!(otio_rate(24.0, None), 24.0);
        assert!((otio_rate(24.0, Some(true)) - 23.976).abs() < 0.001);
        assert!((otio_rate(30.0, Some(true)) - 29.97).abs() < 0.001);
    }

    #[test]
    fn a_rate_is_inherited_from_the_nearest_element_that_has_one() {
        let tree = parse(
            "<sequence><rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>\
             <track><clipitem><rate><timebase>24</timebase></rate></clipitem></track>\
             </sequence>",
        )
        .expect("well-formed");

        let track = tree.find("track").expect("track is present");
        let clip = track.find("clipitem").expect("clipitem is present");

        let sequence_context = Context::new().pushing(&tree).expect("first push");
        assert!((sequence_context.rate().unwrap().unwrap() - 29.97).abs() < 0.001);

        // The track has no rate of its own, so it keeps the sequence's.
        let track_context = sequence_context.pushing(track).expect("second push");
        assert!((track_context.rate().unwrap().unwrap() - 29.97).abs() < 0.001);

        // The clip overrides only the timebase, so it keeps the NTSC flag
        // from above it: 24 * 1000 / 1001, not a flat 24.
        let clip_context = track_context.pushing(clip).expect("third push");
        assert!((clip_context.rate().unwrap().unwrap() - 23.976).abs() < 0.001);
    }

    #[test]
    fn pushing_an_element_already_in_the_stack_is_refused() {
        let tree = parse("<a/>").expect("well-formed");
        let context = Context::new().pushing(&tree).expect("first push");
        assert!(context.pushing(&tree).is_err());
    }

    #[test]
    fn bool_values_follow_fcps_spelling() {
        let tree =
            parse("<p><t>TRUE</t><f>FALSE</f><l>true</l><o>yes</o></p>").expect("well-formed");
        assert!(bool_value(tree.find("t").unwrap()));
        assert!(!bool_value(tree.find("f").unwrap()));
        assert!(bool_value(tree.find("l").unwrap()));
        assert!(!bool_value(tree.find("o").unwrap()));
    }

    #[test]
    fn url_paths_drop_the_scheme_and_host() {
        assert_eq!(url_path("file:///var/tmp/take1.mov"), "/var/tmp/take1.mov");
        assert_eq!(url_path("file://localhost/v/take1.mov"), "/v/take1.mov");
        assert_eq!(url_path("/var/tmp/take1.mov"), "/var/tmp/take1.mov");
        assert_eq!(url_path("take1.mov"), "take1.mov");
        assert_eq!(url_path("http://host/a/b?q=1#f"), "/a/b");
        assert_eq!(url_basename("file:///var/tmp/take1.mov"), "take1.mov");
    }

    #[test]
    fn frames_are_formatted_as_python_does() {
        assert_eq!(format_frames(0.0), "0");
        assert_eq!(format_frames(-0.0), "0");
        assert_eq!(format_frames(536.0), "536");
        assert_eq!(format_frames(-1.0), "-1");
        // Python's `.0f` rounds half to even.
        assert_eq!(format_frames(0.5), "0");
        assert_eq!(format_frames(1.5), "2");
        assert_eq!(format_frames(2.5), "2");
        assert_eq!(super::frame_text(RationalTime::new(24.0, 24.0)), "24");
    }
}

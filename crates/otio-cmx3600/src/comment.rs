//! The comment lines that follow an edit statement.
//!
//! A CMX 3600 event is one or two fixed-shape lines of timecode followed by
//! any number of free-form comments. Everything the format cannot say in the
//! edit statement itself — which file a reel refers to, what the clip is
//! called, a colour decision, a marker, a speed change — is said here, and
//! each system that writes EDLs says it slightly differently.
//!
//! A comment this adapter recognizes becomes structure on the clip. One it
//! does not is kept verbatim on the clip's metadata, so writing the timeline
//! back out reproduces it.

/// The comments of one event, sorted into what they mean.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comments {
    /// `FROM CLIP NAME`: what to call the clip.
    pub clip_name: Option<String>,
    /// `TO CLIP NAME`: what to call the clip on the far side of a dissolve.
    pub dest_clip_name: Option<String>,
    /// `FROM CLIP`, `FROM FILE` or `OTIO REFERENCE`: where the media is.
    pub media_reference: Option<String>,
    /// `LOC`: markers, in the order they appeared.
    pub locators: Vec<String>,
    /// `ASC_SOP`: slope, offset and power.
    pub asc_sop: Option<String>,
    /// `ASC_SAT`: saturation.
    pub asc_sat: Option<String>,
    /// `M2`: a speed change.
    pub motion_effect: Option<String>,
    /// `* FREEZE FRAME`: the event holds one frame.
    pub freeze_frame: Option<String>,
    /// Everything else, with leading asterisks stripped.
    pub unhandled: Vec<String>,
}

/// One comment this adapter knows how to read.
///
/// The order is significant and matches upstream's: `FROM CLIP NAME` has to
/// be tried before `FROM CLIP`, or every name would read as a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tag {
    ClipName,
    DestClipName,
    FromClip,
    FromFile,
    Locator,
    AscSop,
    AscSat,
    Motion,
    FreezeFrame,
    OtioReference,
}

/// The tags, in the order they are tried.
const TAGS: [Tag; 10] = [
    Tag::ClipName,
    Tag::DestClipName,
    Tag::FromClip,
    Tag::FromFile,
    Tag::Locator,
    Tag::AscSop,
    Tag::AscSat,
    Tag::Motion,
    Tag::FreezeFrame,
    Tag::OtioReference,
];

impl Tag {
    /// Returns the literal this tag begins with, if it is a fixed string.
    const fn literal(self) -> Option<&'static str> {
        match self {
            Self::ClipName => Some("FROM CLIP NAME"),
            Self::DestClipName => Some("TO CLIP NAME"),
            Self::FromClip => Some("FROM CLIP"),
            Self::FromFile => Some("FROM FILE"),
            Self::Locator => Some("LOC"),
            Self::AscSop => Some("ASC_SOP"),
            Self::AscSat => Some("ASC_SAT"),
            Self::Motion => Some("M2"),
            Self::FreezeFrame => Some("* FREEZE FRAME"),
            Self::OtioReference => None,
        }
    }

    /// Consumes this tag from the front of `rest`, returning what follows.
    fn take(self, rest: &str) -> Option<&str> {
        if let Some(literal) = self.literal() {
            return rest.strip_prefix(literal);
        }

        // `OTIO REFERENCE FROM` and `OTIO REFERENCE TO`, and anything else
        // alphabetic, which is upstream's `[a-zA-Z]+`.
        let after = rest.strip_prefix("* OTIO REFERENCE ")?;
        let direction = after
            .find(|character: char| !character.is_ascii_alphabetic())
            .unwrap_or(after.len());
        (direction > 0).then(|| &after[direction..])
    }
}

/// Returns the body of `comment` if it carries `tag`.
///
/// The shape is upstream's: an optional leading `*`, whitespace, the tag, an
/// optional `:`, whitespace, and the rest of the line. The leading `*` is
/// optional *and* part of two of the tags, so it is tried both ways — which
/// is how `* OTIO REFERENCE FROM:` matches a tag that starts with its own
/// asterisk.
fn body_of(comment: &str, tag: Tag) -> Option<&str> {
    for consume_star in [true, false] {
        let mut rest = comment;
        if consume_star {
            let Some(after) = rest.strip_prefix('*') else {
                continue;
            };
            rest = after;
        }
        rest = rest.trim_start();

        if let Some(after) = tag.take(rest) {
            let after = after.strip_prefix(':').unwrap_or(after);
            return Some(after.trim());
        }
    }

    None
}

impl Comments {
    /// Sorts an event's comment lines.
    #[must_use]
    pub fn parse(comments: &[&str]) -> Self {
        let mut result = Self::default();

        'next: for comment in comments {
            for tag in TAGS {
                let Some(body) = body_of(comment, tag) else {
                    continue;
                };
                let body = body.to_string();
                match tag {
                    Tag::ClipName => result.clip_name = Some(body),
                    Tag::DestClipName => result.dest_clip_name = Some(body),
                    // `FROM CLIP`, `FROM FILE` and `OTIO REFERENCE` all name
                    // the media; which one a file uses is a matter of which
                    // system wrote it.
                    Tag::FromClip | Tag::FromFile | Tag::OtioReference => {
                        result.media_reference = Some(body);
                    }
                    // Several markers per event, so these accumulate.
                    Tag::Locator => result.locators.push(body),
                    Tag::AscSop => result.asc_sop = Some(body),
                    Tag::AscSat => result.asc_sat = Some(body),
                    Tag::Motion => result.motion_effect = Some(body),
                    Tag::FreezeFrame => result.freeze_frame = Some(body),
                }
                continue 'next;
            }

            let stripped = comment.trim_start_matches('*').trim();
            if !stripped.is_empty() {
                result.unhandled.push(stripped.to_string());
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::Comments;

    #[test]
    fn reads_the_comments_avid_writes() {
        let comments = Comments::parse(&[
            "* FROM CLIP NAME:  take_1",
            "* FROM CLIP: S:/path/to/take_1.exr",
        ]);
        assert_eq!(comments.clip_name.as_deref(), Some("take_1"));
        assert_eq!(
            comments.media_reference.as_deref(),
            Some("S:/path/to/take_1.exr")
        );
    }

    #[test]
    fn a_name_is_not_mistaken_for_a_path() {
        // `FROM CLIP NAME` has to be tried before `FROM CLIP`, or the name
        // would be read as the media it refers to.
        let comments = Comments::parse(&["* FROM CLIP NAME:  take_1"]);
        assert_eq!(comments.clip_name.as_deref(), Some("take_1"));
        assert_eq!(comments.media_reference, None);
    }

    #[test]
    fn a_tag_carrying_its_own_asterisk_still_matches() {
        // `* OTIO REFERENCE FROM` begins with an asterisk that the optional
        // leading asterisk would otherwise swallow.
        let comments = Comments::parse(&["* OTIO REFERENCE FROM: /var/tmp/test.exr"]);
        assert_eq!(
            comments.media_reference.as_deref(),
            Some("/var/tmp/test.exr")
        );

        let comments = Comments::parse(&["* * FREEZE FRAME"]);
        assert_eq!(comments.freeze_frame.as_deref(), Some(""));
    }

    #[test]
    fn a_colon_and_the_spacing_around_it_are_optional() {
        // Files in the wild write all of these.
        for line in ["*ASC_SAT 0.9", "* ASC_SAT: 0.9", "*  ASC_SAT:0.9"] {
            assert_eq!(
                Comments::parse(&[line]).asc_sat.as_deref(),
                Some("0.9"),
                "for {line}"
            );
        }
    }

    #[test]
    fn markers_accumulate_and_the_rest_is_kept_verbatim() {
        let comments = Comments::parse(&[
            "* LOC: 01:00:01:14 RED     ANIM FIX NEEDED",
            "* LOC: 01:00:02:14 BLUE",
            "* SOURCE FILE: ZZ100_501.LAY3.01",
        ]);
        assert_eq!(comments.locators.len(), 2);
        assert_eq!(comments.unhandled, ["SOURCE FILE: ZZ100_501.LAY3.01"]);
    }
}

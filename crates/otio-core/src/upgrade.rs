//! Reading documents written by older releases.
//!
//! OTIO tags every object with a schema version, and a field can move, change
//! shape, or gain a replacement between versions. Upstream handles this with
//! upgrade functions registered per schema version; the equivalents live here,
//! applied by the reader when it meets an older object.
//!
//! Without these, an old file still parses — but quietly loses whatever moved.
//! That is worse than failing.
//!
//! The reverse direction, writing a document targeted at an older release, is
//! not implemented yet.

use crate::value::Color;

/// The key upstream files a pre-`Clip.2` media reference under.
pub const DEFAULT_MEDIA_KEY: &str = "DEFAULT_MEDIA";

/// The named colours `Marker.2` allowed, with the values `Marker.3` gives them.
///
/// Taken from upstream's `Color` constants.
const NAMED_COLORS: [(&str, f64, f64, f64, f64, &str); 12] = [
    ("PINK", 1.0, 0.0, 1.0, 1.0, "Pink"),
    ("RED", 1.0, 0.0, 0.0, 1.0, "Red"),
    ("ORANGE", 1.0, 0.5, 0.0, 1.0, "Orange"),
    ("YELLOW", 1.0, 1.0, 0.0, 1.0, "Yellow"),
    ("GREEN", 0.0, 1.0, 0.0, 1.0, "Green"),
    ("CYAN", 0.0, 1.0, 1.0, 1.0, "Cyan"),
    ("BLUE", 0.0, 0.0, 1.0, 1.0, "Blue"),
    ("PURPLE", 0.5, 0.0, 0.5, 1.0, "Purple"),
    ("MAGENTA", 1.0, 0.0, 1.0, 1.0, "Magenta"),
    ("BLACK", 0.0, 0.0, 0.0, 1.0, "Black"),
    ("WHITE", 1.0, 1.0, 1.0, 1.0, "White"),
    ("TRANSPARENT", 0.0, 0.0, 0.0, 0.0, "Transparent"),
];

/// Upgrades a `Marker.2` colour name to a `Marker.3` colour.
///
/// `Marker.2` colour names were case-insensitive. A name that is not one of
/// the known colours becomes white but keeps the name it had, so nothing the
/// file said is thrown away.
#[must_use]
pub fn color_from_legacy_name(name: &str) -> Color {
    let upper = name.to_uppercase();
    NAMED_COLORS
        .iter()
        .find(|(key, ..)| *key == upper)
        .map_or_else(
            || Color::new(1.0, 1.0, 1.0, 1.0, name.to_string()),
            |&(_, r, g, b, a, canonical)| Color::new(r, g, b, a, canonical.to_string()),
        )
}

#[cfg(test)]
mod tests {
    use super::color_from_legacy_name;

    #[test]
    fn known_names_take_their_canonical_values() {
        let red = color_from_legacy_name("RED");
        assert_eq!((red.r, red.g, red.b, red.a), (1.0, 0.0, 0.0, 1.0));
        assert_eq!(red.name, "Red");
    }

    #[test]
    fn names_are_matched_without_regard_to_case() {
        assert_eq!(
            color_from_legacy_name("green"),
            color_from_legacy_name("GREEN")
        );
        assert_eq!(color_from_legacy_name("Green").name, "Green");
    }

    #[test]
    fn an_unknown_name_keeps_the_name_it_had() {
        let custom = color_from_legacy_name("Studio Teal");
        assert_eq!(custom.name, "Studio Teal");
        assert_eq!(
            (custom.r, custom.g, custom.b, custom.a),
            (1.0, 1.0, 1.0, 1.0)
        );
    }
}

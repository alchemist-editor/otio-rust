//! Moving objects between the schema versions of different releases.
//!
//! OTIO tags every object with a schema version, and a field can move, change
//! shape, or gain a replacement between versions. Upstream handles this with
//! upgrade and downgrade functions registered per schema version; its
//! built-in ones are here, and [`crate::registry`] holds them alongside any a
//! program registers. The reader runs the upgrades when it meets an older
//! object; the writer runs the downgrades when asked to write for an older
//! release.
//!
//! Without the upgrades, an old file still parses — but quietly loses
//! whatever moved. That is worse than failing.
//!
//! Each function works on an object's fields, in the self-contained form
//! [`crate::registry`] describes: nested objects are dictionaries carrying
//! their `OTIO_SCHEMA`.

use crate::error::Result;
use crate::value::{Any, AnyDictionary, Color};

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

/// Whether a field holds an object, in the dictionary form functions see.
fn is_object(value: &Any) -> bool {
    matches!(value, Any::Dictionary(entries) if entries.contains_key("OTIO_SCHEMA"))
}

/// `Marker.1` to `Marker.2`: `range` became `marked_range`.
///
/// Upstream runs every function keyed from the file's own version up, so a
/// `Marker.2` file goes through this one too. Upstream's copies `range`
/// whether or not it is there, which leaves such a file's `marked_range`
/// empty, and its reader then refuses the marker as a type mismatch: at the
/// pinned commit, upstream cannot read a `Marker.2` file at all. This one
/// only moves a `range` that is present, so those files read. That is a
/// deliberate divergence.
///
/// # Errors
///
/// None; the signature is the one every version function has.
pub fn marker_1_to_2(fields: &mut AnyDictionary) -> Result<()> {
    if let Some(range) = fields.remove("range") {
        fields.insert("marked_range".to_string(), range);
    }
    Ok(())
}

/// `Marker.2` to `Marker.3`: a colour name became a colour.
///
/// See [`color_from_legacy_name`].
///
/// # Errors
///
/// None; the signature is the one every version function has.
pub fn marker_2_to_3(fields: &mut AnyDictionary) -> Result<()> {
    if let Some(Any::String(name)) = fields.get("color") {
        let color = color_from_legacy_name(name);
        fields.insert("color".to_string(), Any::Color(color));
    }
    Ok(())
}

/// `Clip.1` to `Clip.2`: the single `media_reference` became the
/// [`DEFAULT_MEDIA_KEY`] entry of `media_references`.
///
/// A `Clip.1` with no media reference used to default to a
/// `MissingReference`, so one is supplied here to keep that behaviour.
///
/// # Errors
///
/// None; the signature is the one every version function has.
pub fn clip_1_to_2(fields: &mut AnyDictionary) -> Result<()> {
    let mut reference = fields.remove("media_reference").unwrap_or(Any::Null);
    if !is_object(&reference) {
        let mut missing = AnyDictionary::new();
        missing.insert(
            "OTIO_SCHEMA".to_string(),
            Any::String("MissingReference.1".to_string()),
        );
        reference = Any::Dictionary(missing);
    }
    let mut references = AnyDictionary::new();
    references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);
    fields.insert("media_references".to_string(), Any::Dictionary(references));
    fields.insert(
        "active_media_reference_key".to_string(),
        Any::String(DEFAULT_MEDIA_KEY.to_string()),
    );
    Ok(())
}

/// The colour names `Marker.2` knew.
const LEGACY_COLOR_NAMES: [&str; 12] = [
    "RED",
    "GREEN",
    "BLUE",
    "YELLOW",
    "CYAN",
    "MAGENTA",
    "PINK",
    "ORANGE",
    "PURPLE",
    "BLACK",
    "WHITE",
    "TRANSPARENT",
];

/// `Marker.3` to `Marker.2`: a colour becomes a colour name.
///
/// A colour named after one of `Marker.2`'s colours becomes that name in
/// capitals; any other name is kept as it is. A colour with no name at all is
/// left as a colour, as upstream leaves it.
///
/// # Errors
///
/// None; the signature is the one every version function has.
pub fn marker_3_to_2(fields: &mut AnyDictionary) -> Result<()> {
    let Some(Any::Dictionary(color)) = fields.get("color") else {
        return Ok(());
    };
    let name = color.get("name").and_then(Any::as_str).unwrap_or_default();
    if name.is_empty() {
        return Ok(());
    }
    // Upstream upper-cases byte by byte, so only ASCII letters change.
    let upper = name.to_ascii_uppercase();
    let written = if LEGACY_COLOR_NAMES.contains(&upper.as_str()) {
        upper
    } else {
        name.to_string()
    };
    fields.insert("color".to_string(), Any::String(written));
    Ok(())
}

/// `Clip.2` to `Clip.1`: the active media reference becomes the only one.
///
/// # Errors
///
/// None; the signature is the one every version function has.
pub fn clip_2_to_1(fields: &mut AnyDictionary) -> Result<()> {
    let active = match (
        fields.get("media_references"),
        fields.get("active_media_reference_key"),
    ) {
        (Some(Any::Dictionary(references)), Some(Any::String(key))) => match references.get(key) {
            Some(reference @ Any::Dictionary(_)) => Some(reference.clone()),
            _ => None,
        },
        _ => None,
    };
    if let Some(reference) = active {
        fields.insert("media_reference".to_string(), reference);
    }
    fields.remove("media_references");
    fields.remove("active_media_reference_key");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn color_field(name: &str) -> AnyDictionary {
        let mut color = AnyDictionary::new();
        color.insert("OTIO_SCHEMA".into(), "Color.1".into());
        color.insert("name".into(), name.into());
        let mut fields = AnyDictionary::new();
        fields.insert("color".into(), Any::Dictionary(color));
        fields
    }

    #[test]
    fn a_known_colour_goes_back_to_its_legacy_name() {
        let mut fields = color_field("Red");
        marker_3_to_2(&mut fields).unwrap();
        assert_eq!(fields["color"], Any::from("RED"));

        let mut fields = color_field("something unknown");
        marker_3_to_2(&mut fields).unwrap();
        assert_eq!(fields["color"], Any::from("something unknown"));

        // With no name there is nothing to write, so the colour stays.
        let mut fields = color_field("");
        let before = fields.clone();
        marker_3_to_2(&mut fields).unwrap();
        assert_eq!(fields, before);
    }

    #[test]
    fn a_clip_keeps_only_its_active_reference_going_down() {
        let reference = |url: &str| {
            let mut entries = AnyDictionary::new();
            entries.insert("OTIO_SCHEMA".into(), "ExternalReference.1".into());
            entries.insert("target_url".into(), url.into());
            Any::Dictionary(entries)
        };
        let mut references = AnyDictionary::new();
        references.insert("DEFAULT_MEDIA".into(), reference("a.mov"));
        references.insert("proxy".into(), reference("b.mov"));
        let mut fields = AnyDictionary::new();
        fields.insert("media_references".into(), Any::Dictionary(references));
        fields.insert("active_media_reference_key".into(), "proxy".into());

        clip_2_to_1(&mut fields).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields["media_reference"], reference("b.mov"));

        // And up again, a clip without one gains a missing reference.
        let mut fields = AnyDictionary::new();
        clip_1_to_2(&mut fields).unwrap();
        let Any::Dictionary(references) = &fields["media_references"] else {
            panic!("media_references is a dictionary");
        };
        assert!(is_object(&references[DEFAULT_MEDIA_KEY]));
        assert_eq!(
            fields["active_media_reference_key"],
            Any::from("DEFAULT_MEDIA")
        );
    }
}

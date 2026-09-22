//! Upstream's C++ type names, as its reader's error messages spell them.
//!
//! Upstream names types in reading errors with `typeid(T).name()`, special
//! casing only `std::string` (`string`) and `void` (`None`). That is a
//! compiler's spelling, not a stable one: GCC and Clang give the mangled
//! Itanium ABI name, MSVC a readable one. The names here are the ones a GCC
//! or Clang build of the upstream release this library follows (0.19, whose
//! inline namespace is `v0_19`, with Imath 3.2) gives on Linux, where
//! `int64_t` is `long` and so `l`. A macOS build says `x` for `int64_t`; a
//! Windows build says `__int64`, and `class opentimelineio::v0_19::Clip` for
//! a class.

use crate::json::Value;

/// `std::string`.
pub(crate) const STRING: &str = "string";
/// `bool`.
pub(crate) const BOOL: &str = "b";
/// `double`.
pub(crate) const DOUBLE: &str = "d";
/// `int64_t`, which upstream's reader stores every JSON integer as.
pub(crate) const INT64: &str = "l";
/// A JSON `null`, which upstream reads as an empty `std::any`.
pub(crate) const NONE: &str = "None";
/// `AnyDictionary`, which a JSON object without a schema becomes.
pub(crate) const ANY_DICTIONARY: &str = "N14opentimelineio5v0_1913AnyDictionaryE";
/// `AnyVector`, which a JSON array becomes.
pub(crate) const ANY_VECTOR: &str = "N14opentimelineio5v0_199AnyVectorE";
/// `opentime::RationalTime`.
pub(crate) const RATIONAL_TIME: &str = "N8opentime5v0_1912RationalTimeE";
/// `opentime::TimeRange`.
pub(crate) const TIME_RANGE: &str = "N8opentime5v0_199TimeRangeE";
/// `opentime::TimeTransform`.
pub(crate) const TIME_TRANSFORM: &str = "N8opentime5v0_1913TimeTransformE";
/// `Color`.
pub(crate) const COLOR: &str = "N14opentimelineio5v0_195ColorE";
/// `Imath::V2d`.
pub(crate) const V2D: &str = "N9Imath_3_24Vec2IdEE";
/// `Imath::Box2d`.
pub(crate) const BOX2D: &str = "N9Imath_3_23BoxINS_4Vec2IdEEEE";
/// `SerializableObject::ReferenceId`, which a `SerializableObjectRef.1`
/// becomes.
pub(crate) const REFERENCE_ID: &str = "N14opentimelineio5v0_1918SerializableObject11ReferenceIdE";
/// `SerializableObject::Retainer<SerializableObject>`, which upstream holds
/// every object in a `std::any` as.
pub(crate) const RETAINER: &str = "N14opentimelineio5v0_1918SerializableObject8RetainerIS1_EE";

/// `Composable`, which a composition's children must be.
pub(crate) const COMPOSABLE: &str = "N14opentimelineio5v0_1910ComposableE";
/// `Marker`.
pub(crate) const MARKER: &str = "N14opentimelineio5v0_196MarkerE";
/// `Effect`.
pub(crate) const EFFECT: &str = "N14opentimelineio5v0_196EffectE";
/// `MediaReference`.
pub(crate) const MEDIA_REFERENCE: &str = "N14opentimelineio5v0_1914MediaReferenceE";
/// `SerializableObject`.
pub(crate) const SERIALIZABLE_OBJECT: &str = "N14opentimelineio5v0_1918SerializableObjectE";

/// The mangled name of a class in upstream's `opentimelineio` namespace.
pub(crate) fn class(name: &str) -> String {
    format!("N14opentimelineio5v0_19{}{name}E", name.len())
}

/// The C++ class upstream reads an object with this schema name into.
///
/// Legacy names map to the class they were renamed to, and a name upstream
/// does not know becomes its `UnknownSchema`.
pub(crate) fn class_for_schema(schema_name: &str) -> String {
    let class_name = match schema_name {
        "Sequence" => "Track",
        "Filler" => "Gap",
        "SerializeableCollection" => "SerializableCollection",
        known @ ("Clip"
        | "Item"
        | "Gap"
        | "Track"
        | "Stack"
        | "Timeline"
        | "Transition"
        | "Marker"
        | "Effect"
        | "TimeEffect"
        | "LinearTimeWarp"
        | "FreezeFrame"
        | "ExternalReference"
        | "MissingReference"
        | "GeneratorReference"
        | "ImageSequenceReference"
        | "SerializableCollection"
        | "SerializableObject"
        | "SerializableObjectWithMetadata"
        | "Composable"
        | "Composition"
        | "MediaReference") => known,
        _ => "UnknownSchema",
    };
    class(class_name)
}

/// The value types upstream's reader decodes itself rather than through its
/// type registry, by their exact schema string.
pub(crate) fn value_type(schema: &str) -> Option<&'static str> {
    match schema {
        "RationalTime.1" => Some(RATIONAL_TIME),
        "TimeRange.1" => Some(TIME_RANGE),
        "TimeTransform.1" => Some(TIME_TRANSFORM),
        "Color.1" => Some(COLOR),
        "V2d.1" => Some(V2D),
        "Box2d.1" => Some(BOX2D),
        "SerializableObjectRef.1" => Some(REFERENCE_ID),
        _ => None,
    }
}

/// The C++ type upstream's reader turns a JSON value into.
pub(crate) fn of_json(value: &Value) -> String {
    match value {
        Value::Null => NONE.to_string(),
        Value::Bool(_) => BOOL.to_string(),
        Value::Number(crate::json::Number::Double(_)) => DOUBLE.to_string(),
        Value::Number(_) => INT64.to_string(),
        Value::String(_) => STRING.to_string(),
        Value::Array(_) => ANY_VECTOR.to_string(),
        Value::Object(_) => match value.get("OTIO_SCHEMA").and_then(Value::as_str) {
            None => ANY_DICTIONARY.to_string(),
            Some(schema) => value_type(schema).map_or_else(
                || {
                    let name = schema.rsplit_once('.').map_or(schema, |(name, _)| name);
                    class_for_schema(name)
                },
                ToString::to_string,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_mangled_as_gcc_mangles_them() {
        // Checked against OpenTimelineIO 0.18.1's messages, with its
        // `v0_18_1` namespace spelled for 0.19's `v0_19`.
        assert_eq!(class("Clip"), "N14opentimelineio5v0_194ClipE");
        assert_eq!(
            class("ImageSequenceReference"),
            "N14opentimelineio5v0_1922ImageSequenceReferenceE"
        );
        assert_eq!(class("AnyDictionary"), ANY_DICTIONARY);
        assert_eq!(class("AnyVector"), ANY_VECTOR);
        assert_eq!(class("Composable"), COMPOSABLE);
        assert_eq!(class("Marker"), MARKER);
        assert_eq!(class("Effect"), EFFECT);
        assert_eq!(class("MediaReference"), MEDIA_REFERENCE);
        assert_eq!(class("SerializableObject"), SERIALIZABLE_OBJECT);
        assert_eq!(class("Color"), COLOR);
        assert_eq!(class_for_schema("Sequence"), class("Track"));
        assert_eq!(class_for_schema("Blah"), class("UnknownSchema"));
    }

    #[test]
    fn json_values_are_named_as_upstream_stores_them() {
        let parse = |text: &str| crate::json::parse(text).unwrap();
        assert_eq!(of_json(&parse("5")), "l");
        assert_eq!(of_json(&parse("18446744073709551615")), "l");
        assert_eq!(of_json(&parse("5.5")), "d");
        assert_eq!(of_json(&parse("true")), "b");
        assert_eq!(of_json(&parse("null")), "None");
        assert_eq!(of_json(&parse("\"x\"")), "string");
        assert_eq!(of_json(&parse("[]")), ANY_VECTOR);
        assert_eq!(of_json(&parse("{}")), ANY_DICTIONARY);
        assert_eq!(
            of_json(&parse(r#"{"OTIO_SCHEMA": "RationalTime.1"}"#)),
            RATIONAL_TIME
        );
        assert_eq!(
            of_json(&parse(r#"{"OTIO_SCHEMA": "SerializableObjectRef.1"}"#)),
            REFERENCE_ID
        );
        assert_eq!(of_json(&parse(r#"{"OTIO_SCHEMA": "Gap.1"}"#)), class("Gap"));
        assert_eq!(
            of_json(&parse(r#"{"OTIO_SCHEMA": "Blah.1"}"#)),
            class("UnknownSchema")
        );
    }
}

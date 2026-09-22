//! Reading OTIO JSON into a [`Document`].
//!
//! Two properties matter more than anything else here:
//!
//! - **Nothing is dropped.** An object whose schema this library does not know
//!   is kept verbatim as [`UnknownSchema`], so a file written by a third-party
//!   plugin survives a read and rewrite intact.
//! - **Absent fields take upstream's defaults** rather than failing, because
//!   files written by older versions of OpenTimelineIO are missing fields that
//!   later versions added.
//!
//! Every object's schema is looked up in [`crate::registry`] first. An object
//! older than the registered version is upgraded there before it is read; one
//! newer is refused, as upstream refuses it; one whose schema was
//! registered at run time is read as a [`DynamicObject`]; and one whose
//! schema was registered as a subclass of a built-in is read as that
//! built-in, with an [`Extension`] naming the subclass and holding the fields
//! the built-in does not read.

use std::collections::HashMap;

use crate::json::{self, Number, ObjectLines, Value};
use opentime::{RationalTime, TimeRange, TimeTransform};

use crate::arena::{Document, NodeId};
use crate::cxx;
use crate::error::{Error, ReadLocation, ReadObject, Result};
use crate::registry::{self, DynamicBase, SchemaKind};
use crate::schema::{
    Base, Clip, Composable, Composition, DynamicObject, EffectData, Extension, ExternalReference,
    Gap, GeneratorReference, ImageSequenceReference, ItemData, Marker, MediaReferenceData,
    MissingFramePolicy, MissingReference, Node, SerializableCollection, Stack, Timeline, Track,
    Transition, UnknownSchema,
};
use crate::upgrade::color_from_legacy_name;
use crate::value::{Any, AnyDictionary, Box2d, Color, V2d};

/// Parses an OTIO JSON document.
///
/// # Errors
///
/// Returns [`Error::Json`] if the input is not valid JSON, and one of the
/// structural errors if an object is missing its schema tag or a field holds
/// the wrong kind of value.
pub fn from_str(input: &str) -> Result<Document> {
    from_str_unlocated(input).map_err(|error| locate(error, input))
}

/// Parses an OTIO JSON document as [`from_str`] does, but reports an error
/// without saying where in the text it is.
///
/// For text that was never a file: upstream builds an object from a
/// dictionary with no line numbers, and its errors then carry none, so
/// [`crate::registry::instance_from_schema`] reads through this.
pub(crate) fn from_str_unlocated(input: &str) -> Result<Document> {
    let value = json::parse(input)?;
    let mut document = Document::new();
    let mut reader = Reader {
        document: &mut document,
        ids: HashMap::new(),
    };
    let root = reader.read_node(&value, "$", Wanted::SerializableObject)?;
    document.set_root(Some(root));
    Ok(document)
}

/// Says where in the text a reading error happened, as upstream says it.
///
/// Upstream reads bottom-up, decoding each object when its closing brace
/// is reached, and so reports an error at the line of the innermost object
/// being decoded: see [`ReadLocation`]. Keeping line numbers for every
/// object costs time and memory on every read, so this reader does without
/// them, and only when a read fails parses the text again, recording them,
/// to find the lines for the error.
fn locate(error: Error, input: &str) -> Error {
    let Ok((root, lines)) = json::parse_with_object_lines(input) else {
        return error;
    };
    match error {
        Error::TypeMismatch {
            detail,
            path,
            at: None,
        } => {
            let at = if path.ends_with(".OTIO_SCHEMA") {
                // A schema tag that is not a string fails before upstream
                // knows what the object is, so only the line is given.
                let chain = walk(&root, &path);
                chain
                    .iter()
                    .rev()
                    .nth(1)
                    .and_then(|value| value.as_object())
                    .and_then(|entries| lines.line_of(entries))
                    .map(|line| ReadLocation { line, object: None })
            } else {
                reader_of(&root, &lines, &path)
            };
            Error::TypeMismatch { detail, path, at }
        }
        Error::MissingSchema {
            expected,
            path,
            at: None,
        } => {
            let at = reader_of(&root, &lines, &path);
            Error::MissingSchema { expected, path, at }
        }
        Error::UnknownMissingFramePolicy {
            name,
            path,
            at: None,
        } => {
            let at = reader_of(&root, &lines, &path);
            Error::UnknownMissingFramePolicy { name, path, at }
        }
        Error::MalformedSchema {
            schema,
            path,
            line: None,
        } => {
            let line = object_line(&root, &lines, &path);
            Error::MalformedSchema { schema, path, line }
        }
        Error::UnsupportedSchemaVersion {
            schema,
            version,
            highest,
            path,
            line: None,
        } => {
            let line = object_line(&root, &lines, &path);
            Error::UnsupportedSchemaVersion {
                schema,
                version,
                highest,
                path,
                line,
            }
        }
        Error::UnresolvedReference {
            id,
            path,
            line: None,
        } => {
            // Upstream resolves references once the object holding them has
            // been decoded, and gives that object's line.
            let line = reader_of(&root, &lines, &path).map(|at| at.line);
            Error::UnresolvedReference { id, path, line }
        }
        error => error,
    }
}

/// The line of the closing brace of the object at `path`.
fn object_line(root: &Value, lines: &ObjectLines, path: &str) -> Option<usize> {
    walk(root, path)
        .last()
        .and_then(|value| value.as_object())
        .and_then(|entries| lines.line_of(entries))
}

/// Finds the object upstream would be decoding when it found a problem with
/// the value at `path`: the innermost object strictly around it that has a
/// schema tag.
fn reader_of(root: &Value, lines: &ObjectLines, path: &str) -> Option<ReadLocation> {
    let chain = walk(root, path);
    let ancestors = chain.get(..chain.len().saturating_sub(1))?;
    ancestors.iter().rev().find_map(|value| {
        let entries = value.as_object()?;
        let schema = lookup(entries, "OTIO_SCHEMA")?.as_str()?;
        let line = lines.line_of(entries)?;
        let object = if cxx::value_type(schema).is_some() {
            // Upstream decodes a value type without naming it.
            None
        } else {
            let name = lookup(entries, "name")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>");
            let class_name = schema.rsplit_once('.').map_or(schema, |(name, _)| name);
            Some(ReadObject {
                name: name.to_string(),
                type_name: cxx::class_for_schema(class_name),
            })
        };
        Some(ReadLocation { line, object })
    })
}

/// Follows a path this reader built, such as `$.tracks.children[0].name`,
/// and returns every value along it, the root first.
///
/// A key may itself hold a `.` or a `[`, so where more than one key fits,
/// the longest is taken.
fn walk<'v>(root: &'v Value, path: &str) -> Vec<&'v Value> {
    let mut chain = vec![root];
    let mut rest = path.strip_prefix('$').unwrap_or(path);
    let mut current = root;
    while !rest.is_empty() {
        let next = match current {
            Value::Object(entries) => rest.strip_prefix('.').and_then(|after| {
                entries
                    .iter()
                    .filter(|(key, _)| {
                        after.strip_prefix(key.as_str()).is_some_and(|tail| {
                            tail.is_empty() || tail.starts_with('.') || tail.starts_with('[')
                        })
                    })
                    .max_by_key(|(key, _)| key.len())
                    .map(|(key, value)| (value, &after[key.len()..]))
            }),
            Value::Array(entries) => rest
                .strip_prefix('[')
                .and_then(|after| after.split_once(']'))
                .and_then(|(index, tail)| {
                    let index: usize = index.parse().ok()?;
                    entries.get(index).map(|value| (value, tail))
                }),
            _ => None,
        };
        let Some((value, tail)) = next else {
            break;
        };
        chain.push(value);
        current = value;
        rest = tail;
    }
    chain
}

/// The kind of object a place in the document must hold.
#[derive(Clone, Copy)]
enum Wanted {
    /// A composition's child.
    Composable,
    /// An entry in an item's `markers`.
    Marker,
    /// An entry in an item's `effects`.
    Effect,
    /// An entry in a clip's `media_references`.
    MediaReference,
    /// A timeline's `tracks`.
    Stack,
    /// Anything at all, such as a collection's child.
    SerializableObject,
}

impl Wanted {
    /// The C++ type upstream names when what it read is not an object of a
    /// kind it knows.
    ///
    /// A list or dictionary entry is read straight into the kind wanted; a
    /// single object, such as a timeline's `tracks`, is read as any object
    /// first, and only then checked for its kind.
    fn read_as(self) -> &'static str {
        match self {
            Self::Composable => cxx::COMPOSABLE,
            Self::Marker => cxx::MARKER,
            Self::Effect => cxx::EFFECT,
            Self::MediaReference => cxx::MEDIA_REFERENCE,
            Self::Stack | Self::SerializableObject => cxx::SERIALIZABLE_OBJECT,
        }
    }

    /// The C++ type upstream names for this kind.
    fn cxx(self) -> String {
        match self {
            Self::Composable => cxx::COMPOSABLE.to_string(),
            Self::Marker => cxx::MARKER.to_string(),
            Self::Effect => cxx::EFFECT.to_string(),
            Self::MediaReference => cxx::MEDIA_REFERENCE.to_string(),
            Self::Stack => cxx::class("Stack"),
            Self::SerializableObject => cxx::SERIALIZABLE_OBJECT.to_string(),
        }
    }

    /// Whether an object with this schema name is of this kind.
    ///
    /// A schema this library does not know is let through anywhere, and
    /// kept as it is, where upstream reads it as an `UnknownSchema` and
    /// refuses it everywhere but a collection or metadata.
    fn admits(self, schema_name: &str) -> bool {
        let known = cxx::class_for_schema(schema_name) != cxx::class("UnknownSchema");
        if !known {
            return true;
        }
        match self {
            Self::SerializableObject => true,
            Self::Composable => matches!(
                schema_name,
                "Clip"
                    | "Item"
                    | "Gap"
                    | "Filler"
                    | "Track"
                    | "Sequence"
                    | "Stack"
                    | "Transition"
                    | "Composable"
                    | "Composition"
            ),
            Self::Marker => schema_name == "Marker",
            Self::Effect => matches!(
                schema_name,
                "Effect" | "TimeEffect" | "LinearTimeWarp" | "FreezeFrame"
            ),
            Self::MediaReference => matches!(
                schema_name,
                "MediaReference"
                    | "ExternalReference"
                    | "MissingReference"
                    | "GeneratorReference"
                    | "ImageSequenceReference"
            ),
            Self::Stack => schema_name == "Stack",
        }
    }
}

/// The C++ type upstream holds a JSON value as, in a `std::any`.
///
/// This is [`cxx::of_json`], except that an object with a schema is held
/// as a `Retainer`, whatever it is.
fn held_as(value: &Value) -> String {
    let found = cxx::of_json(value);
    let is_object = value
        .get("OTIO_SCHEMA")
        .and_then(Value::as_str)
        .is_some_and(|schema| cxx::value_type(schema).is_none());
    if is_object {
        cxx::RETAINER.to_string()
    } else {
        found
    }
}

/// A field held a value of the wrong type, in upstream's words.
fn field_mismatch(expected: &str, key: &str, value: &Value, path: String) -> Error {
    Error::TypeMismatch {
        detail: format!(
            "expected type {expected} under key '{key}': found type {} instead",
            held_as(value)
        ),
        path,
        at: None,
    }
}

/// A field that holds a container, or a number upstream reads through the
/// same template, held a value of the wrong type, in upstream's words.
fn container_mismatch(expected: &str, value: &Value, path: String) -> Error {
    Error::TypeMismatch {
        detail: format!(
            "while decoding complex STL type, expected type '{expected}', found type '{}' \
             instead",
            held_as(value)
        ),
        path,
        at: None,
    }
}

/// Reads a field that holds a value type, which must carry that type's
/// schema tag.
fn value_type_field<'v>(
    value: &'v Value,
    expected: &'static str,
    key: &str,
    path: &str,
) -> Result<&'v [(String, Value)]> {
    match value.as_object() {
        Some(entries) if cxx::of_json(value) == expected => Ok(entries),
        _ => Err(field_mismatch(expected, key, value, path.to_string())),
    }
}

/// Parses an OTIO JSON document whose root may be any value, not only an
/// object.
///
/// Upstream's reader returns whatever the file holds: a list of objects, a
/// plain dictionary, or a lone `RationalTime`, as well as the usual timeline.
/// Objects anywhere in the value live in the returned document, which has
/// the value as its root when the value is itself an object.
///
/// # Errors
///
/// As [`from_str`].
pub fn from_str_any(input: &str) -> Result<(Document, Any)> {
    let value = json::parse(input)?;
    let mut document = Document::new();
    let mut reader = Reader {
        document: &mut document,
        ids: HashMap::new(),
    };
    let root = reader
        .read_any(&value, "$")
        .map_err(|error| locate(error, input))?;
    if let Any::Object(id) = root {
        document.set_root(Some(id));
    }
    Ok((document, root))
}

struct Reader<'a> {
    document: &'a mut Document,
    /// Objects that declared an `OTIO_REF_ID`, so later references resolve.
    ids: HashMap<String, NodeId>,
}

/// Splits an `OTIO_SCHEMA` value into its name and version.
///
/// The version is read as upstream reads it, with `std::stoi`: leading
/// whitespace and a sign are allowed and anything after the digits is
/// ignored, so `Clip. 2` and `Clip.2x` are both version 2. A version that
/// does not fit in an `int` is malformed, as upstream finds it; so is a
/// negative one, which upstream accepts and then crashes on.
fn split_schema(schema: &str, path: &str) -> Result<(String, u32)> {
    let malformed = || Error::MalformedSchema {
        schema: schema.to_string(),
        path: path.to_string(),
        line: None,
    };
    let (name, version) = schema.rsplit_once('.').ok_or_else(malformed)?;
    let version = stoi(version).ok_or_else(malformed)?;
    let version = u32::try_from(version).map_err(|_| malformed())?;
    Ok((name.to_string(), version))
}

/// Reads a decimal number the way `std::stoi` does, or `None` where it
/// would throw.
fn stoi(text: &str) -> Option<i32> {
    // C's isspace: space, \t, \n, \v, \f and \r.
    let text = text.trim_start_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r']);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let magnitude: i64 = digits[..end].parse().ok()?;
    i32::try_from(if negative { -magnitude } else { magnitude }).ok()
}

/// Looks a key up in an object's entries. A duplicate key takes the first.
fn lookup<'v>(object: &'v [(String, Value)], name: &str) -> Option<&'v Value> {
    object
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// Looks a key up, treating an explicit `null` as absent so that the field
/// falls back to its default.
fn field<'v>(object: &'v [(String, Value)], name: &str) -> Option<&'v Value> {
    lookup(object, name).filter(|value| !value.is_null())
}

/// Reads a number as an `f64`, whatever form it was written in.
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(number.as_f64()),
        _ => None,
    }
}

/// Reads a number as an `i64`, if it was written as an integer.
///
/// Upstream stores every JSON integer as an `int64_t`, dropping the top
/// bit of one too large for it, and refuses a number written with a
/// fraction or an exponent even where its value is whole.
fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(Number::Int(inner)) => Some(*inner),
        Value::Number(Number::UInt(inner)) => i64::try_from(inner & 0x7FFF_FFFF_FFFF_FFFF).ok(),
        _ => None,
    }
}

/// Reads a sequence's missing-frame policy.
///
/// An absent field takes the default, but a name this library does not know
/// is an error rather than a fallback: upstream refuses the file, on the
/// grounds that quietly treating an unknown policy as `error` would change
/// what a player does with the media.
fn read_missing_frame_policy(object: &[(String, Value)], path: &str) -> Result<MissingFramePolicy> {
    let Some(value) = field(object, "missing_frame_policy") else {
        return Ok(MissingFramePolicy::default());
    };
    // Upstream reads a policy that is not a string as an empty name, and
    // so reports it as unknown rather than as the wrong type.
    let name = value.as_str().unwrap_or_default();
    MissingFramePolicy::from_name(name).ok_or_else(|| Error::UnknownMissingFramePolicy {
        name: name.to_string(),
        path: format!("{path}.missing_frame_policy"),
        at: None,
    })
}

fn read_string(object: &[(String, Value)], name: &'static str, path: &str) -> Result<String> {
    match field(object, name) {
        None => Ok(String::new()),
        Some(value) => value
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| field_mismatch(cxx::STRING, name, value, format!("{path}.{name}"))),
    }
}

fn read_bool(
    object: &[(String, Value)],
    name: &'static str,
    default: bool,
    path: &str,
) -> Result<bool> {
    match field(object, name) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| field_mismatch(cxx::BOOL, name, value, format!("{path}.{name}"))),
    }
}

fn read_f64(
    object: &[(String, Value)],
    name: &'static str,
    default: f64,
    path: &str,
) -> Result<f64> {
    match field(object, name) {
        None => Ok(default),
        Some(value) => as_f64(value)
            .ok_or_else(|| field_mismatch(cxx::DOUBLE, name, value, format!("{path}.{name}"))),
    }
}

fn read_i64(
    object: &[(String, Value)],
    name: &'static str,
    default: i64,
    path: &str,
) -> Result<i64> {
    match field(object, name) {
        None => Ok(default),
        // Upstream reads these through its container template, whose
        // message differs from a plain field's.
        Some(value) => as_i64(value)
            .ok_or_else(|| container_mismatch(cxx::INT64, value, format!("{path}.{name}"))),
    }
}

/// Reads a bare `{rate, value}` body, as found inside a `RationalTime.1`.
fn read_rational_time_body(object: &[(String, Value)], path: &str) -> Result<RationalTime> {
    Ok(RationalTime::new(
        read_f64(object, "value", 0.0, path)?,
        read_f64(object, "rate", 1.0, path)?,
    ))
}

impl Reader<'_> {
    /// Reads a value that may be plain JSON, an OTIO value type, or a nested
    /// object.
    fn read_any(&mut self, value: &Value, path: &str) -> Result<Any> {
        match value {
            Value::Null => Ok(Any::Null),
            Value::Bool(inner) => Ok(Any::Bool(*inner)),
            Value::Number(Number::Int(inner)) => Ok(Any::Int(*inner)),
            Value::Number(Number::UInt(inner)) => Ok(Any::UInt(*inner)),
            Value::Number(Number::Double(inner)) => Ok(Any::Double(*inner)),
            Value::String(inner) => Ok(Any::String(inner.clone())),
            Value::Array(entries) => {
                let mut result = Vec::with_capacity(entries.len());
                for (index, entry) in entries.iter().enumerate() {
                    result.push(self.read_any(entry, &format!("{path}[{index}]"))?);
                }
                Ok(Any::Vector(result))
            }
            Value::Object(object) => self.read_any_object(object, path),
        }
    }

    fn read_any_object(&mut self, object: &[(String, Value)], path: &str) -> Result<Any> {
        let Some(schema) = schema_tag(object, path)? else {
            // A plain dictionary, such as a nested block of metadata.
            let mut result = AnyDictionary::new();
            for (key, entry) in object {
                result.insert(key.clone(), self.read_any(entry, &format!("{path}.{key}"))?);
            }
            return Ok(Any::Dictionary(result));
        };

        let (name, _version) = split_schema(schema, path)?;
        match name.as_str() {
            "RationalTime" => Ok(Any::RationalTime(read_rational_time_body(object, path)?)),
            "TimeRange" => Ok(Any::TimeRange(self.read_time_range_body(object, path)?)),
            "TimeTransform" => Ok(Any::TimeTransform(
                self.read_time_transform_body(object, path)?,
            )),
            "Color" => Ok(Any::Color(read_color_body(object, path)?)),
            "V2d" => Ok(Any::V2d(read_v2d_body(object, path)?)),
            "Box2d" => Ok(Any::Box2d(self.read_box2d_body(object, path)?)),
            "SerializableObjectRef" => {
                let id = read_string(object, "id", path)?;
                self.ids.get(&id).copied().map(Any::Object).ok_or_else(|| {
                    Error::UnresolvedReference {
                        id,
                        path: path.to_string(),
                        line: None,
                    }
                })
            }
            // Anything else with a schema tag is an OTIO object, which
            // metadata is allowed to hold.
            _ => {
                let id = self.read_object(object, &name, schema, path)?;
                Ok(Any::Object(id))
            }
        }
    }

    fn read_nested_time(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        path: &str,
    ) -> Result<RationalTime> {
        let path = format!("{path}.{name}");
        match field(object, name) {
            None => Ok(RationalTime::default()),
            Some(value) => {
                let inner = value_type_field(value, cxx::RATIONAL_TIME, name, &path)?;
                Ok(read_rational_time_body(inner, &path)?)
            }
        }
    }

    fn read_time_range_body(
        &mut self,
        object: &[(String, Value)],
        path: &str,
    ) -> Result<TimeRange> {
        Ok(TimeRange::new(
            self.read_nested_time(object, "start_time", path)?,
            self.read_nested_time(object, "duration", path)?,
        ))
    }

    fn read_time_transform_body(
        &mut self,
        object: &[(String, Value)],
        path: &str,
    ) -> Result<TimeTransform> {
        Ok(TimeTransform::new(
            self.read_nested_time(object, "offset", path)?,
            read_f64(object, "scale", 1.0, path)?,
            read_f64(object, "rate", -1.0, path)?,
        ))
    }

    fn read_box2d_body(&mut self, object: &[(String, Value)], path: &str) -> Result<Box2d> {
        let corner = |name: &'static str| -> Result<V2d> {
            let path = format!("{path}.{name}");
            match field(object, name) {
                None => Ok(V2d::default()),
                Some(value) => {
                    read_v2d_body(value_type_field(value, cxx::V2D, name, &path)?, &path)
                }
            }
        };
        Ok(Box2d::new(corner("min")?, corner("max")?))
    }

    fn read_optional_time_range(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        path: &str,
    ) -> Result<Option<TimeRange>> {
        let path = format!("{path}.{name}");
        match field(object, name) {
            None => Ok(None),
            Some(value) => Ok(Some(self.read_time_range_body(
                value_type_field(value, cxx::TIME_RANGE, name, &path)?,
                &path,
            )?)),
        }
    }

    fn read_optional_color(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        legacy_names: bool,
        path: &str,
    ) -> Result<Option<Color>> {
        let path = format!("{path}.{name}");
        match field(object, name) {
            None => Ok(None),
            // Marker.2 and earlier wrote the colour as a bare name such as
            // "RED"; Marker.3 writes a Color.1 object. Upgrade the name to the
            // colour it stood for. An item's colour came later, and was
            // never a name.
            Some(Value::String(color)) if legacy_names => Ok(Some(color_from_legacy_name(color))),
            Some(value) => Ok(Some(read_color_body(
                value_type_field(value, cxx::COLOR, name, &path)?,
                &path,
            )?)),
        }
    }

    fn read_optional_box2d(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        path: &str,
    ) -> Result<Option<Box2d>> {
        let path = format!("{path}.{name}");
        match field(object, name) {
            None => Ok(None),
            Some(value) => Ok(Some(self.read_box2d_body(
                value_type_field(value, cxx::BOX2D, name, &path)?,
                &path,
            )?)),
        }
    }

    fn read_dictionary(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        path: &str,
    ) -> Result<AnyDictionary> {
        let path = format!("{path}.{name}");
        let Some(value) = field(object, name) else {
            return Ok(AnyDictionary::new());
        };
        let inner = match value.as_object() {
            Some(entries) if cxx::of_json(value) == cxx::ANY_DICTIONARY => entries,
            _ => return Err(field_mismatch(cxx::ANY_DICTIONARY, name, value, path)),
        };
        let mut result = AnyDictionary::new();
        for (key, entry) in inner {
            result.insert(key.clone(), self.read_any(entry, &format!("{path}.{key}"))?);
        }
        Ok(result)
    }

    fn read_node_list(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        wanted: Wanted,
        path: &str,
    ) -> Result<Vec<NodeId>> {
        let path = format!("{path}.{name}");
        let Some(value) = field(object, name) else {
            return Ok(Vec::new());
        };
        let entries = value
            .as_array()
            .ok_or_else(|| container_mismatch(cxx::ANY_VECTOR, value, path.clone()))?;
        let mut result = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            result.push(self.read_node(entry, &format!("{path}[{index}]"), wanted)?);
        }
        Ok(result)
    }

    fn read_base(&mut self, object: &[(String, Value)], path: &str) -> Result<Base> {
        Ok(Base {
            name: read_string(object, "name", path)?,
            metadata: self.read_dictionary(object, "metadata", path)?,
            extension: None,
        })
    }

    fn read_item(&mut self, object: &[(String, Value)], path: &str) -> Result<ItemData> {
        Ok(ItemData {
            base: self.read_base(object, path)?,
            parent: None,
            source_range: self.read_optional_time_range(object, "source_range", path)?,
            effects: self.read_node_list(object, "effects", Wanted::Effect, path)?,
            markers: self.read_node_list(object, "markers", Wanted::Marker, path)?,
            enabled: read_bool(object, "enabled", true, path)?,
            color: self.read_optional_color(object, "color", false, path)?,
        })
    }

    fn read_effect(&mut self, object: &[(String, Value)], path: &str) -> Result<EffectData> {
        Ok(EffectData {
            base: self.read_base(object, path)?,
            effect_name: read_string(object, "effect_name", path)?,
            enabled: read_bool(object, "enabled", true, path)?,
        })
    }

    fn read_media(&mut self, object: &[(String, Value)], path: &str) -> Result<MediaReferenceData> {
        Ok(MediaReferenceData {
            base: self.read_base(object, path)?,
            available_range: self.read_optional_time_range(object, "available_range", path)?,
            available_image_bounds: self.read_optional_box2d(
                object,
                "available_image_bounds",
                path,
            )?,
        })
    }

    /// Reads an object that must be an OTIO object of the kind wanted,
    /// rather than a value type.
    fn read_node(&mut self, value: &Value, path: &str, wanted: Wanted) -> Result<NodeId> {
        let not_an_object = |found: String| Error::TypeMismatch {
            detail: format!(
                "expected to read a {}, found a {found} instead",
                wanted.read_as()
            ),
            path: path.to_string(),
            at: None,
        };
        let Some(object) = value.as_object() else {
            return Err(not_an_object(cxx::of_json(value)));
        };
        let Some(schema) = schema_tag(object, path)? else {
            return Err(Error::MissingSchema {
                expected: wanted.read_as(),
                path: path.to_string(),
                at: None,
            });
        };
        let (name, _version) = split_schema(schema, path)?;

        if name == "SerializableObjectRef" {
            let id = read_string(object, "id", path)?;
            return match self.ids.get(&id) {
                Some(id) => Ok(*id),
                // Upstream resolves references only in metadata, so one
                // anywhere else stays a reference id, of the wrong type.
                None => Err(not_an_object(cxx::REFERENCE_ID.to_string())),
            };
        }
        if let Some(value_type) = cxx::value_type(schema) {
            return Err(not_an_object(value_type.to_string()));
        }
        // An object of a subclass is the built-in it derives from, and is
        // held wherever that built-in may be.
        let built_in = registry::built_in_schema(&name);
        let kind = built_in.unwrap_or(&name);
        if !wanted.admits(kind) {
            let found = cxx::class_for_schema(kind);
            return Err(Error::TypeMismatch {
                detail: match wanted {
                    // A single object is read through `Retainer<T>`, which
                    // words this its own way.
                    Wanted::Stack => format!(
                        "Expected object of type {}; read type {found} instead",
                        wanted.cxx()
                    ),
                    _ => format!(
                        "expected to read a {}, found a {found} instead",
                        wanted.cxx()
                    ),
                },
                path: path.to_string(),
                at: None,
            });
        }

        self.read_object(object, &name, schema, path)
    }

    /// Builds a node from an object body whose schema has already been split.
    fn read_object(
        &mut self,
        original: &[(String, Value)],
        name: &str,
        schema: &str,
        path: &str,
    ) -> Result<NodeId> {
        let (_, mut version) = split_schema(schema, path)?;

        // What the registry says decides how the object is read: not at all
        // if it is too new, upgraded first if it is too old.
        let found = registry::find(name);
        let upgraded;
        let mut object = original;
        if let Some(found) = &found {
            if version > found.version {
                return Err(Error::UnsupportedSchemaVersion {
                    schema: name.to_string(),
                    version,
                    highest: found.version,
                    path: path.to_string(),
                    line: None,
                });
            }
            if version < found.version {
                upgraded = self.upgrade(original, name, version, found.version, path)?;
                object = &upgraded;
                version = found.version;
            }
        }

        let node = match (name, found.map(|found| found.kind)) {
            // Not a schema anybody registered. Keep every field so that
            // rewriting the file does not discard it.
            (_, None) | ("UnknownSchema", _) => Node::Unknown(UnknownSchema {
                original_schema_name: name.to_string(),
                original_schema_version: version,
                data: self.read_fields(object, &[], path)?,
            }),
            (_, Some(SchemaKind::Dynamic(base))) => {
                self.read_dynamic(object, name, version, base, path)?
            }
            (_, Some(SchemaKind::Subclass(built_in))) => {
                let node = self.read_built_in(object, built_in, version, path)?;
                let schema = Some((name.to_string(), version));
                self.read_extension(node, object, built_in, schema, path)?
            }
            _ => {
                let node = self.read_built_in(object, name, version, path)?;
                self.read_extension(node, object, name, None, path)?
            }
        };

        let id = self.document.insert(node);
        self.link_children(id);

        // An object may declare an id that later references point back at.
        // Upstream writes the full object before any reference to it, so a
        // forward reference does not arise in practice.
        if let Some(Value::String(ref_id)) = lookup(original, "OTIO_REF_ID") {
            self.ids.insert(ref_id.clone(), id);
        }

        Ok(id)
    }

    /// Reads every field of an object except its schema tag, its reference
    /// id and those named in `skip`.
    fn read_fields(
        &mut self,
        object: &[(String, Value)],
        skip: &[&str],
        path: &str,
    ) -> Result<AnyDictionary> {
        let mut data = AnyDictionary::new();
        for (key, entry) in object {
            if key == "OTIO_SCHEMA" || key == "OTIO_REF_ID" || skip.contains(&key.as_str()) {
                continue;
            }
            data.insert(key.clone(), self.read_any(entry, &format!("{path}.{key}"))?);
        }
        Ok(data)
    }

    /// Reads an object of a schema registered at run time.
    fn read_dynamic(
        &mut self,
        object: &[(String, Value)],
        name: &str,
        version: u32,
        base: DynamicBase,
        path: &str,
    ) -> Result<Node> {
        let (base, skip): (_, &[&str]) = match base {
            DynamicBase::SerializableObject => (None, &[]),
            DynamicBase::SerializableObjectWithMetadata => {
                (Some(self.read_base(object, path)?), &["name", "metadata"])
            }
        };
        Ok(Node::Dynamic(DynamicObject {
            schema_name: name.to_string(),
            schema_version: version,
            base,
            fields: self.read_fields(object, skip, path)?,
        }))
    }

    /// Gives a built-in object read from `object` the extension it needs:
    /// the subclass `schema` it is an instance of, if any, and every field
    /// the reader of `built_in` did not take.
    ///
    /// Upstream reads an object of a subclass into the concrete class it
    /// derives from, and any field that class does not read, on any object,
    /// is kept in its dynamic fields and written back out; this does both.
    /// An object with neither keeps no extension at all.
    fn read_extension(
        &mut self,
        mut node: Node,
        object: &[(String, Value)],
        built_in: &str,
        schema: Option<(String, u32)>,
        path: &str,
    ) -> Result<Node> {
        // An unknown schema holds every field already, and the root classes
        // with fields beyond their own were read as dynamic objects.
        if matches!(node, Node::Unknown(_) | Node::Dynamic(_)) {
            return Ok(node);
        }
        let own = own_fields(built_in);
        if schema.is_none() && !has_fields_beyond(object, own) {
            return Ok(node);
        }
        let fields = self.read_fields(object, own, path)?;
        if let Some(base) = node.base_mut() {
            base.extension = Some(Box::new(Extension { schema, fields }));
        }
        Ok(node)
    }

    /// Runs the registered upgrade functions on an object read at `from`,
    /// and returns it as it would have been written at `to`.
    fn upgrade(
        &mut self,
        object: &[(String, Value)],
        name: &str,
        from: u32,
        to: u32,
        path: &str,
    ) -> Result<Vec<(String, Value)>> {
        let functions = registry::upgrades(name, from);
        if functions.is_empty() {
            let mut result = object.to_vec();
            set_schema(&mut result, name, to);
            return Ok(result);
        }

        let mut fields = AnyDictionary::new();
        for (key, entry) in object {
            if key == "OTIO_SCHEMA" || key == "OTIO_REF_ID" {
                continue;
            }
            fields.insert(
                key.clone(),
                self.plain_any(entry, &format!("{path}.{key}"))?,
            );
        }
        for function in functions {
            function(&mut fields)?;
        }

        let mut result = vec![(
            "OTIO_SCHEMA".to_string(),
            Value::String(format!("{name}.{to}")),
        )];
        result.extend(
            fields
                .iter()
                .map(|(key, value)| Ok((key.clone(), value_from_any(value, path)?)))
                .collect::<Result<Vec<_>>>()?,
        );
        Ok(result)
    }

    /// Reads a value into the self-contained form version functions see:
    /// value types become values, but objects stay dictionaries.
    fn plain_any(&mut self, value: &Value, path: &str) -> Result<Any> {
        match value {
            Value::Array(entries) => {
                let mut result = Vec::with_capacity(entries.len());
                for (index, entry) in entries.iter().enumerate() {
                    result.push(self.plain_any(entry, &format!("{path}[{index}]"))?);
                }
                Ok(Any::Vector(result))
            }
            Value::Object(object) => {
                let value_type = lookup(object, "OTIO_SCHEMA")
                    .and_then(Value::as_str)
                    .and_then(|schema| schema.rsplit_once('.'))
                    .is_some_and(|(name, _)| {
                        matches!(
                            name,
                            "RationalTime"
                                | "TimeRange"
                                | "TimeTransform"
                                | "Color"
                                | "V2d"
                                | "Box2d"
                        )
                    });
                if value_type {
                    return self.read_any_object(object, path);
                }
                let mut result = AnyDictionary::new();
                for (key, entry) in object {
                    result.insert(
                        key.clone(),
                        self.plain_any(entry, &format!("{path}.{key}"))?,
                    );
                }
                Ok(Any::Dictionary(result))
            }
            _ => self.read_any(value, path),
        }
    }

    /// Reads an object of one of the schemas built into this library.
    fn read_built_in(
        &mut self,
        object: &[(String, Value)],
        name: &str,
        version: u32,
        path: &str,
    ) -> Result<Node> {
        Ok(match name {
            "Clip" => Node::Clip(Clip {
                item: self.read_item(object, path)?,
                media_references: self.read_media_references(object, path)?,
                active_media_reference_key: read_string(
                    object,
                    "active_media_reference_key",
                    path,
                )?,
            }),
            "Item" => Node::Item(self.read_item(object, path)?),
            "Gap" | "Filler" => Node::Gap(Gap {
                item: self.read_item(object, path)?,
            }),
            "Track" | "Sequence" => Node::Track(Track {
                item: self.read_item(object, path)?,
                children: self.read_node_list(object, "children", Wanted::Composable, path)?,
                kind: read_string(object, "kind", path)?,
            }),
            "Stack" => Node::Stack(Stack {
                item: self.read_item(object, path)?,
                children: self.read_node_list(object, "children", Wanted::Composable, path)?,
            }),
            "Timeline" => Node::Timeline(Timeline {
                base: self.read_base(object, path)?,
                tracks: match field(object, "tracks") {
                    None => None,
                    Some(value) => {
                        Some(self.read_node(value, &format!("{path}.tracks"), Wanted::Stack)?)
                    }
                },
                global_start_time: match field(object, "global_start_time") {
                    None => None,
                    Some(value) => {
                        let path = format!("{path}.global_start_time");
                        Some(read_rational_time_body(
                            value_type_field(
                                value,
                                cxx::RATIONAL_TIME,
                                "global_start_time",
                                &path,
                            )?,
                            &path,
                        )?)
                    }
                },
            }),
            "Transition" => Node::Transition(Transition {
                base: self.read_base(object, path)?,
                parent: None,
                in_offset: self.read_nested_time(object, "in_offset", path)?,
                out_offset: self.read_nested_time(object, "out_offset", path)?,
                transition_type: read_string(object, "transition_type", path)?,
                enabled: read_bool(object, "enabled", true, path)?,
            }),
            "Marker" => Node::Marker(Marker {
                base: self.read_base(object, path)?,
                color: self.read_optional_color(object, "color", true, path)?,
                // Marker.1 called this field `range`.
                marked_range: match self.read_optional_time_range(object, "marked_range", path)? {
                    Some(range) => range,
                    None => self
                        .read_optional_time_range(object, "range", path)?
                        .unwrap_or_default(),
                },
                comment: read_string(object, "comment", path)?,
            }),
            "Effect" => Node::Effect(self.read_effect(object, path)?),
            "TimeEffect" => Node::TimeEffect(self.read_effect(object, path)?),
            "LinearTimeWarp" => Node::LinearTimeWarp {
                effect: self.read_effect(object, path)?,
                time_scalar: read_f64(object, "time_scalar", 1.0, path)?,
            },
            "FreezeFrame" => Node::FreezeFrame {
                effect: self.read_effect(object, path)?,
                time_scalar: read_f64(object, "time_scalar", 0.0, path)?,
            },
            "ExternalReference" => Node::ExternalReference(ExternalReference {
                media: self.read_media(object, path)?,
                target_url: read_string(object, "target_url", path)?,
            }),
            "MissingReference" => Node::MissingReference(MissingReference {
                media: self.read_media(object, path)?,
            }),
            "GeneratorReference" => Node::GeneratorReference(GeneratorReference {
                media: self.read_media(object, path)?,
                generator_kind: read_string(object, "generator_kind", path)?,
                parameters: self.read_dictionary(object, "parameters", path)?,
            }),
            "ImageSequenceReference" => Node::ImageSequenceReference(ImageSequenceReference {
                media: self.read_media(object, path)?,
                target_url_base: read_string(object, "target_url_base", path)?,
                name_prefix: read_string(object, "name_prefix", path)?,
                name_suffix: read_string(object, "name_suffix", path)?,
                start_frame: read_i64(object, "start_frame", 1, path)?,
                frame_step: read_i64(object, "frame_step", 1, path)?,
                rate: read_f64(object, "rate", 1.0, path)?,
                frame_zero_padding: read_i64(object, "frame_zero_padding", 0, path)?,
                missing_frame_policy: read_missing_frame_policy(object, path)?,
            }),
            // The misspelling is a legacy alias an old release wrote, and
            // upstream still maps it to the correct schema.
            "SerializableCollection" | "SerializeableCollection" => {
                Node::SerializableCollection(SerializableCollection {
                    base: self.read_base(object, path)?,
                    children: self.read_node_list(
                        object,
                        "children",
                        Wanted::SerializableObject,
                        path,
                    )?,
                })
            }
            // Upstream's base classes, which it registers as schemas in
            // their own right and its Python API can construct directly.
            // Either may carry dynamic fields beyond its own, and then it
            // is read as upstream holds it: as the base class with those
            // fields beside.
            "SerializableObject" if has_fields_beyond(object, &[]) => {
                self.read_dynamic(object, name, version, DynamicBase::SerializableObject, path)?
            }
            "SerializableObject" => Node::SerializableObject,
            "SerializableObjectWithMetadata"
                if has_fields_beyond(object, &["name", "metadata"]) =>
            {
                self.read_dynamic(
                    object,
                    name,
                    version,
                    DynamicBase::SerializableObjectWithMetadata,
                    path,
                )?
            }
            "SerializableObjectWithMetadata" => {
                Node::SerializableObjectWithMetadata(self.read_base(object, path)?)
            }
            "Composable" => Node::Composable(Composable {
                base: self.read_base(object, path)?,
                parent: None,
            }),
            "Composition" => Node::Composition(Composition {
                item: self.read_item(object, path)?,
                children: self.read_node_list(object, "children", Wanted::Composable, path)?,
            }),
            "MediaReference" => Node::MediaReference(self.read_media(object, path)?),
            // Registered as built in, but with no reader of its own. Keep
            // every field so that rewriting the file does not discard it.
            _ => Node::Unknown(UnknownSchema {
                original_schema_name: name.to_string(),
                original_schema_version: version,
                data: self.read_fields(object, &[], path)?,
            }),
        })
    }

    fn read_media_references(
        &mut self,
        object: &[(String, Value)],
        path: &str,
    ) -> Result<std::collections::BTreeMap<String, NodeId>> {
        let path = format!("{path}.media_references");
        let Some(value) = field(object, "media_references") else {
            return Ok(std::collections::BTreeMap::new());
        };
        let inner = match value.as_object() {
            Some(entries) if cxx::of_json(value) == cxx::ANY_DICTIONARY => entries,
            _ => return Err(container_mismatch(cxx::ANY_DICTIONARY, value, path)),
        };
        let mut result = std::collections::BTreeMap::new();
        for (key, entry) in inner {
            result.insert(
                key.clone(),
                self.read_node(entry, &format!("{path}.{key}"), Wanted::MediaReference)?,
            );
        }
        Ok(result)
    }

    /// Points a composition's children back at it.
    ///
    /// The parent link is not serialized, so it is rebuilt from the nesting.
    fn link_children(&mut self, parent: NodeId) {
        let Some(node) = self.document.get(parent) else {
            return;
        };
        let Some(children) = node.children() else {
            return;
        };
        let children = children.to_vec();
        for child in children {
            if let Some(child) = self.document.get_mut(child) {
                child.set_parent(Some(parent));
            }
        }
    }
}

/// Returns an object's schema tag, or `None` if it has none.
///
/// # Errors
///
/// A tag that is not a string is the wrong type, as upstream finds it.
fn schema_tag<'v>(object: &'v [(String, Value)], path: &str) -> Result<Option<&'v str>> {
    match lookup(object, "OTIO_SCHEMA") {
        None => Ok(None),
        Some(Value::String(schema)) => Ok(Some(schema)),
        Some(value) => Err(field_mismatch(
            cxx::STRING,
            "OTIO_SCHEMA",
            value,
            format!("{path}.OTIO_SCHEMA"),
        )),
    }
}

/// Whether an object has fields other than its schema tag, its reference id
/// and those named in `own`.
fn has_fields_beyond(object: &[(String, Value)], own: &[&str]) -> bool {
    object.iter().any(|(key, _)| {
        key != "OTIO_SCHEMA" && key != "OTIO_REF_ID" && !own.contains(&key.as_str())
    })
}

/// The fields the reader of a built-in schema takes, and a subclass of it
/// therefore does not keep as its own.
///
/// These are what the writer writes for each, plus the older names the
/// reader still accepts (`Marker.1`'s `range`). A schema not listed has no
/// reader of its own.
fn own_fields(built_in: &str) -> &'static [&'static str] {
    const BASE: [&str; 2] = ["metadata", "name"];
    macro_rules! with {
        ($($field:literal),* $(,)?) => {
            &["metadata", "name", $($field),*]
        };
    }
    macro_rules! item {
        ($($field:literal),* $(,)?) => {
            with!["source_range", "effects", "markers", "enabled", "color", $($field),*]
        };
    }
    macro_rules! media {
        ($($field:literal),* $(,)?) => {
            with!["available_range", "available_image_bounds", $($field),*]
        };
    }
    match built_in {
        "Clip" => item!["media_references", "active_media_reference_key"],
        "Item" | "Gap" | "Filler" => item![],
        "Track" | "Sequence" => item!["children", "kind"],
        "Stack" | "Composition" => item!["children"],
        "Timeline" => with!["global_start_time", "tracks"],
        "Transition" => with!["in_offset", "out_offset", "transition_type", "enabled"],
        "Marker" => with!["color", "marked_range", "range", "comment"],
        "Effect" | "TimeEffect" => with!["effect_name", "enabled"],
        "LinearTimeWarp" | "FreezeFrame" => with!["effect_name", "enabled", "time_scalar"],
        "MediaReference" | "MissingReference" => media![],
        "ExternalReference" => media!["target_url"],
        "GeneratorReference" => media!["generator_kind", "parameters"],
        "ImageSequenceReference" => media![
            "target_url_base",
            "name_prefix",
            "name_suffix",
            "start_frame",
            "frame_step",
            "rate",
            "frame_zero_padding",
            "missing_frame_policy",
        ],
        "SerializableCollection" | "SerializeableCollection" => with!["children"],
        _ => &BASE,
    }
}

/// Sets an object's `OTIO_SCHEMA` entry.
fn set_schema(object: &mut Vec<(String, Value)>, name: &str, version: u32) {
    let schema = Value::String(format!("{name}.{version}"));
    match object.iter_mut().find(|(key, _)| key == "OTIO_SCHEMA") {
        Some((_, value)) => *value = schema,
        None => object.insert(0, ("OTIO_SCHEMA".to_string(), schema)),
    }
}

/// Turns a value in the self-contained form back into JSON, for the reader
/// to read as though the file had said it.
///
/// # Errors
///
/// [`Error::TypeMismatch`] for a handle into a document, which the
/// self-contained form cannot hold.
pub(crate) fn value_from_any(value: &Any, path: &str) -> Result<Value> {
    let number = |value: f64| Value::Number(Number::Double(value));
    let object = |schema: &str, entries: Vec<(&str, Value)>| {
        let mut result = vec![("OTIO_SCHEMA".to_string(), Value::String(schema.to_string()))];
        result.extend(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value)),
        );
        Value::Object(result)
    };
    let time = |time: RationalTime| {
        object(
            "RationalTime.1",
            vec![
                ("rate", number(time.rate())),
                ("value", number(time.value())),
            ],
        )
    };
    let point = |point: V2d| {
        object(
            "V2d.1",
            vec![("x", number(point.x)), ("y", number(point.y))],
        )
    };
    Ok(match value {
        Any::Null => Value::Null,
        Any::Bool(inner) => Value::Bool(*inner),
        Any::Int(inner) => Value::Number(Number::Int(*inner)),
        Any::UInt(inner) => Value::Number(Number::UInt(*inner)),
        Any::Double(inner) => number(*inner),
        Any::String(inner) => Value::String(inner.clone()),
        Any::RationalTime(inner) => time(*inner),
        Any::TimeRange(inner) => object(
            "TimeRange.1",
            vec![
                ("duration", time(inner.duration())),
                ("start_time", time(inner.start_time())),
            ],
        ),
        Any::TimeTransform(inner) => object(
            "TimeTransform.1",
            vec![
                ("offset", time(inner.offset())),
                ("rate", number(inner.rate())),
                ("scale", number(inner.scale())),
            ],
        ),
        Any::Color(inner) => object(
            "Color.1",
            vec![
                ("r", number(inner.r)),
                ("g", number(inner.g)),
                ("b", number(inner.b)),
                ("a", number(inner.a)),
                ("name", Value::String(inner.name.clone())),
            ],
        ),
        Any::V2d(inner) => point(*inner),
        Any::Box2d(inner) => object(
            "Box2d.1",
            vec![("min", point(inner.min)), ("max", point(inner.max))],
        ),
        Any::Vector(items) => Value::Array(
            items
                .iter()
                .map(|item| value_from_any(item, path))
                .collect::<Result<_>>()?,
        ),
        Any::Dictionary(entries) => Value::Object(
            entries
                .iter()
                .map(|(key, item)| Ok((key.clone(), value_from_any(item, path)?)))
                .collect::<Result<_>>()?,
        ),
        other => {
            return Err(Error::TypeMismatch {
                // Upstream's version functions see objects as dictionaries,
                // so it has no such case; the wording is this library's own.
                detail: format!(
                    "expected a value with no object handles in it, found {} instead",
                    other.type_name()
                ),
                path: path.to_string(),
                at: None,
            });
        }
    })
}

fn read_color_body(object: &[(String, Value)], path: &str) -> Result<Color> {
    Ok(Color {
        r: read_f64(object, "r", 1.0, path)?,
        g: read_f64(object, "g", 1.0, path)?,
        b: read_f64(object, "b", 1.0, path)?,
        a: read_f64(object, "a", 1.0, path)?,
        name: read_string(object, "name", path)?,
    })
}

fn read_v2d_body(object: &[(String, Value)], path: &str) -> Result<V2d> {
    Ok(V2d::new(
        read_f64(object, "x", 0.0, path)?,
        read_f64(object, "y", 0.0, path)?,
    ))
}

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

use std::collections::HashMap;

use crate::json::{self, Number, Value};
use opentime::{RationalTime, TimeRange, TimeTransform};

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::schema::{
    Base, Clip, EffectData, ExternalReference, Gap, GeneratorReference, ImageSequenceReference,
    ItemData, Marker, MediaReferenceData, MissingFramePolicy, MissingReference, Node,
    SerializableCollection, Stack, Timeline, Track, Transition, UnknownSchema,
};
use crate::upgrade::{DEFAULT_MEDIA_KEY, color_from_legacy_name};
use crate::value::{Any, AnyDictionary, Box2d, Color, V2d};

/// Parses an OTIO JSON document.
///
/// # Errors
///
/// Returns [`Error::Json`] if the input is not valid JSON, and one of the
/// structural errors if an object is missing its schema tag or a field holds
/// the wrong kind of value.
pub fn from_str(input: &str) -> Result<Document> {
    let value = json::parse(input)?;
    let mut document = Document::new();
    let mut reader = Reader {
        document: &mut document,
        ids: HashMap::new(),
    };
    let root = reader.read_node(&value, "$")?;
    document.set_root(Some(root));
    Ok(document)
}

struct Reader<'a> {
    document: &'a mut Document,
    /// Objects that declared an `OTIO_REF_ID`, so later references resolve.
    ids: HashMap<String, NodeId>,
}

/// Splits an `OTIO_SCHEMA` value into its name and version.
fn split_schema(schema: &str, path: &str) -> Result<(String, u32)> {
    let (name, version) = schema
        .rsplit_once('.')
        .ok_or_else(|| Error::MalformedSchema {
            schema: schema.to_string(),
            path: path.to_string(),
        })?;
    let version = version.parse().map_err(|_| Error::MalformedSchema {
        schema: schema.to_string(),
        path: path.to_string(),
    })?;
    Ok((name.to_string(), version))
}

fn expect_object<'v>(value: &'v Value, path: &str) -> Result<&'v [(String, Value)]> {
    value.as_object().ok_or_else(|| Error::TypeMismatch {
        expected: "object",
        found: describe(value),
        path: path.to_string(),
    })
}

/// Names a JSON value's type, for error messages.
fn describe(value: &Value) -> String {
    value.type_name().to_string()
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

/// Reads a number as an `i64`, if it was written as an integer that fits.
fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(Number::Int(inner)) => Some(*inner),
        Value::Number(Number::UInt(inner)) => i64::try_from(*inner).ok(),
        Value::Number(Number::Double(inner)) if inner.fract() == 0.0 => Some(*inner as i64),
        _ => None,
    }
}

fn read_string(object: &[(String, Value)], name: &'static str, path: &str) -> Result<String> {
    match field(object, name) {
        None => Ok(String::new()),
        Some(value) => value
            .as_str()
            .map(ToString::to_string)
            .ok_or_else(|| Error::TypeMismatch {
                expected: "string",
                found: describe(value),
                path: format!("{path}.{name}"),
            }),
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
        Some(value) => value.as_bool().ok_or_else(|| Error::TypeMismatch {
            expected: "bool",
            found: describe(value),
            path: format!("{path}.{name}"),
        }),
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
        Some(value) => as_f64(value).ok_or_else(|| Error::TypeMismatch {
            expected: "number",
            found: describe(value),
            path: format!("{path}.{name}"),
        }),
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
        Some(value) => as_i64(value).ok_or_else(|| Error::TypeMismatch {
            expected: "integer",
            found: describe(value),
            path: format!("{path}.{name}"),
        }),
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
        let Some(schema) = lookup(object, "OTIO_SCHEMA").and_then(Value::as_str) else {
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
                let inner = expect_object(value, &path)?;
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
                Some(value) => read_v2d_body(expect_object(value, &path)?, &path),
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
            Some(value) => Ok(Some(
                self.read_time_range_body(expect_object(value, &path)?, &path)?,
            )),
        }
    }

    fn read_optional_color(
        &mut self,
        object: &[(String, Value)],
        name: &'static str,
        path: &str,
    ) -> Result<Option<Color>> {
        let path = format!("{path}.{name}");
        match field(object, name) {
            None => Ok(None),
            // Marker.2 and earlier wrote the colour as a bare name such as
            // "RED"; Marker.3 writes a Color.1 object. Upgrade the name to the
            // colour it stood for.
            Some(Value::String(name)) => Ok(Some(color_from_legacy_name(name))),
            Some(value) => Ok(Some(read_color_body(expect_object(value, &path)?, &path)?)),
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
            Some(value) => Ok(Some(
                self.read_box2d_body(expect_object(value, &path)?, &path)?,
            )),
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
        let inner = expect_object(value, &path)?;
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
        path: &str,
    ) -> Result<Vec<NodeId>> {
        let path = format!("{path}.{name}");
        let Some(value) = field(object, name) else {
            return Ok(Vec::new());
        };
        let entries = value.as_array().ok_or_else(|| Error::TypeMismatch {
            expected: "array",
            found: describe(value),
            path: path.clone(),
        })?;
        let mut result = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            result.push(self.read_node(entry, &format!("{path}[{index}]"))?);
        }
        Ok(result)
    }

    fn read_base(&mut self, object: &[(String, Value)], path: &str) -> Result<Base> {
        Ok(Base {
            name: read_string(object, "name", path)?,
            metadata: self.read_dictionary(object, "metadata", path)?,
        })
    }

    fn read_item(&mut self, object: &[(String, Value)], path: &str) -> Result<ItemData> {
        Ok(ItemData {
            base: self.read_base(object, path)?,
            parent: None,
            source_range: self.read_optional_time_range(object, "source_range", path)?,
            effects: self.read_node_list(object, "effects", path)?,
            markers: self.read_node_list(object, "markers", path)?,
            enabled: read_bool(object, "enabled", true, path)?,
            color: self.read_optional_color(object, "color", path)?,
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

    /// Reads an object that must be an OTIO object rather than a value type.
    fn read_node(&mut self, value: &Value, path: &str) -> Result<NodeId> {
        let object = expect_object(value, path)?;
        let schema = lookup(object, "OTIO_SCHEMA")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::MissingSchema {
                path: path.to_string(),
            })?;
        let (name, _version) = split_schema(schema, path)?;

        if name == "SerializableObjectRef" {
            let id = read_string(object, "id", path)?;
            return self
                .ids
                .get(&id)
                .copied()
                .ok_or_else(|| Error::UnresolvedReference {
                    id,
                    path: path.to_string(),
                });
        }

        self.read_object(object, &name, schema, path)
    }

    /// Builds a node from an object body whose schema has already been split.
    fn read_object(
        &mut self,
        object: &[(String, Value)],
        name: &str,
        schema: &str,
        path: &str,
    ) -> Result<NodeId> {
        let (_, version) = split_schema(schema, path)?;

        let node = match name {
            "Clip" => {
                let item = self.read_item(object, path)?;
                let (media_references, active_media_reference_key) = if version < 2 {
                    self.upgrade_clip_media_reference(object, path)?
                } else {
                    (
                        self.read_media_references(object, path)?,
                        read_string(object, "active_media_reference_key", path)?,
                    )
                };
                Node::Clip(Clip {
                    item,
                    media_references,
                    active_media_reference_key,
                })
            }
            "Item" => Node::Item(self.read_item(object, path)?),
            "Gap" | "Filler" => Node::Gap(Gap {
                item: self.read_item(object, path)?,
            }),
            "Track" | "Sequence" => Node::Track(Track {
                item: self.read_item(object, path)?,
                children: self.read_node_list(object, "children", path)?,
                kind: read_string(object, "kind", path)?,
            }),
            "Stack" => Node::Stack(Stack {
                item: self.read_item(object, path)?,
                children: self.read_node_list(object, "children", path)?,
            }),
            "Timeline" => Node::Timeline(Timeline {
                base: self.read_base(object, path)?,
                tracks: match field(object, "tracks") {
                    None => None,
                    Some(value) => Some(self.read_node(value, &format!("{path}.tracks"))?),
                },
                global_start_time: match field(object, "global_start_time") {
                    None => None,
                    Some(value) => {
                        let path = format!("{path}.global_start_time");
                        Some(read_rational_time_body(
                            expect_object(value, &path)?,
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
                color: self.read_optional_color(object, "color", path)?,
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
                missing_frame_policy: field(object, "missing_frame_policy")
                    .and_then(Value::as_str)
                    .and_then(MissingFramePolicy::from_name)
                    .unwrap_or_default(),
            }),
            // The misspelling is a legacy alias an old release wrote, and
            // upstream still maps it to the correct schema.
            "SerializableCollection" | "SerializeableCollection" => {
                Node::SerializableCollection(SerializableCollection {
                    base: self.read_base(object, path)?,
                    children: self.read_node_list(object, "children", path)?,
                })
            }
            // Not a schema this library knows. Keep every field so that
            // rewriting the file does not discard it.
            _ => {
                let mut data = AnyDictionary::new();
                for (key, entry) in object {
                    if key == "OTIO_SCHEMA" || key == "OTIO_REF_ID" {
                        continue;
                    }
                    data.insert(key.clone(), self.read_any(entry, &format!("{path}.{key}"))?);
                }
                Node::Unknown(UnknownSchema {
                    original_schema_name: name.to_string(),
                    original_schema_version: version,
                    data,
                })
            }
        };

        let id = self.document.insert(node);
        self.link_children(id);

        // An object may declare an id that later references point back at.
        // Upstream writes the full object before any reference to it, so a
        // forward reference does not arise in practice.
        if let Some(Value::String(ref_id)) = lookup(object, "OTIO_REF_ID") {
            self.ids.insert(ref_id.clone(), id);
        }

        Ok(id)
    }

    /// Reads a `Clip.1`, whose single `media_reference` becomes the
    /// `DEFAULT_MEDIA` entry of a `Clip.2`'s `media_references`.
    ///
    /// A `Clip.1` with no media reference used to default to a
    /// `MissingReference`, so one is supplied here to keep that behaviour.
    fn upgrade_clip_media_reference(
        &mut self,
        object: &[(String, Value)],
        path: &str,
    ) -> Result<(std::collections::BTreeMap<String, NodeId>, String)> {
        let reference = match field(object, "media_reference") {
            Some(value) => self.read_node(value, &format!("{path}.media_reference"))?,
            None => self
                .document
                .insert(Node::MissingReference(MissingReference::default())),
        };
        let mut references = std::collections::BTreeMap::new();
        references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);
        Ok((references, DEFAULT_MEDIA_KEY.to_string()))
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
        let inner = expect_object(value, &path)?;
        let mut result = std::collections::BTreeMap::new();
        for (key, entry) in inner {
            result.insert(
                key.clone(),
                self.read_node(entry, &format!("{path}.{key}"))?,
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

//! Writing a [`Document`] out as OTIO JSON.
//!
//! Key order is part of the format's readability, not just its content, so
//! this emits fields in the order upstream's `write_to` methods do rather than
//! sorting them. Metadata dictionaries are the exception: upstream backs them
//! with an ordered map, so their keys come out sorted, and a `BTreeMap` gives
//! the same result.

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::schema::{EffectData, ItemData, MediaReferenceData, Node};
use crate::value::{Any, AnyDictionary, Box2d, Color, V2d};

/// The indentation upstream's Python bindings write by default.
pub const DEFAULT_INDENT: usize = 4;

/// Serializes a document as OTIO JSON, indented by [`DEFAULT_INDENT`].
///
/// # Errors
///
/// Returns [`Error::StaleHandle`] if the document holds a handle to an object
/// that has been removed, and [`Error::MissingField`] if it has no root.
pub fn to_string(document: &Document) -> Result<String> {
    to_string_pretty(document, DEFAULT_INDENT)
}

/// Serializes a document as OTIO JSON with the given indentation.
///
/// # Errors
///
/// As [`to_string`].
pub fn to_string_pretty(document: &Document, indent: usize) -> Result<String> {
    let root = document.root().ok_or(Error::MissingField {
        field: "root",
        path: "$".to_string(),
    })?;

    let mut writer = Writer {
        document,
        out: String::new(),
        indent,
        level: 0,
    };
    writer.write_node(root)?;
    writer.out.push('\n');
    Ok(writer.out)
}

/// Formats a float the way upstream's JSON writer does.
///
/// RapidJSON is configured with `kWriteNanAndInfFlag`, so non-finite values
/// are written as bare `NaN` and `Infinity` literals. That is not valid JSON
/// and no strict parser will read it back, upstream's included; it is
/// reproduced here so that a document containing one is not silently altered.
fn format_f64(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }

    // Rust's Display gives the shortest string that round-trips, but always in
    // plain decimal and without a fractional part on a whole number. JSON
    // makes no distinction, but OTIO's schema does: `rate` and `value` are
    // doubles and upstream writes them as `24.0`, so match that.
    let plain = value.to_string();

    // A value with a large exponent turns into hundreds of digits in plain
    // decimal. Fall back to exponential form when it is shorter, as upstream's
    // writer does.
    let exponential = format!("{value:e}");
    let shortest = if exponential.len() < plain.len() {
        exponential
    } else {
        plain
    };

    if shortest.contains(['.', 'e', 'E']) {
        shortest
    } else {
        format!("{shortest}.0")
    }
}

/// Escapes a string as a JSON string literal.
fn format_string(value: &str) -> String {
    crate::json::escape(value)
}

struct Writer<'a> {
    document: &'a Document,
    out: String,
    indent: usize,
    level: usize,
}

/// Tracks whether a separator is needed before the next entry.
struct Nesting {
    wrote_any: bool,
}

impl Writer<'_> {
    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..(self.level * self.indent) {
            self.out.push(' ');
        }
    }

    fn begin_object(&mut self) -> Nesting {
        self.out.push('{');
        self.level += 1;
        Nesting { wrote_any: false }
    }

    fn end_object(&mut self, nesting: Nesting) {
        self.level -= 1;
        if nesting.wrote_any {
            self.newline();
        }
        self.out.push('}');
    }

    fn begin_array(&mut self) -> Nesting {
        self.out.push('[');
        self.level += 1;
        Nesting { wrote_any: false }
    }

    fn end_array(&mut self, nesting: Nesting) {
        self.level -= 1;
        if nesting.wrote_any {
            self.newline();
        }
        self.out.push(']');
    }

    /// Opens a field, writing the separator and key. The value follows.
    fn key(&mut self, nesting: &mut Nesting, name: &str) {
        if nesting.wrote_any {
            self.out.push(',');
        }
        nesting.wrote_any = true;
        self.newline();
        self.out.push_str(&format_string(name));
        self.out.push_str(": ");
    }

    /// Opens an array element, writing the separator.
    fn element(&mut self, nesting: &mut Nesting) {
        if nesting.wrote_any {
            self.out.push(',');
        }
        nesting.wrote_any = true;
        self.newline();
    }

    fn schema(&mut self, nesting: &mut Nesting, name: &str, version: u32) {
        self.key(nesting, "OTIO_SCHEMA");
        self.out
            .push_str(&format_string(&format!("{name}.{version}")));
    }

    fn write_str(&mut self, value: &str) {
        self.out.push_str(&format_string(value));
    }

    fn write_f64(&mut self, value: f64) {
        self.out.push_str(&format_f64(value));
    }

    fn write_null(&mut self) {
        self.out.push_str("null");
    }

    fn write_bool(&mut self, value: bool) {
        self.out.push_str(if value { "true" } else { "false" });
    }

    fn write_i64(&mut self, value: i64) {
        self.out.push_str(&value.to_string());
    }

    fn write_rational_time(&mut self, value: opentime::RationalTime) {
        let mut nesting = self.begin_object();
        self.schema(&mut nesting, "RationalTime", 1);
        self.key(&mut nesting, "rate");
        self.write_f64(value.rate());
        self.key(&mut nesting, "value");
        self.write_f64(value.value());
        self.end_object(nesting);
    }

    fn write_time_range(&mut self, value: opentime::TimeRange) {
        let mut nesting = self.begin_object();
        self.schema(&mut nesting, "TimeRange", 1);
        self.key(&mut nesting, "duration");
        self.write_rational_time(value.duration());
        self.key(&mut nesting, "start_time");
        self.write_rational_time(value.start_time());
        self.end_object(nesting);
    }

    fn write_time_transform(&mut self, value: opentime::TimeTransform) {
        let mut nesting = self.begin_object();
        self.schema(&mut nesting, "TimeTransform", 1);
        self.key(&mut nesting, "offset");
        self.write_rational_time(value.offset());
        self.key(&mut nesting, "rate");
        self.write_f64(value.rate());
        self.key(&mut nesting, "scale");
        self.write_f64(value.scale());
        self.end_object(nesting);
    }

    fn write_color(&mut self, value: &Color) {
        let mut nesting = self.begin_object();
        self.schema(&mut nesting, "Color", 1);
        for (name, component) in [
            ("r", value.r),
            ("g", value.g),
            ("b", value.b),
            ("a", value.a),
        ] {
            self.key(&mut nesting, name);
            self.write_f64(component);
        }
        self.key(&mut nesting, "name");
        self.write_str(&value.name);
        self.end_object(nesting);
    }

    fn write_v2d(&mut self, value: V2d) {
        let mut nesting = self.begin_object();
        self.schema(&mut nesting, "V2d", 1);
        self.key(&mut nesting, "x");
        self.write_f64(value.x);
        self.key(&mut nesting, "y");
        self.write_f64(value.y);
        self.end_object(nesting);
    }

    fn write_box2d(&mut self, value: Box2d) {
        let mut nesting = self.begin_object();
        self.schema(&mut nesting, "Box2d", 1);
        self.key(&mut nesting, "min");
        self.write_v2d(value.min);
        self.key(&mut nesting, "max");
        self.write_v2d(value.max);
        self.end_object(nesting);
    }

    fn write_dictionary(&mut self, value: &AnyDictionary) -> Result<()> {
        let mut nesting = self.begin_object();
        for (name, entry) in value {
            self.key(&mut nesting, name);
            self.write_any(entry)?;
        }
        self.end_object(nesting);
        Ok(())
    }

    fn write_any(&mut self, value: &Any) -> Result<()> {
        match value {
            Any::Null => self.write_null(),
            Any::Bool(inner) => self.write_bool(*inner),
            Any::Int(inner) => self.write_i64(*inner),
            Any::UInt(inner) => self.out.push_str(&inner.to_string()),
            Any::Double(inner) => self.write_f64(*inner),
            Any::String(inner) => self.write_str(inner),
            Any::RationalTime(inner) => self.write_rational_time(*inner),
            Any::TimeRange(inner) => self.write_time_range(*inner),
            Any::TimeTransform(inner) => self.write_time_transform(*inner),
            Any::Color(inner) => self.write_color(inner),
            Any::V2d(inner) => self.write_v2d(*inner),
            Any::Box2d(inner) => self.write_box2d(*inner),
            Any::Vector(inner) => {
                let mut nesting = self.begin_array();
                for entry in inner {
                    self.element(&mut nesting);
                    self.write_any(entry)?;
                }
                self.end_array(nesting);
            }
            Any::Dictionary(inner) => self.write_dictionary(inner)?,
            Any::Object(id) => self.write_node(*id)?,
        }
        Ok(())
    }

    fn write_optional_time_range(
        &mut self,
        nesting: &mut Nesting,
        name: &str,
        value: Option<opentime::TimeRange>,
    ) {
        self.key(nesting, name);
        match value {
            Some(range) => self.write_time_range(range),
            None => self.write_null(),
        }
    }

    fn write_node_list(&mut self, nesting: &mut Nesting, name: &str, ids: &[NodeId]) -> Result<()> {
        self.key(nesting, name);
        let mut array = self.begin_array();
        for id in ids {
            self.element(&mut array);
            self.write_node(*id)?;
        }
        self.end_array(array);
        Ok(())
    }

    /// Writes the fields every named object shares: metadata, then name.
    fn write_base(&mut self, nesting: &mut Nesting, base: &crate::schema::Base) -> Result<()> {
        self.key(nesting, "metadata");
        self.write_dictionary(&base.metadata)?;
        self.key(nesting, "name");
        self.write_str(&base.name);
        Ok(())
    }

    /// Writes the fields every item shares, base fields included.
    fn write_item(&mut self, nesting: &mut Nesting, item: &ItemData) -> Result<()> {
        self.write_base(nesting, &item.base)?;
        self.write_optional_time_range(nesting, "source_range", item.source_range);
        self.write_node_list(nesting, "effects", &item.effects)?;
        self.write_node_list(nesting, "markers", &item.markers)?;
        self.key(nesting, "enabled");
        self.write_bool(item.enabled);
        self.key(nesting, "color");
        match &item.color {
            Some(color) => self.write_color(color),
            None => self.write_null(),
        }
        Ok(())
    }

    fn write_effect(&mut self, nesting: &mut Nesting, effect: &EffectData) -> Result<()> {
        self.write_base(nesting, &effect.base)?;
        self.key(nesting, "effect_name");
        self.write_str(&effect.effect_name);
        self.key(nesting, "enabled");
        self.write_bool(effect.enabled);
        Ok(())
    }

    fn write_media(&mut self, nesting: &mut Nesting, media: &MediaReferenceData) -> Result<()> {
        self.write_base(nesting, &media.base)?;
        self.write_optional_time_range(nesting, "available_range", media.available_range);
        self.key(nesting, "available_image_bounds");
        match media.available_image_bounds {
            Some(bounds) => self.write_box2d(bounds),
            None => self.write_null(),
        }
        Ok(())
    }

    fn write_node(&mut self, id: NodeId) -> Result<()> {
        // Take a copy of the document reference so the borrow of the node is
        // tied to the document rather than to `self`, leaving `self` free to
        // be borrowed mutably while the node is in hand.
        let document = self.document;
        let node = document.try_get(id)?;

        let mut nesting = self.begin_object();
        self.schema(&mut nesting, node.schema_name(), node.schema_version());

        match node {
            Node::Unknown(unknown) => {
                // Held verbatim, so it goes back out exactly as it came in.
                for (name, entry) in &unknown.data {
                    self.key(&mut nesting, name);
                    self.write_any(entry)?;
                }
            }
            Node::Clip(clip) => {
                self.write_item(&mut nesting, &clip.item)?;
                self.key(&mut nesting, "media_references");
                let mut references = self.begin_object();
                for (key, reference) in &clip.media_references {
                    self.key(&mut references, key);
                    self.write_node(*reference)?;
                }
                self.end_object(references);
                self.key(&mut nesting, "active_media_reference_key");
                self.write_str(&clip.active_media_reference_key);
            }
            Node::Item(item) => {
                self.write_item(&mut nesting, item)?;
            }
            Node::Gap(gap) => {
                self.write_item(&mut nesting, &gap.item)?;
            }
            Node::Track(track) => {
                self.write_item(&mut nesting, &track.item)?;
                self.write_node_list(&mut nesting, "children", &track.children)?;
                self.key(&mut nesting, "kind");
                self.write_str(&track.kind);
            }
            Node::Stack(stack) => {
                self.write_item(&mut nesting, &stack.item)?;
                self.write_node_list(&mut nesting, "children", &stack.children)?;
            }
            Node::Timeline(timeline) => {
                self.write_base(&mut nesting, &timeline.base)?;
                self.key(&mut nesting, "global_start_time");
                match timeline.global_start_time {
                    Some(time) => self.write_rational_time(time),
                    None => self.write_null(),
                }
                self.key(&mut nesting, "tracks");
                match timeline.tracks {
                    Some(stack) => self.write_node(stack)?,
                    None => self.write_null(),
                }
            }
            Node::Transition(transition) => {
                self.write_base(&mut nesting, &transition.base)?;
                self.key(&mut nesting, "in_offset");
                self.write_rational_time(transition.in_offset);
                self.key(&mut nesting, "out_offset");
                self.write_rational_time(transition.out_offset);
                self.key(&mut nesting, "transition_type");
                self.write_str(&transition.transition_type);
                self.key(&mut nesting, "enabled");
                self.write_bool(transition.enabled);
            }
            Node::Marker(marker) => {
                self.write_base(&mut nesting, &marker.base)?;
                self.key(&mut nesting, "color");
                match &marker.color {
                    Some(color) => self.write_color(color),
                    None => self.write_null(),
                }
                self.key(&mut nesting, "marked_range");
                self.write_time_range(marker.marked_range);
                self.key(&mut nesting, "comment");
                self.write_str(&marker.comment);
            }
            Node::Effect(effect) | Node::TimeEffect(effect) => {
                self.write_effect(&mut nesting, effect)?;
            }
            Node::LinearTimeWarp {
                effect,
                time_scalar,
            }
            | Node::FreezeFrame {
                effect,
                time_scalar,
            } => {
                self.write_effect(&mut nesting, effect)?;
                self.key(&mut nesting, "time_scalar");
                self.write_f64(*time_scalar);
            }
            Node::ExternalReference(reference) => {
                self.write_media(&mut nesting, &reference.media)?;
                self.key(&mut nesting, "target_url");
                self.write_str(&reference.target_url);
            }
            Node::MissingReference(reference) => {
                self.write_media(&mut nesting, &reference.media)?;
            }
            Node::GeneratorReference(reference) => {
                self.write_media(&mut nesting, &reference.media)?;
                self.key(&mut nesting, "generator_kind");
                self.write_str(&reference.generator_kind);
                self.key(&mut nesting, "parameters");
                self.write_dictionary(&reference.parameters)?;
            }
            Node::ImageSequenceReference(reference) => {
                self.write_media(&mut nesting, &reference.media)?;
                self.key(&mut nesting, "target_url_base");
                self.write_str(&reference.target_url_base);
                self.key(&mut nesting, "name_prefix");
                self.write_str(&reference.name_prefix);
                self.key(&mut nesting, "name_suffix");
                self.write_str(&reference.name_suffix);
                self.key(&mut nesting, "start_frame");
                self.write_i64(reference.start_frame);
                self.key(&mut nesting, "frame_step");
                self.write_i64(reference.frame_step);
                self.key(&mut nesting, "rate");
                self.write_f64(reference.rate);
                self.key(&mut nesting, "frame_zero_padding");
                self.write_i64(reference.frame_zero_padding);
                self.key(&mut nesting, "missing_frame_policy");
                self.write_str(reference.missing_frame_policy.as_str());
            }
            Node::SerializableCollection(collection) => {
                self.write_base(&mut nesting, &collection.base)?;
                self.write_node_list(&mut nesting, "children", &collection.children)?;
            }
        }

        self.end_object(nesting);
        Ok(())
    }
}

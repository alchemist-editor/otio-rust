//! Writing a [`Document`] out as OTIO JSON.
//!
//! Key order is part of the format's readability, not just its content, so
//! this emits fields in the order upstream's `write_to` methods do rather than
//! sorting them. Metadata dictionaries are the exception: upstream backs them
//! with an ordered map, so their keys come out sorted, and a `BTreeMap` gives
//! the same result.

use std::collections::HashSet;

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::json::{Number, Value};
use crate::registry::SchemaVersionMap;
use crate::schema::{EffectData, ItemData, MediaReferenceData, Node};
use crate::value::{Any, AnyDictionary, Box2d, Color, V2d};

/// The indentation upstream's Python bindings write by default.
pub const DEFAULT_INDENT: usize = 4;

/// How a value is written: its layout, and which release it is written for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// Spaces per level of nesting, with each entry on a line of its own; or
    /// `None` for compact JSON with no whitespace at all.
    ///
    /// Pretty output ends with a newline and compact output does not.
    pub indent: Option<usize>,
    /// The schema version to write each named schema at, for a reader that
    /// knows no newer one.
    ///
    /// An object whose schema is newer than its target is downgraded on the
    /// way out by the functions in [`crate::registry`], and so is everything
    /// inside it; see [`to_string_with`]. Upstream calls this
    /// `schema_version_targets`.
    pub schema_version_targets: SchemaVersionMap,
}

impl Default for WriteOptions {
    /// Indented by [`DEFAULT_INDENT`], at the current schema versions.
    fn default() -> Self {
        Self {
            indent: Some(DEFAULT_INDENT),
            schema_version_targets: SchemaVersionMap::new(),
        }
    }
}

/// Serializes any value, objects in it resolved against `document`.
///
/// This is the general form of the other functions here. Two things happen
/// that the simple forms never need:
///
/// - **Downgrading.** An object whose schema has a target in
///   [`WriteOptions::schema_version_targets`] below its own version is
///   written as upstream writes it: turned into a dictionary, together with
///   everything it holds, and walked from the inside out, each dictionary
///   whose schema has a lower target passed through that schema's downgrade
///   functions one version at a time. Being dictionaries by then, their keys
///   come out sorted.
/// - **Cycle detection.** An object that holds itself, however deep down,
///   cannot be written. Holding the same object in two places is fine, and
///   each place gets a full copy, as upstream writes it.
///
/// # Errors
///
/// [`Error::ObjectCycle`] for an object met again inside itself,
/// [`Error::NoDowngradeFunction`] when a target cannot be reached, whatever a
/// downgrade function returns, and [`Error::StaleHandle`] for a handle to an
/// object that has been removed.
pub fn to_string_with(document: &Document, value: &Any, options: &WriteOptions) -> Result<String> {
    let mut writer = Writer::new(document, options, HashSet::new());
    writer.write_any(value)?;
    if options.indent.is_some() {
        writer.out.push('\n');
    }
    Ok(writer.out)
}

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
    to_string_pretty_from(document, root, indent)
}

/// Serializes one object in a document, rather than the document's root.
///
/// Upstream's `write_to_string` takes any object, not only a timeline — its
/// own tests round-trip a bare clip — so the writer has to be able to start
/// anywhere.
///
/// # Errors
///
/// As [`to_string`].
pub fn to_string_pretty_from(document: &Document, root: NodeId, indent: usize) -> Result<String> {
    to_string_any_pretty(document, &Any::Object(root), indent)
}

/// Serializes a bare value rather than an object.
///
/// Upstream's `write_to_string` takes anything its `AnyDictionary` can hold,
/// not only objects: its own tests serialize a list of markers and a bare
/// boolean to compare them. Object handles in `value` are resolved against
/// `document`.
///
/// # Errors
///
/// As [`to_string`].
pub fn to_string_any_pretty(document: &Document, value: &Any, indent: usize) -> Result<String> {
    let options = WriteOptions {
        indent: Some(indent),
        ..WriteOptions::default()
    };
    to_string_with(document, value, &options)
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

    crate::dtoa::dtoa(value)
}

/// Escapes a string as a JSON string literal.
///
/// Upstream hands RapidJSON a C string, so a string with a NUL in it is
/// written only as far as the NUL. AAF files carry such strings — Avid ends
/// its effect IDs with one — and are written here as upstream writes them.
fn format_string(value: &str) -> String {
    let end = value.find('\0').unwrap_or(value.len());
    crate::json::escape(&value[..end])
}

struct Writer<'a> {
    document: &'a Document,
    out: String,
    /// `None` for compact output.
    indent: Option<usize>,
    level: usize,
    targets: &'a SchemaVersionMap,
    /// The objects being written, from the outermost in: meeting one again
    /// is a cycle.
    in_progress: HashSet<NodeId>,
}

/// Tracks whether a separator is needed before the next entry.
struct Nesting {
    wrote_any: bool,
}

impl<'a> Writer<'a> {
    fn new(
        document: &'a Document,
        options: &'a WriteOptions,
        in_progress: HashSet<NodeId>,
    ) -> Self {
        Self {
            document,
            out: String::new(),
            indent: options.indent,
            level: 0,
            targets: &options.schema_version_targets,
            in_progress,
        }
    }

    fn newline(&mut self) {
        let Some(indent) = self.indent else {
            return;
        };
        self.out.push('\n');
        for _ in 0..(self.level * indent) {
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
        self.out
            .push_str(if self.indent.is_some() { ": " } else { ":" });
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

    /// Writes the fields every named object shares: a subclass's own
    /// fields, then metadata, then name.
    ///
    /// Upstream writes an object's dynamic fields before anything its class
    /// adds, so a subclass's fields come first, straight after the schema.
    fn write_base(&mut self, nesting: &mut Nesting, base: &crate::schema::Base) -> Result<()> {
        if let Some(fields) = base.extension_fields() {
            for (name, entry) in fields {
                self.key(nesting, name);
                self.write_any(entry)?;
            }
        }
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

        if !self.in_progress.insert(id) {
            return Err(Error::ObjectCycle {
                schema: node.schema_name().to_string(),
            });
        }
        let result = self.write_node_body(id, node);
        self.in_progress.remove(&id);
        result
    }

    /// Whether `node` is to be written at an older version than its own.
    ///
    /// An unknown schema never is: upstream looks its target up under the
    /// name `UnknownSchema`, which a caller has no reason to name.
    fn target_for(&self, node: &Node) -> Option<u32> {
        if self.targets.is_empty() || matches!(node, Node::Unknown(_)) {
            return None;
        }
        self.targets
            .get(node.schema_name())
            .copied()
            .filter(|target| *target < node.schema_version())
    }

    /// Writes an object downgraded to the versions targeted.
    fn write_downgraded(&mut self, id: NodeId) -> Result<()> {
        // Upstream writes the object into a dictionary first and downgrades
        // that; the dictionary is built here by writing the object as it is
        // now and reading the text back as plain data.
        let plain_options = WriteOptions {
            indent: None,
            schema_version_targets: SchemaVersionMap::new(),
        };
        let mut plain = Writer::new(self.document, &plain_options, self.in_progress.clone());
        plain.write_node_body(id, self.document.try_get(id)?)?;
        let mut value = plain_any(&crate::json::parse(&plain.out)?);
        downgrade(&mut value, self.targets)?;
        self.write_any(&value)
    }

    fn write_node_body(&mut self, id: NodeId, node: &Node) -> Result<()> {
        if self.target_for(node).is_some() {
            return self.write_downgraded(id);
        }

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
            Node::Dynamic(dynamic) => {
                // Upstream writes an object's dynamic fields first, then
                // whatever its class adds: here, a name and metadata.
                for (name, entry) in &dynamic.fields {
                    self.key(&mut nesting, name);
                    self.write_any(entry)?;
                }
                if let Some(base) = &dynamic.base {
                    self.write_base(&mut nesting, base)?;
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
            // Upstream's base classes. Each writes exactly what its C++
            // `write_to` writes, which is its parent's fields and then its
            // own; `SerializableObject` has none at all beyond the schema
            // label every object carries.
            Node::SerializableObject => {}
            Node::SerializableObjectWithMetadata(base) => {
                self.write_base(&mut nesting, base)?;
            }
            Node::Composable(composable) => {
                self.write_base(&mut nesting, &composable.base)?;
            }
            Node::Composition(composition) => {
                self.write_item(&mut nesting, &composition.item)?;
                self.write_node_list(&mut nesting, "children", &composition.children)?;
            }
            Node::MediaReference(media) => {
                self.write_media(&mut nesting, media)?;
            }
        }

        self.end_object(nesting);
        Ok(())
    }
}

/// Turns parsed JSON into the self-contained form downgrade functions see:
/// every object, value types included, a dictionary.
fn plain_any(value: &Value) -> Any {
    match value {
        Value::Null => Any::Null,
        Value::Bool(inner) => Any::Bool(*inner),
        Value::Number(Number::Int(inner)) => Any::Int(*inner),
        Value::Number(Number::UInt(inner)) => Any::UInt(*inner),
        Value::Number(Number::Double(inner)) => Any::Double(*inner),
        Value::String(inner) => Any::String(inner.clone()),
        Value::Array(items) => Any::Vector(items.iter().map(plain_any).collect()),
        Value::Object(entries) => Any::Dictionary(
            entries
                .iter()
                .map(|(key, entry)| (key.clone(), plain_any(entry)))
                .collect(),
        ),
    }
}

/// Downgrades every object in `value` that has a lower target, innermost
/// first, as upstream's cloning encoder does as it closes each one.
fn downgrade(value: &mut Any, targets: &SchemaVersionMap) -> Result<()> {
    match value {
        Any::Vector(items) => {
            for item in items {
                downgrade(item, targets)?;
            }
            Ok(())
        }
        Any::Dictionary(entries) => {
            for entry in entries.values_mut() {
                downgrade(entry, targets)?;
            }
            downgrade_one(entries, targets)
        }
        _ => Ok(()),
    }
}

/// Downgrades one object, in dictionary form, if its schema has a lower
/// target.
fn downgrade_one(entries: &mut AnyDictionary, targets: &SchemaVersionMap) -> Result<()> {
    let Some((name, version)) = entries
        .get("OTIO_SCHEMA")
        .and_then(Any::as_str)
        .and_then(|schema| schema.rsplit_once('.'))
        .and_then(|(name, version)| Some((name.to_string(), version.parse::<u32>().ok()?)))
    else {
        return Ok(());
    };
    let Some(&target) = targets.get(&name) else {
        return Ok(());
    };
    if version <= target {
        return Ok(());
    }
    for function in crate::registry::downgrades(&name, version, target)? {
        function(entries)?;
    }
    entries.insert(
        "OTIO_SCHEMA".to_string(),
        Any::String(format!("{name}.{target}")),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_f64;

    /// Which of the two forms a number is written in, against upstream.
    ///
    /// Every expectation here was produced by asking OpenTimelineIO 0.18.1 to
    /// write the value and reading back what it wrote, rather than by reading
    /// RapidJSON's source. The interesting cases are the two boundaries: the
    /// decimal point may sit up to twenty-one places to the right of the
    /// digits and six to the left before an exponent appears.
    #[test]
    fn writes_a_number_the_way_upstream_writes_it() {
        // A whole number keeps the fractional part OTIO's schema implies,
        // however long it is.
        assert_eq!(format_f64(24.0), "24.0");
        assert_eq!(format_f64(240_000.0), "240000.0");
        assert_eq!(format_f64(4_147_200_000.0), "4147200000.0");
        assert_eq!(format_f64(1e20), "100000000000000000000.0");

        // One place further and upstream switches, with no sign on the
        // exponent and no padding.
        assert_eq!(format_f64(1e21), "1e21");
        assert_eq!(format_f64(1e22), "1e22");
        assert_eq!(format_f64(1.5e300), "1.5e300");

        // The same boundary at the small end.
        assert_eq!(format_f64(0.1), "0.1");
        assert_eq!(format_f64(1e-6), "0.000001");
        assert_eq!(format_f64(1e-7), "1e-7");
        assert_eq!(format_f64(1e-8), "1e-8");

        // Zero has no exponent to switch on.
        assert_eq!(format_f64(0.0), "0.0");
        assert_eq!(format_f64(-0.0), "-0.0");
        assert_eq!(format_f64(-240_000.0), "-240000.0");

        // Not JSON, and not valid to read back, but what upstream writes.
        assert_eq!(format_f64(f64::NAN), "NaN");
        assert_eq!(format_f64(f64::INFINITY), "Infinity");
        assert_eq!(format_f64(f64::NEG_INFINITY), "-Infinity");
    }
}

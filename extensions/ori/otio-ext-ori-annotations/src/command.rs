//! Annotation commands: reading ORI's paint, text and shape objects, and
//! building new ones.
//!
//! A command in a document is an object of one of ORI's schemas, held in an
//! annotation clip's `annotation_commands`. [`Command::read`] gives a typed
//! copy of one; [`Command::insert`] adds a new one to a document and returns
//! its handle, for [`annotations`](crate::annotations) to put in place.
//!
//! Each typed command keeps every field it does not name in `other`, and
//! writes it back, so a command read and inserted again comes out with the
//! same fields. The constructors give each field the default ORI's own
//! Python class gives it, so a new command is written exactly as ORI writes
//! one.
//!
//! A stroke is three or more commands sharing one `uuid`: a [`PaintStart`],
//! one or more [`PaintPoint`] batches and a [`PaintEnd`]. [`stroke`] builds
//! the usual three.

use otio_core::schema::{DynamicObject, Node};
use otio_core::{Any, AnyDictionary, Document, NodeId};

use crate::schema::{self, VERSION};
use crate::{Error, Result};

/// One annotation command.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// `PaintStart.1`: opens a stroke.
    PaintStart(PaintStart),
    /// `PaintPoint.1`: a batch of points on an open stroke.
    PaintPoint(PaintPoint),
    /// `PaintEnd.1`: closes a stroke.
    PaintEnd(PaintEnd),
    /// `TextAnnotation.1`: text placed on the image.
    Text(TextAnnotation),
    /// `EllipseAnnotation.1`: an ellipse within a box.
    Ellipse(Shape),
    /// `RectangleAnnotation.1`: a rectangle.
    Rectangle(Shape),
    /// `ArrowAnnotation.1`: an arrow.
    Arrow(Arrow),
    /// An object in a command list that is not one of ORI's commands, kept
    /// as it is.
    Other(Other),
}

/// Opens a stroke: `PaintStart.1`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintStart {
    /// The stroke's id, shared by its points and its end. Written `null`
    /// when empty.
    pub uuid: String,
    /// Red, green, blue and alpha, each 0 to 1. ORI's default is `null`.
    pub rgba: Option<[f64; 4]>,
    /// The brush profile, such as `circle` or `gauss`.
    pub brush: String,
    /// What the stroke does: `color` paints, `erase` erases. ORI calls this
    /// field `type`.
    pub kind: String,
    /// Which source the stroke is on, when a frame shows several.
    pub source_index: i64,
    /// Who drew it, for display only.
    pub friendly_name: Option<String>,
    /// A stable, opaque id of who drew it.
    pub participant_hash: Option<String>,
    /// Whether it is shown.
    pub visible: bool,
    /// When it was drawn, in ISO 8601.
    pub timestamp: Option<String>,
    /// Every other field: ORI's `ghost`, `ghost_before`, `ghost_after`,
    /// `hold` and `layer_range`, and anything a later ORI adds.
    pub other: AnyDictionary,
}

impl PaintStart {
    /// A stroke opening with ORI's defaults: a `circle` brush painting
    /// `color`, visible, on source 0, with no colour or owner yet.
    #[must_use]
    pub fn new(uuid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            rgba: None,
            brush: "circle".to_string(),
            kind: "color".to_string(),
            source_index: 0,
            friendly_name: None,
            participant_hash: None,
            visible: true,
            timestamp: None,
            other: nulls(&[
                "ghost",
                "ghost_after",
                "ghost_before",
                "hold",
                "layer_range",
            ]),
        }
    }
}

/// The points of a stroke: ORI's `PaintVertices.1`, four parallel arrays.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Vertices {
    /// Horizontal positions, in ORI's coordinates.
    pub x: Vec<f64>,
    /// Vertical positions, in ORI's coordinates.
    pub y: Vec<f64>,
    /// The brush's size at each point, as a fraction of the image height.
    pub size: Vec<f64>,
    /// The brush's opacity at each point; empty for fully opaque.
    pub alpha: Vec<f64>,
    /// Any other field.
    pub other: AnyDictionary,
}

impl Vertices {
    /// Points given as `(x, y)` in ORI's coordinates, all drawn at one size.
    #[must_use]
    pub fn with_size(points: &[(f64, f64)], size: f64) -> Self {
        Self {
            x: points.iter().map(|point| point.0).collect(),
            y: points.iter().map(|point| point.1).collect(),
            size: vec![size; points.len()],
            alpha: Vec::new(),
            other: AnyDictionary::new(),
        }
    }

    /// How many points there are: the length of `x`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }
}

/// A batch of points on an open stroke: `PaintPoint.1`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintPoint {
    /// The stroke's id.
    pub uuid: String,
    /// Which source the stroke is on.
    pub source_index: i64,
    /// The points; ORI's default is `null`.
    pub points: Option<Vertices>,
    /// When the batch was drawn.
    pub timestamp: Option<String>,
    /// Every other field: ORI's `layer_range`, and anything later.
    pub other: AnyDictionary,
}

impl PaintPoint {
    /// A batch of points on stroke `uuid`, on source 0.
    #[must_use]
    pub fn new(uuid: impl Into<String>, points: Vertices) -> Self {
        Self {
            uuid: uuid.into(),
            source_index: 0,
            points: Some(points),
            timestamp: None,
            other: nulls(&["layer_range"]),
        }
    }
}

/// Closes a stroke: `PaintEnd.1`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintEnd {
    /// The stroke's id.
    pub uuid: String,
    /// When the stroke ended.
    pub timestamp: Option<String>,
    /// Every other field: ORI's `points`, which ORI leaves `null`.
    pub other: AnyDictionary,
}

impl PaintEnd {
    /// The end of stroke `uuid`.
    #[must_use]
    pub fn new(uuid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            timestamp: None,
            other: nulls(&["points"]),
        }
    }
}

/// Text placed on the image: `TextAnnotation.1`.
///
/// ORI leaves every field `null` until it is set; these do too.
#[derive(Debug, Clone, PartialEq)]
pub struct TextAnnotation {
    /// Its id.
    pub uuid: String,
    /// The text.
    pub text: Option<String>,
    /// Where it is, in ORI's coordinates.
    pub position: Option<[f64; 2]>,
    /// Red, green, blue and alpha.
    pub rgba: Option<[f64; 4]>,
    /// The font's name.
    pub font: Option<String>,
    /// The font's size.
    pub font_size: Option<f64>,
    /// A scale applied on top of the size.
    pub scale: Option<f64>,
    /// Rotation, in degrees.
    pub rotation: Option<f64>,
    /// Letter spacing.
    pub spacing: Option<f64>,
    /// Who wrote it, for display only.
    pub friendly_name: Option<String>,
    /// When it was written.
    pub timestamp: Option<String>,
    /// Any other field.
    pub other: AnyDictionary,
}

impl TextAnnotation {
    /// Text with only an id, as ORI starts one.
    #[must_use]
    pub fn new(uuid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            text: None,
            position: None,
            rgba: None,
            font: None,
            font_size: None,
            scale: None,
            rotation: None,
            spacing: None,
            friendly_name: None,
            timestamp: None,
            other: AnyDictionary::new(),
        }
    }
}

/// An ellipse or a rectangle: `EllipseAnnotation.1` or
/// `RectangleAnnotation.1`, which have the same fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// Its id.
    pub uuid: String,
    /// One corner of its box, in ORI's coordinates.
    pub min: [f64; 2],
    /// The opposite corner.
    pub max: [f64; 2],
    /// The outline's colour.
    pub rgba: [f64; 4],
    /// The fill's colour; transparent for none.
    pub inner_rgba: [f64; 4],
    /// The outline's width.
    pub size: f64,
    /// Whether it is shown.
    pub visible: bool,
    /// When it was drawn.
    pub timestamp: Option<String>,
    /// Any other field.
    pub other: AnyDictionary,
}

impl Shape {
    /// A shape with ORI's defaults: a white outline of width 1, no fill, a
    /// box with both corners at the origin.
    #[must_use]
    pub fn new(uuid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            min: [0.0, 0.0],
            max: [0.0, 0.0],
            rgba: [1.0, 1.0, 1.0, 1.0],
            inner_rgba: [0.0, 0.0, 0.0, 0.0],
            size: 1.0,
            visible: true,
            timestamp: None,
            other: AnyDictionary::new(),
        }
    }
}

/// An arrow: `ArrowAnnotation.1`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    /// Its id.
    pub uuid: String,
    /// Its tail, in ORI's coordinates.
    pub start: [f64; 2],
    /// Its head.
    pub end: [f64; 2],
    /// Its colour.
    pub rgba: [f64; 4],
    /// Its width.
    pub size: f64,
    /// Whether it is shown.
    pub visible: bool,
    /// When it was drawn.
    pub timestamp: Option<String>,
    /// Any other field.
    pub other: AnyDictionary,
}

impl Arrow {
    /// An arrow with ORI's defaults: white, width 1, both ends at the origin.
    #[must_use]
    pub fn new(uuid: impl Into<String>) -> Self {
        Self {
            uuid: uuid.into(),
            start: [0.0, 0.0],
            end: [0.0, 0.0],
            rgba: [1.0, 1.0, 1.0, 1.0],
            size: 1.0,
            visible: true,
            timestamp: None,
            other: AnyDictionary::new(),
        }
    }
}

/// An object in a command list that is not one of ORI's commands.
#[derive(Debug, Clone, PartialEq)]
pub struct Other {
    /// Its schema name, without the version.
    pub schema_name: String,
    /// Its schema version.
    pub schema_version: u32,
    /// Its fields. Objects inside are handles into the document it was read
    /// from.
    pub fields: AnyDictionary,
}

/// The three commands of a stroke drawn in one go: `start`, then `points`
/// as one batch, then its end, all with `start`'s uuid and timestamp.
#[must_use]
pub fn stroke(start: PaintStart, points: Vertices) -> [Command; 3] {
    let points = PaintPoint {
        source_index: start.source_index,
        timestamp: start.timestamp.clone(),
        ..PaintPoint::new(start.uuid.clone(), points)
    };
    let end = PaintEnd {
        timestamp: start.timestamp.clone(),
        ..PaintEnd::new(start.uuid.clone())
    };
    [
        Command::PaintStart(start),
        Command::PaintPoint(points),
        Command::PaintEnd(end),
    ]
}

impl Command {
    /// The command's `uuid`, which a stroke's commands share. `None` for one
    /// with no uuid, or an empty one.
    #[must_use]
    pub fn uuid(&self) -> Option<&str> {
        let uuid = match self {
            Self::PaintStart(command) => command.uuid.as_str(),
            Self::PaintPoint(command) => command.uuid.as_str(),
            Self::PaintEnd(command) => command.uuid.as_str(),
            Self::Text(command) => command.uuid.as_str(),
            Self::Ellipse(command) | Self::Rectangle(command) => command.uuid.as_str(),
            Self::Arrow(command) => command.uuid.as_str(),
            Self::Other(other) => other.fields.get("uuid").and_then(Any::as_str).unwrap_or(""),
        };
        (!uuid.is_empty()).then_some(uuid)
    }

    /// The schema name it is written with, without the version, such as
    /// `PaintPoint`.
    #[must_use]
    pub fn schema_name(&self) -> &str {
        match self {
            Self::PaintStart(_) => schema::PAINT_START,
            Self::PaintPoint(_) => schema::PAINT_POINT,
            Self::PaintEnd(_) => schema::PAINT_END,
            Self::Text(_) => schema::TEXT_ANNOTATION,
            Self::Ellipse(_) => schema::ELLIPSE_ANNOTATION,
            Self::Rectangle(_) => schema::RECTANGLE_ANNOTATION,
            Self::Arrow(_) => schema::ARROW_ANNOTATION,
            Self::Other(other) => &other.schema_name,
        }
    }

    /// Reads the command `id` names.
    ///
    /// # Errors
    ///
    /// [`Error::NotEnabled`] if the extension is off;
    /// [`otio_core::Error::StaleHandle`] if `id` is not in the document;
    /// [`Error::BadField`] if one of ORI's fields holds the wrong type.
    pub fn read(document: &Document, id: NodeId) -> Result<Self> {
        crate::require_enabled()?;
        let (name, version, fields) = match document.try_get(id)? {
            Node::Dynamic(dynamic) => (
                dynamic.schema_name.as_str(),
                dynamic.schema_version,
                &dynamic.fields,
            ),
            Node::Unknown(unknown) => (
                unknown.original_schema_name.as_str(),
                unknown.original_schema_version,
                &unknown.data,
            ),
            node => {
                return Err(Error::WrongKind {
                    expected: "an annotation command",
                    found: node.schema_name().to_string(),
                });
            }
        };
        let mut fields = Fields::new(name, fields.clone());
        Ok(match name {
            schema::PAINT_START => Self::PaintStart(PaintStart {
                uuid: fields.uuid()?,
                rgba: fields.optional_array("rgba")?,
                brush: fields.string_or("brush", "")?,
                kind: fields.string_or("type", "")?,
                source_index: fields.int_or("source_index", 0)?,
                friendly_name: fields.optional_string("friendly_name")?,
                participant_hash: fields.optional_string("participant_hash")?,
                visible: fields.bool_or("visible", true)?,
                timestamp: fields.optional_string("timestamp")?,
                other: fields.rest(),
            }),
            schema::PAINT_POINT => Self::PaintPoint(PaintPoint {
                uuid: fields.uuid()?,
                source_index: fields.int_or("source_index", 0)?,
                points: match fields.take("points") {
                    None | Some(Any::Null) => None,
                    Some(Any::Object(vertices)) => Some(read_vertices(document, vertices)?),
                    Some(_) => return Err(fields.bad("points", "a PaintVertices object")),
                },
                timestamp: fields.optional_string("timestamp")?,
                other: fields.rest(),
            }),
            schema::PAINT_END => Self::PaintEnd(PaintEnd {
                uuid: fields.uuid()?,
                timestamp: fields.optional_string("timestamp")?,
                other: fields.rest(),
            }),
            schema::TEXT_ANNOTATION => Self::Text(TextAnnotation {
                uuid: fields.uuid()?,
                text: fields.optional_string("text")?,
                position: fields.optional_array("position")?,
                rgba: fields.optional_array("rgba")?,
                font: fields.optional_string("font")?,
                font_size: fields.optional_number("font_size")?,
                scale: fields.optional_number("scale")?,
                rotation: fields.optional_number("rotation")?,
                spacing: fields.optional_number("spacing")?,
                friendly_name: fields.optional_string("friendly_name")?,
                timestamp: fields.optional_string("timestamp")?,
                other: fields.rest(),
            }),
            schema::ELLIPSE_ANNOTATION | schema::RECTANGLE_ANNOTATION => {
                let shape = Shape {
                    uuid: fields.uuid()?,
                    min: fields.array_or("min", [0.0, 0.0])?,
                    max: fields.array_or("max", [0.0, 0.0])?,
                    rgba: fields.array_or("rgba", [1.0, 1.0, 1.0, 1.0])?,
                    inner_rgba: fields.array_or("inner_rgba", [0.0; 4])?,
                    size: fields.number_or("size", 1.0)?,
                    visible: fields.bool_or("visible", true)?,
                    timestamp: fields.optional_string("timestamp")?,
                    other: fields.rest(),
                };
                if name == schema::ELLIPSE_ANNOTATION {
                    Self::Ellipse(shape)
                } else {
                    Self::Rectangle(shape)
                }
            }
            schema::ARROW_ANNOTATION => Self::Arrow(Arrow {
                uuid: fields.uuid()?,
                start: fields.array_or("start", [0.0, 0.0])?,
                end: fields.array_or("end", [0.0, 0.0])?,
                rgba: fields.array_or("rgba", [1.0, 1.0, 1.0, 1.0])?,
                size: fields.number_or("size", 1.0)?,
                visible: fields.bool_or("visible", true)?,
                timestamp: fields.optional_string("timestamp")?,
                other: fields.rest(),
            }),
            _ => Self::Other(Other {
                schema_name: name.to_string(),
                schema_version: version,
                fields: fields.rest(),
            }),
        })
    }

    /// Adds the command to `document` as a new object with no owner, and
    /// returns its handle.
    ///
    /// Objects held in `other` fields are copied, so the command must have
    /// been read from, or built for, this document.
    ///
    /// # Errors
    ///
    /// [`Error::NotEnabled`] if the extension is off, and
    /// [`otio_core::Error::StaleHandle`] if an object in `other` is not in
    /// the document.
    pub fn insert(&self, document: &mut Document) -> Result<NodeId> {
        crate::require_enabled()?;
        let (name, version, mut fields) = match self {
            Self::Other(other) => (
                other.schema_name.clone(),
                other.schema_version,
                copy_objects(document, &other.fields)?,
            ),
            _ => (
                self.schema_name().to_string(),
                VERSION,
                self.fields(document)?,
            ),
        };
        if let Some(uuid) = self.uuid() {
            fields.insert("uuid".to_string(), Any::String(uuid.to_string()));
        } else if !matches!(self, Self::Other(_)) {
            fields.insert("uuid".to_string(), Any::Null);
        }
        Ok(document.insert(object(name, version, fields)))
    }

    /// Every field but `uuid`, as written.
    fn fields(&self, document: &mut Document) -> Result<AnyDictionary> {
        let mut fields;
        match self {
            Self::PaintStart(command) => {
                fields = copy_objects(document, &command.other)?;
                put(&mut fields, "rgba", command.rgba.map(array));
                put(&mut fields, "brush", command.brush.clone());
                put(&mut fields, "type", command.kind.clone());
                put(&mut fields, "source_index", command.source_index);
                put(&mut fields, "friendly_name", command.friendly_name.clone());
                put(
                    &mut fields,
                    "participant_hash",
                    command.participant_hash.clone(),
                );
                put(&mut fields, "visible", command.visible);
                put(&mut fields, "timestamp", command.timestamp.clone());
            }
            Self::PaintPoint(command) => {
                fields = copy_objects(document, &command.other)?;
                put(&mut fields, "source_index", command.source_index);
                let points = match &command.points {
                    Some(vertices) => Any::Object(insert_vertices(document, vertices)?),
                    None => Any::Null,
                };
                put(&mut fields, "points", points);
                put(&mut fields, "timestamp", command.timestamp.clone());
            }
            Self::PaintEnd(command) => {
                fields = copy_objects(document, &command.other)?;
                put(&mut fields, "timestamp", command.timestamp.clone());
            }
            Self::Text(command) => {
                fields = copy_objects(document, &command.other)?;
                put(&mut fields, "text", command.text.clone());
                put(&mut fields, "position", command.position.map(array));
                put(&mut fields, "rgba", command.rgba.map(array));
                put(&mut fields, "font", command.font.clone());
                put(&mut fields, "font_size", command.font_size);
                put(&mut fields, "scale", command.scale);
                put(&mut fields, "rotation", command.rotation);
                put(&mut fields, "spacing", command.spacing);
                put(&mut fields, "friendly_name", command.friendly_name.clone());
                put(&mut fields, "timestamp", command.timestamp.clone());
            }
            Self::Ellipse(command) | Self::Rectangle(command) => {
                fields = copy_objects(document, &command.other)?;
                put(&mut fields, "min", array(command.min));
                put(&mut fields, "max", array(command.max));
                put(&mut fields, "rgba", array(command.rgba));
                put(&mut fields, "inner_rgba", array(command.inner_rgba));
                put(&mut fields, "size", command.size);
                put(&mut fields, "visible", command.visible);
                put(&mut fields, "timestamp", command.timestamp.clone());
            }
            Self::Arrow(command) => {
                fields = copy_objects(document, &command.other)?;
                put(&mut fields, "start", array(command.start));
                put(&mut fields, "end", array(command.end));
                put(&mut fields, "rgba", array(command.rgba));
                put(&mut fields, "size", command.size);
                put(&mut fields, "visible", command.visible);
                put(&mut fields, "timestamp", command.timestamp.clone());
            }
            Self::Other(other) => fields = copy_objects(document, &other.fields)?,
        }
        Ok(fields)
    }
}

/// The schema name of the command `id` names, without the version, if it
/// is an object at all.
#[must_use]
pub fn schema_name_of(document: &Document, id: NodeId) -> Option<&str> {
    document.get(id).map(Node::schema_name)
}

/// The `uuid` of the command `id` names, without reading the rest of it.
#[must_use]
pub fn uuid_of(document: &Document, id: NodeId) -> Option<&str> {
    let fields = match document.get(id)? {
        Node::Dynamic(dynamic) => &dynamic.fields,
        Node::Unknown(unknown) => &unknown.data,
        _ => return None,
    };
    fields
        .get("uuid")
        .and_then(Any::as_str)
        .filter(|uuid| !uuid.is_empty())
}

/// The current time in ISO 8601, in UTC with microseconds, for a command's
/// `timestamp`. ORI writes local time with no zone; a reader treats both as
/// opaque text.
#[must_use]
pub fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs();
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:06}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60,
        now.subsec_micros()
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01, by Howard
/// Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// An ORI object: a dynamic one when its schema is registered, which it is
/// while the extension is on, and an unknown one otherwise.
fn object(schema_name: String, schema_version: u32, fields: AnyDictionary) -> Node {
    if otio_core::registry::schema_kind(&schema_name).is_some() {
        Node::Dynamic(DynamicObject {
            schema_name,
            schema_version,
            base: None,
            fields,
        })
    } else {
        Node::Unknown(otio_core::schema::UnknownSchema {
            original_schema_name: schema_name,
            original_schema_version: schema_version,
            data: fields,
        })
    }
}

fn read_vertices(document: &Document, id: NodeId) -> Result<Vertices> {
    let (name, fields) = match document.try_get(id)? {
        Node::Dynamic(dynamic) => (dynamic.schema_name.as_str(), &dynamic.fields),
        Node::Unknown(unknown) => (unknown.original_schema_name.as_str(), &unknown.data),
        node => {
            return Err(Error::WrongKind {
                expected: "PaintVertices",
                found: node.schema_name().to_string(),
            });
        }
    };
    if name != schema::PAINT_VERTICES {
        return Err(Error::WrongKind {
            expected: "PaintVertices",
            found: name.to_string(),
        });
    }
    let mut fields = Fields::new(name, fields.clone());
    Ok(Vertices {
        x: fields.numbers("x")?,
        y: fields.numbers("y")?,
        size: fields.numbers("size")?,
        alpha: fields.numbers("alpha")?,
        other: fields.rest(),
    })
}

fn insert_vertices(document: &mut Document, vertices: &Vertices) -> Result<NodeId> {
    let mut fields = copy_objects(document, &vertices.other)?;
    for (key, values) in [
        ("x", &vertices.x),
        ("y", &vertices.y),
        ("size", &vertices.size),
        ("alpha", &vertices.alpha),
    ] {
        put(
            &mut fields,
            key,
            Any::Vector(values.iter().copied().map(Any::Double).collect()),
        );
    }
    Ok(document.insert(object(schema::PAINT_VERTICES.to_string(), VERSION, fields)))
}

/// `fields` with every object inside copied, so the copy owns its own.
fn copy_objects(document: &mut Document, fields: &AnyDictionary) -> Result<AnyDictionary> {
    let mut copy = fields.clone();
    let mut failure = None;
    for value in copy.values_mut() {
        value.visit_objects_mut(&mut |id: &mut NodeId| {
            if failure.is_some() {
                return;
            }
            match document.deep_clone(*id) {
                Ok(cloned) => *id = cloned,
                Err(error) => failure = Some(error),
            }
        });
    }
    match failure {
        Some(error) => Err(error.into()),
        None => Ok(copy),
    }
}

/// Removes an ORI object and every object it holds from the document.
pub(crate) fn drop_object(document: &mut Document, id: NodeId) {
    let Some(mut node) = document.remove(id) else {
        return;
    };
    let mut held = Vec::new();
    node.visit_held_objects_mut(&mut |id: &mut NodeId| held.push(*id));
    for id in held {
        drop_object(document, id);
    }
}

fn nulls(keys: &[&str]) -> AnyDictionary {
    keys.iter()
        .map(|key| ((*key).to_string(), Any::Null))
        .collect()
}

fn array<const N: usize>(values: [f64; N]) -> Any {
    Any::Vector(values.into_iter().map(Any::Double).collect())
}

fn put(fields: &mut AnyDictionary, key: &str, value: impl Into<Value>) {
    fields.insert(key.to_string(), value.into().0);
}

/// A field value, so [`put`] can take plain Rust values and `Option`s of
/// them, writing `None` as `null`.
struct Value(Any);

impl From<Any> for Value {
    fn from(value: Any) -> Self {
        Self(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self(Any::String(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self(Any::Double(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self(Any::Int(value))
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self(Any::Bool(value))
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self(Any::Null), Into::into)
    }
}

/// A command's fields, taken one by one as they are read, so what is left
/// is the command's `other`.
struct Fields<'a> {
    schema: &'a str,
    fields: AnyDictionary,
}

impl<'a> Fields<'a> {
    const fn new(schema: &'a str, fields: AnyDictionary) -> Self {
        Self { schema, fields }
    }

    fn take(&mut self, key: &str) -> Option<Any> {
        self.fields.remove(key)
    }

    fn rest(&mut self) -> AnyDictionary {
        std::mem::take(&mut self.fields)
    }

    fn bad(&self, field: &'static str, expected: &'static str) -> Error {
        Error::BadField {
            schema: self.schema.to_string(),
            field,
            expected,
        }
    }

    fn uuid(&mut self) -> Result<String> {
        Ok(self.optional_string("uuid")?.unwrap_or_default())
    }

    fn optional_string(&mut self, key: &'static str) -> Result<Option<String>> {
        match self.take(key) {
            None | Some(Any::Null) => Ok(None),
            Some(Any::String(value)) => Ok(Some(value)),
            Some(_) => Err(self.bad(key, "text")),
        }
    }

    fn string_or(&mut self, key: &'static str, default: &str) -> Result<String> {
        match self.take(key) {
            None => Ok(default.to_string()),
            Some(Any::String(value)) => Ok(value),
            Some(_) => Err(self.bad(key, "text")),
        }
    }

    fn int_or(&mut self, key: &'static str, default: i64) -> Result<i64> {
        match self.take(key) {
            None => Ok(default),
            Some(Any::Int(value)) => Ok(value),
            Some(_) => Err(self.bad(key, "a whole number")),
        }
    }

    fn bool_or(&mut self, key: &'static str, default: bool) -> Result<bool> {
        match self.take(key) {
            None => Ok(default),
            Some(Any::Bool(value)) => Ok(value),
            Some(_) => Err(self.bad(key, "true or false")),
        }
    }

    fn optional_number(&mut self, key: &'static str) -> Result<Option<f64>> {
        match self.take(key) {
            None | Some(Any::Null) => Ok(None),
            Some(value) => number(&value)
                .map(Some)
                .ok_or_else(|| self.bad(key, "a number")),
        }
    }

    fn number_or(&mut self, key: &'static str, default: f64) -> Result<f64> {
        match self.take(key) {
            None => Ok(default),
            Some(value) => number(&value).ok_or_else(|| self.bad(key, "a number")),
        }
    }

    fn numbers(&mut self, key: &'static str) -> Result<Vec<f64>> {
        match self.take(key) {
            None | Some(Any::Null) => Ok(Vec::new()),
            Some(Any::Vector(values)) => values
                .iter()
                .map(number)
                .collect::<Option<Vec<f64>>>()
                .ok_or_else(|| self.bad(key, "a list of numbers")),
            Some(_) => Err(self.bad(key, "a list of numbers")),
        }
    }

    fn optional_array<const N: usize>(&mut self, key: &'static str) -> Result<Option<[f64; N]>> {
        match self.take(key) {
            None | Some(Any::Null) => Ok(None),
            Some(value) => fixed(&value)
                .map(Some)
                .ok_or_else(|| self.bad(key, length(N))),
        }
    }

    fn array_or<const N: usize>(
        &mut self,
        key: &'static str,
        default: [f64; N],
    ) -> Result<[f64; N]> {
        match self.take(key) {
            None => Ok(default),
            Some(value) => fixed(&value).ok_or_else(|| self.bad(key, length(N))),
        }
    }
}

const fn length(count: usize) -> &'static str {
    match count {
        2 => "two numbers",
        4 => "four numbers",
        _ => "a list of numbers",
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "ORI's numbers are coordinates and colours, far below 2^53"
)]
fn number(value: &Any) -> Option<f64> {
    match value {
        Any::Double(value) => Some(*value),
        Any::Int(value) => Some(*value as f64),
        Any::UInt(value) => Some(*value as f64),
        _ => None,
    }
}

fn fixed<const N: usize>(value: &Any) -> Option<[f64; N]> {
    let Any::Vector(values) = value else {
        return None;
    };
    if values.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, value) in out.iter_mut().zip(values) {
        *slot = number(value)?;
    }
    Some(out)
}

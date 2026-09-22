//! The dynamically typed values OTIO stores in `metadata`.
//!
//! Upstream calls these `AnyDictionary` and `AnyVector`, built on `std::any`.
//! Adapters lean on them heavily to carry format-specific data that the core
//! schema has no field for, so a port that cannot round-trip them loses data
//! on every file it touches.

use std::collections::BTreeMap;

use opentime::{RationalTime, TimeRange, TimeTransform};

use crate::arena::NodeId;

/// A colour, as used for marker and clip tinting in an editorial UI.
///
/// Not for image pixel content: components are sRGB transfer-function encoded
/// and interoperable only to within 1/255.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Color {
    /// Red, nominally in 0..=1.
    pub r: f64,
    /// Green, nominally in 0..=1.
    pub g: f64,
    /// Blue, nominally in 0..=1.
    pub b: f64,
    /// Alpha, nominally in 0..=1.
    pub a: f64,
    /// An optional human-readable name, such as `"RED"`.
    pub name: String,
}

impl Color {
    /// Construct a colour from its components and a name.
    #[must_use]
    pub const fn new(r: f64, g: f64, b: f64, a: f64, name: String) -> Self {
        Self { r, g, b, a, name }
    }

    /// Construct an opaque colour with no name.
    #[must_use]
    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self {
            r,
            g,
            b,
            a: 1.0,
            name: String::new(),
        }
    }

    /// Construct a colour with no name.
    #[must_use]
    pub const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self {
            r,
            g,
            b,
            a,
            name: String::new(),
        }
    }

    /// Whether two colours would look the same, ignoring their names.
    ///
    /// This is upstream's `operator==`, which compares the eight-bit form of
    /// each colour rather than the doubles: two colours a fraction of a
    /// 255th apart are the same colour to a user interface. The name is
    /// deliberately not part of it, so the named `Color::RED` equals an
    /// unnamed red read out of a file.
    #[must_use]
    pub fn looks_like(&self, other: &Self) -> bool {
        self.to_rgba_int_list(8) == other.to_rgba_int_list(8)
    }

    /// The colour as `#rrggbbaa`.
    #[must_use]
    pub fn to_hex(&self) -> String {
        let [r, g, b, a] = self.to_rgba_int_list(8);
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }

    /// The colour's four components at `bit_depth` bits each, truncated.
    #[must_use]
    pub fn to_rgba_int_list(&self, bit_depth: i32) -> [i64; 4] {
        let scale = 2.0_f64.powi(bit_depth) - 1.0;
        let at = |value: f64| (value * scale) as i64;
        [at(self.r), at(self.g), at(self.b), at(self.a)]
    }

    /// The colour packed into one 32-bit integer.
    ///
    /// # Note on a difference from upstream
    ///
    /// Upstream packs blue at bits 16-23 and green at bits 8-15 here, and
    /// unpacks them the other way round in [`Color::from_agbr_int`], so a
    /// round trip through this swaps green and blue. Both are reproduced as
    /// they are: a file or a plugin that went through upstream carries
    /// whatever upstream produced, and `tests/color.rs` pins the behaviour so
    /// that it cannot be quietly "fixed" here.
    #[must_use]
    pub fn to_agbr_integer(&self) -> u32 {
        let [r, g, b, a] = self.to_rgba_int_list(8).map(|value| value as u32);
        (a << 24)
            .wrapping_add(b << 16)
            .wrapping_add(g << 8)
            .wrapping_add(r)
    }

    /// The colour's four components as they are stored.
    #[must_use]
    pub const fn to_rgba_float_list(&self) -> [f64; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Reads a colour from `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    ///
    /// A leading `#` or `0x` is optional. Each component is read as
    /// upstream reads it, with `std::stoi(…, 16)`, so leading whitespace
    /// and a sign are allowed and reading stops at the first character that
    /// is not a hex digit: `#-1-1-1` is a colour, if an odd one.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BadColor`](crate::Error::BadColor) with upstream's
    /// message: `Invalid hex format` if the text is not one of those four
    /// lengths, and `stoi` if a component does not start with a hex digit.
    pub fn from_hex(text: &str) -> crate::Result<Self> {
        let bytes = text.as_bytes();
        let digits = match bytes {
            [b'#', rest @ ..] | [b'0', b'x' | b'X', rest @ ..] => rest,
            _ => bytes,
        };

        // A short form gives each component one digit out of fifteen; a long
        // one gives it two out of 255.
        let (width, scale) = match digits.len() {
            3 | 4 => (1, 15.0),
            6 | 8 => (2, 255.0),
            _ => {
                return Err(crate::Error::BadColor {
                    text: "Invalid hex format".to_string(),
                });
            }
        };
        let component = |index: usize| -> crate::Result<f64> {
            let at = index * width;
            let value = crate::bundle::stoi_hex(&digits[at..at + width]).map_err(|error| {
                crate::Error::BadColor {
                    text: error.to_string(),
                }
            })?;
            Ok(f64::from(value) / scale)
        };
        let (r, g, b) = (component(0)?, component(1)?, component(2)?);
        let alpha = if digits.len() == 4 || digits.len() == 8 {
            component(3)?
        } else {
            1.0
        };
        Ok(Self::rgba(r, g, b, alpha))
    }

    /// Reads a colour from three or four integers at `bit_depth` bits each.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BadColor`](crate::Error::BadColor) if there are not
    /// three or four of them.
    pub fn from_int_list(components: &[i64], bit_depth: i32) -> crate::Result<Self> {
        let scale = 2.0_f64.powi(bit_depth) - 1.0;
        Self::from_float_list(
            &components
                .iter()
                .map(|value| *value as f64 / scale)
                .collect::<Vec<_>>(),
        )
    }

    /// Reads a colour from three or four components.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BadColor`](crate::Error::BadColor) if there are not
    /// three or four of them.
    pub fn from_float_list(components: &[f64]) -> crate::Result<Self> {
        match components {
            [r, g, b] => Ok(Self::rgb(*r, *g, *b)),
            [r, g, b, a] => Ok(Self::rgba(*r, *g, *b, *a)),
            _ => Err(crate::Error::BadColor {
                text: "List must have exactly 3 or 4 elements".to_string(),
            }),
        }
    }

    /// Reads a colour from one packed 32-bit integer.
    ///
    /// See [`Color::to_agbr_integer`] for the green-and-blue swap between the
    /// two directions, which is upstream's and is kept.
    #[must_use]
    pub fn from_agbr_int(agbr: u32) -> Self {
        let at = |shift: u32| f64::from((agbr >> shift) & 0xFF) / 255.0;
        Self::rgba(at(0), at(16), at(8), at(24))
    }
}

/// The named colours upstream defines, which a marker's `color` usually is.
///
/// Each is a function rather than a constant because a [`Color`] carries an
/// owned name, and a `String` cannot be built in a constant.
impl Color {
    /// `#ff00ff`, named `Pink`. The same components as [`Color::magenta`],
    /// which is upstream's doing.
    #[must_use]
    pub fn pink() -> Self {
        Self::named(1.0, 0.0, 1.0, "Pink")
    }

    /// `#ff0000`, named `Red`.
    #[must_use]
    pub fn red() -> Self {
        Self::named(1.0, 0.0, 0.0, "Red")
    }

    /// `#ff8000`, named `Orange`.
    #[must_use]
    pub fn orange() -> Self {
        Self::named(1.0, 0.5, 0.0, "Orange")
    }

    /// `#ffff00`, named `Yellow`.
    #[must_use]
    pub fn yellow() -> Self {
        Self::named(1.0, 1.0, 0.0, "Yellow")
    }

    /// `#00ff00`, named `Green`.
    #[must_use]
    pub fn green() -> Self {
        Self::named(0.0, 1.0, 0.0, "Green")
    }

    /// `#00ffff`, named `Cyan`.
    #[must_use]
    pub fn cyan() -> Self {
        Self::named(0.0, 1.0, 1.0, "Cyan")
    }

    /// `#0000ff`, named `Blue`.
    #[must_use]
    pub fn blue() -> Self {
        Self::named(0.0, 0.0, 1.0, "Blue")
    }

    /// `#800080`, named `Purple`.
    #[must_use]
    pub fn purple() -> Self {
        Self::named(0.5, 0.0, 0.5, "Purple")
    }

    /// `#ff00ff`, named `Magenta`.
    #[must_use]
    pub fn magenta() -> Self {
        Self::named(1.0, 0.0, 1.0, "Magenta")
    }

    /// `#000000`, named `Black`.
    #[must_use]
    pub fn black() -> Self {
        Self::named(0.0, 0.0, 0.0, "Black")
    }

    /// `#ffffff`, named `White`.
    #[must_use]
    pub fn white() -> Self {
        Self::named(1.0, 1.0, 1.0, "White")
    }

    /// Fully transparent black, named `Transparent`.
    #[must_use]
    pub fn transparent() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0, "Transparent".to_string())
    }

    /// An opaque named colour.
    fn named(r: f64, g: f64, b: f64, name: &str) -> Self {
        Self::new(r, g, b, 1.0, name.to_string())
    }
}

/// A two-dimensional point.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V2d {
    /// The horizontal coordinate.
    pub x: f64,
    /// The vertical coordinate.
    pub y: f64,
}

impl V2d {
    /// Construct a point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// The dot product, Imath's `V2d::dot` and `^`.
    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// The z component of the cross product, Imath's `V2d::cross` and `%`.
    #[must_use]
    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    /// The squared length.
    #[must_use]
    pub fn length2(self) -> f64 {
        self.dot(self)
    }

    /// The length.
    ///
    /// Imath scales a vector down before squaring it when its squared
    /// length would underflow, so that a tiny vector still has a length;
    /// [`f64::hypot`] does the same job.
    #[must_use]
    pub fn length(self) -> f64 {
        let squared = self.length2();
        if squared < 2.0 * f64::MIN_POSITIVE {
            return self.x.hypot(self.y);
        }
        squared.sqrt()
    }

    /// The vector scaled to length one, or itself if it has no length.
    ///
    /// Imath's `normalized`, which leaves a null vector alone rather than
    /// dividing by zero.
    #[must_use]
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length == 0.0 {
            return self;
        }
        Self::new(self.x / length, self.y / length)
    }

    /// The vector scaled to length one, or `None` for a null vector.
    ///
    /// Imath's `normalizedExc`, which throws for a null vector.
    #[must_use]
    pub fn normalized_checked(self) -> Option<Self> {
        let length = self.length();
        (length != 0.0).then(|| Self::new(self.x / length, self.y / length))
    }

    /// The vector divided by its length with no check at all.
    ///
    /// Imath's `normalizedNonNull`: a null vector comes back as NaNs.
    #[must_use]
    pub fn normalized_unchecked(self) -> Self {
        let length = self.length();
        Self::new(self.x / length, self.y / length)
    }

    /// Whether each component is within `error` of `other`'s.
    #[must_use]
    pub fn equal_with_abs_error(self, other: Self, error: f64) -> bool {
        (self.x - other.x).abs() <= error && (self.y - other.y).abs() <= error
    }

    /// Whether each component is within `error` times its own size of
    /// `other`'s.
    #[must_use]
    pub fn equal_with_rel_error(self, other: Self, error: f64) -> bool {
        (self.x - other.x).abs() <= error * self.x.abs()
            && (self.y - other.y).abs() <= error * self.y.abs()
    }
}

impl std::ops::Add for V2d {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Sub for V2d {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

/// Component by component, as Imath multiplies two vectors.
impl std::ops::Mul for V2d {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y)
    }
}

/// Component by component, as Imath divides two vectors.
impl std::ops::Div for V2d {
    type Output = Self;

    fn div(self, other: Self) -> Self {
        Self::new(self.x / other.x, self.y / other.y)
    }
}

/// An axis-aligned rectangle, used for a media reference's image bounds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Box2d {
    /// The corner with the smaller coordinates.
    pub min: V2d,
    /// The corner with the larger coordinates.
    pub max: V2d,
}

impl Box2d {
    /// Construct a rectangle from its two corners.
    #[must_use]
    pub const fn new(min: V2d, max: V2d) -> Self {
        Self { min, max }
    }

    /// The smallest rectangle holding both this one and `other`.
    #[must_use]
    pub fn extended_by(self, other: Self) -> Self {
        Self {
            min: V2d::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: V2d::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// The smallest rectangle holding both this one and `point`.
    #[must_use]
    pub fn extended_by_point(self, point: V2d) -> Self {
        self.extended_by(Self::new(point, point))
    }

    /// The point halfway between the corners.
    #[must_use]
    pub fn center(self) -> V2d {
        // Imath's formula, kept rather than `f64::midpoint` so the two
        // round alike.
        V2d::new(
            (self.max.x + self.min.x) / 2.0,
            (self.max.y + self.min.y) / 2.0,
        )
    }

    /// Whether `point` lies inside or on the edge of the rectangle.
    #[must_use]
    pub fn contains_point(self, point: V2d) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Whether the rectangles overlap, touching edges included.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        other.max.x >= self.min.x
            && other.min.x <= self.max.x
            && other.max.y >= self.min.y
            && other.min.y <= self.max.y
    }
}

/// An ordered map of metadata keys to values.
///
/// Upstream backs `AnyDictionary` with `std::map`, so its keys are ordered and
/// serialization is deterministic. A `BTreeMap` gives the same ordering.
pub type AnyDictionary = BTreeMap<String, Any>;

/// A dynamically typed value, as stored in `metadata` and in a generator
/// reference's `parameters`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Any {
    /// JSON `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer too large for [`Any::Int`].
    UInt(u64),
    /// A floating-point number.
    Double(f64),
    /// A string.
    String(String),
    /// A `RationalTime.1`.
    RationalTime(RationalTime),
    /// A `TimeRange.1`.
    TimeRange(TimeRange),
    /// A `TimeTransform.1`.
    TimeTransform(TimeTransform),
    /// A `Color.1`.
    Color(Color),
    /// A `V2d.1`.
    V2d(V2d),
    /// A `Box2d.1`.
    Box2d(Box2d),
    /// An array of values.
    Vector(Vec<Any>),
    /// A nested dictionary.
    Dictionary(AnyDictionary),
    /// A reference to an object living in the document's arena.
    ///
    /// Metadata may hold whole OTIO objects, not just plain data.
    Object(NodeId),
}

impl Any {
    /// Returns the name of this value's type, for error messages.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::UInt(_) => "unsigned int",
            Self::Double(_) => "double",
            Self::String(_) => "string",
            Self::RationalTime(_) => "RationalTime",
            Self::TimeRange(_) => "TimeRange",
            Self::TimeTransform(_) => "TimeTransform",
            Self::Color(_) => "Color",
            Self::V2d(_) => "V2d",
            Self::Box2d(_) => "Box2d",
            Self::Vector(_) => "array",
            Self::Dictionary(_) => "object",
            Self::Object(_) => "OTIO object",
        }
    }

    /// Returns the string inside, if this is a [`Any::String`].
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the dictionary inside, if this is a [`Any::Dictionary`].
    #[must_use]
    pub const fn as_dictionary(&self) -> Option<&AnyDictionary> {
        match self {
            Self::Dictionary(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the array inside, if this is a [`Any::Vector`].
    #[must_use]
    pub fn as_slice(&self) -> Option<&[Any]> {
        match self {
            Self::Vector(value) => Some(value),
            _ => None,
        }
    }
}

impl From<bool> for Any {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Any {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<f64> for Any {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

impl From<String> for Any {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for Any {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl Any {
    /// Runs `f` on every object handle this value holds, however deeply
    /// nested, without changing any.
    pub fn visit_objects(&self, f: &mut impl FnMut(NodeId)) {
        match self {
            Self::Object(id) => f(*id),
            Self::Vector(items) => {
                for item in items {
                    item.visit_objects(f);
                }
            }
            Self::Dictionary(entries) => {
                for value in entries.values() {
                    value.visit_objects(f);
                }
            }
            _ => {}
        }
    }

    /// Runs `f` on every object handle inside this value, however deeply
    /// nested.
    ///
    /// Metadata may hold whole OTIO objects, and a vector or dictionary may
    /// hold more of them, so anything that rewrites handles has to reach all
    /// the way down.
    pub fn visit_objects_mut(&mut self, f: &mut impl FnMut(&mut NodeId)) {
        match self {
            Self::Object(id) => f(id),
            Self::Vector(items) => {
                for item in items {
                    item.visit_objects_mut(f);
                }
            }
            Self::Dictionary(entries) => {
                for value in entries.values_mut() {
                    value.visit_objects_mut(f);
                }
            }
            _ => {}
        }
    }
}

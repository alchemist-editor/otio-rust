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

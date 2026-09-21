//! The small value types OTIO objects and their metadata carry.

use otio_core::{Box2d, Color, V2d};

/// A colour, as four components from 0 to 1.
///
/// OTIO's `Color` also carries a name, which the calls that read and write a
/// colour take separately: a name is variable-length and a colour is not.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioColor {
    /// Red.
    pub r: f64,
    /// Green.
    pub g: f64,
    /// Blue.
    pub b: f64,
    /// Alpha.
    pub a: f64,
}

/// A point, or a size, in two dimensions.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioV2d {
    /// The horizontal component.
    pub x: f64,
    /// The vertical component.
    pub y: f64,
}

/// An axis-aligned rectangle.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioBox2d {
    /// The corner with the smaller coordinates.
    pub min: OtioV2d,
    /// The corner with the larger coordinates.
    pub max: OtioV2d,
}

impl OtioColor {
    /// Builds a `Color` with a name, from this colour's components.
    pub(crate) fn to_color(self, name: &str) -> Color {
        Color::new(self.r, self.g, self.b, self.a, name.to_string())
    }
}

impl From<&Color> for OtioColor {
    fn from(color: &Color) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
        }
    }
}

impl From<OtioV2d> for V2d {
    fn from(value: OtioV2d) -> Self {
        Self::new(value.x, value.y)
    }
}

impl From<V2d> for OtioV2d {
    fn from(value: V2d) -> Self {
        Self {
            x: value.x,
            y: value.y,
        }
    }
}

impl From<OtioBox2d> for Box2d {
    fn from(value: OtioBox2d) -> Self {
        Self::new(value.min.into(), value.max.into())
    }
}

impl From<Box2d> for OtioBox2d {
    fn from(value: Box2d) -> Self {
        Self {
            min: value.min.into(),
            max: value.max.into(),
        }
    }
}

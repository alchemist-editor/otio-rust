//! ORI's image coordinates, and converting them to pixels and to OTIO's own
//! image space.
//!
//! ORI places every point height-normalised, with Y up and the origin at the
//! centre of the image:
//!
//! ```text
//!   x = (px - W/2) / H          px = x * H + W/2
//!   y = -(py - H/2) / H         py = -y * H + H/2
//!
//!            y = +0.5
//!      ┌───────────────────┐
//!      │                   │      x runs from -W/(2H) to +W/(2H),
//!  ────┼─────────●─────────┼──    so ±8/9 on a 16:9 frame;
//!      │       (0,0)       │      y always runs from -0.5 to +0.5.
//!      └───────────────────┘
//!            y = -0.5
//! ```
//!
//! This is a port of ORI's `otio_sync_core/coords.py`, with its numbers.
//!
//! OpenTimelineIO's own image space is unit-less and Y up, and a media
//! reference may say where its picture sits in it with
//! `available_image_bounds`. [`ori_to_bounds`] and [`bounds_to_ori`] map
//! ORI's space onto such bounds: the bounds' centre is ORI's origin and their
//! height is ORI's unit. Without bounds ORI assumes a 16:9 frame, which
//! [`DEFAULT_ASPECT_HALF`] carries.

use otio_core::{Box2d, Document, NodeId, V2d};

use crate::Result;

/// Half the width of a 16:9 frame in ORI's units, `1920 / (2 × 1080)`: what
/// ORI assumes when it does not know the image's size.
pub const DEFAULT_ASPECT_HALF: f64 = 8.0 / 9.0;

/// The letter spacing ORI gives text that does not say: RV's neutral
/// spacing.
pub const DEFAULT_SPACING: f64 = 0.8;

/// The font size ORI gives text that does not say.
pub const DEFAULT_FONT_SIZE: f64 = 50.0;

/// Half the image's width in ORI's units, `W / (2H)`.
///
/// [`DEFAULT_ASPECT_HALF`] when the height is not positive, so an image of
/// unknown size never divides by zero, as in ORI.
#[must_use]
pub fn aspect_half(width: f64, height: f64) -> f64 {
    // A NaN height falls back too, as Python's falsy test does for None.
    if height.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return DEFAULT_ASPECT_HALF;
    }
    width / (2.0 * height)
}

/// An image's size in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageSize {
    /// Its width.
    pub width: f64,
    /// Its height.
    pub height: f64,
}

impl ImageSize {
    /// An image `width` by `height` pixels.
    #[must_use]
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// Converts a pixel position, counted from the top left, to ORI's
    /// coordinates.
    #[must_use]
    pub fn pixels_to_ori(self, px: f64, py: f64) -> (f64, f64) {
        let x = (px - self.width / 2.0) / self.height;
        let y = -((py - self.height / 2.0) / self.height);
        (x, y)
    }

    /// Converts ORI's coordinates to a pixel position counted from the top
    /// left, unrounded. The inverse of [`ImageSize::pixels_to_ori`].
    #[must_use]
    pub fn ori_to_pixels(self, x: f64, y: f64) -> (f64, f64) {
        let px = x * self.height + self.width / 2.0;
        let py = -y * self.height + self.height / 2.0;
        (px, py)
    }

    /// Half the image's width in ORI's units; see [`aspect_half`].
    #[must_use]
    pub fn aspect_half(self) -> f64 {
        aspect_half(self.width, self.height)
    }
}

/// Converts ORI's coordinates to a point in OpenTimelineIO image space,
/// given the picture's `available_image_bounds`.
#[must_use]
pub fn ori_to_bounds(x: f64, y: f64, bounds: Box2d) -> V2d {
    let height = bounds.max.y - bounds.min.y;
    let centre_x = (bounds.min.x + bounds.max.x) / 2.0;
    let centre_y = (bounds.min.y + bounds.max.y) / 2.0;
    V2d {
        x: centre_x + x * height,
        y: centre_y + y * height,
    }
}

/// Converts a point in OpenTimelineIO image space to ORI's coordinates,
/// given the picture's `available_image_bounds`. The inverse of
/// [`ori_to_bounds`]; bounds with no height give the origin.
#[must_use]
pub fn bounds_to_ori(point: V2d, bounds: Box2d) -> (f64, f64) {
    let height = bounds.max.y - bounds.min.y;
    if height == 0.0 {
        return (0.0, 0.0);
    }
    let centre_x = (bounds.min.x + bounds.max.x) / 2.0;
    let centre_y = (bounds.min.y + bounds.max.y) / 2.0;
    ((point.x - centre_x) / height, (point.y - centre_y) / height)
}

/// Half the width of an item's picture in ORI's units: from its active media
/// reference's `available_image_bounds` when it has them, or
/// [`DEFAULT_ASPECT_HALF`].
///
/// # Errors
///
/// [`otio_core::Error::StaleHandle`] if `item` is not in the document, or
/// what otio-core says when it is not an item.
pub fn item_aspect_half(document: &Document, item: NodeId) -> Result<f64> {
    Ok(match document.available_image_bounds(item) {
        Ok(Some(bounds)) => aspect_half(bounds.max.x - bounds.min.x, bounds.max.y - bounds.min.y),
        Ok(None) | Err(otio_core::Error::NoImageBounds { .. }) => DEFAULT_ASPECT_HALF,
        Err(error) => return Err(error.into()),
    })
}

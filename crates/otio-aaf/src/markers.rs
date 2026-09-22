//! Colours: a marker's, and a clip's.
//!
//! Avid stores a marker's colour three ways, newest first: an extended colour
//! name, a legacy colour name, and a 16-bit RGB triple. Upstream reads them
//! in that order and, for the triple, picks the nearest of OTIO's marker
//! colours by hue, which is ported here as it is written.
//!
//! OTIO 0.18, which upstream targets, stores a marker's colour as a name.
//! OTIO 0.19, which this workspace targets, stores it as a colour, and
//! upgrades a name to one on reading. A marker made here is given the colour
//! that upgrade would give it, so reading an AAF here and reading upstream's
//! output here come to the same thing.

use otio_core::upgrade::color_from_legacy_name;
use otio_core::{Any, AnyDictionary, Color};

/// A marker's colour, from the metadata its descriptive marker carries.
pub(crate) fn marker_color(metadata: &AnyDictionary) -> Color {
    let attributes = match metadata.get("CommentMarkerAttributeList") {
        Some(Any::Dictionary(attributes)) => Some(attributes),
        _ => None,
    };
    let named = |key: &str| {
        attributes
            .and_then(|attributes| match attributes.get(key) {
                Some(Any::String(name)) => Some(name.as_str()),
                _ => None,
            })
            .and_then(marker_color_named)
    };
    let name = named("_ATN_CRM_COLOR_EXTENDED")
        .or_else(|| named("_ATN_CRM_COLOR"))
        .unwrap_or_else(|| nearest_marker_color(metadata.get("CommentMarkerColor")));
    color_from_legacy_name(name)
}

/// OTIO's marker colour of this name, if there is one.
///
/// Upstream looks the name up as an attribute of `MarkerColor`, upper-cased,
/// so any case matches and nothing else does.
fn marker_color_named(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    let upper = name.to_uppercase();
    MARKER_COLORS.into_iter().find(|known| *known == upper)
}

/// The marker colours OTIO 0.18 names.
const MARKER_COLORS: [&str; 11] = [
    "PINK", "RED", "ORANGE", "YELLOW", "GREEN", "CYAN", "BLUE", "PURPLE", "MAGENTA", "BLACK",
    "WHITE",
];

/// The nearest marker colour to a 16-bit RGB triple.
///
/// Upstream's `_convert_rgb_to_marker_color`, adapted there from OTIO's GES
/// adapter: an exact primary if it is one, else black or white for greys and
/// extremes of lightness, else a colour by hue.
fn nearest_marker_color(rgb: Option<&Any>) -> &'static str {
    let Some(Any::Dictionary(rgb)) = rgb else {
        return "RED";
    };
    if rgb.is_empty() {
        return "RED";
    }
    let channel = |key: &str| -> f64 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a 16-bit channel is exact in f64"
        )]
        match rgb.get(key) {
            Some(Any::Int(v)) => *v as f64 / 65535.0,
            Some(Any::UInt(v)) => *v as f64 / 65535.0,
            Some(Any::Double(v)) => *v / 65535.0,
            _ => 0.0,
        }
    };
    let (red, green, blue) = (channel("red"), channel("green"), channel("blue"));
    match (red, green, blue) {
        (1.0, 0.0, 0.0) => return "RED",
        (0.0, 1.0, 0.0) => return "GREEN",
        (0.0, 0.0, 1.0) => return "BLUE",
        (0.0, 0.0, 0.0) => return "BLACK",
        (1.0, 1.0, 1.0) => return "WHITE",
        _ => {}
    }

    let (hue, lightness, saturation) = rgb_to_hls(red, green, blue);
    if saturation < 0.2 {
        return if lightness > 0.65 { "WHITE" } else { "BLACK" };
    }
    if lightness < 0.13 {
        return "BLACK";
    }
    if lightness > 0.9 {
        return "WHITE";
    }
    let nearest = color_from_hue(hue);
    if nearest == "RED" && lightness > 0.53 {
        return "PINK";
    }
    if nearest == "MAGENTA" && hue < 0.89 && lightness < 0.42 {
        // Some darker magentas look more like purple.
        return "PURPLE";
    }
    nearest
}

/// A marker colour by hue, in `[0, 1)`.
fn color_from_hue(hue: f64) -> &'static str {
    if hue <= 0.04 || hue > 0.93 {
        "RED"
    } else if hue <= 0.13 {
        "ORANGE"
    } else if hue <= 0.2 {
        "YELLOW"
    } else if hue <= 0.43 {
        "GREEN"
    } else if hue <= 0.52 {
        "CYAN"
    } else if hue <= 0.74 {
        "BLUE"
    } else if hue <= 0.82 {
        "PURPLE"
    } else {
        "MAGENTA"
    }
}

/// Python's `colorsys.rgb_to_hls`, operation for operation.
fn rgb_to_hls(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let sumc = maxc + minc;
    let rangec = maxc - minc;
    let l = sumc / 2.0;
    if minc == maxc {
        return (0.0, l, 0.0);
    }
    let s = if l <= 0.5 {
        rangec / sumc
    } else {
        rangec / (2.0 - maxc - minc)
    };
    let rc = (maxc - r) / rangec;
    let gc = (maxc - g) / rangec;
    let bc = (maxc - b) / rangec;
    let h = if r == maxc {
        bc - gc
    } else if g == maxc {
        2.0 + rc - bc
    } else {
        4.0 + gc - rc
    };
    ((h / 6.0).rem_euclid(1.0), l, s)
}

/// The named OTIO colour with exactly these components, or the colour as it
/// is.
///
/// Upstream's `_resolve_named_colors`, which checks in this order — so an
/// RGB of magenta is `Magenta`, never `Pink`, though both have it.
pub(crate) fn named_color(color: Color) -> Color {
    let named = [
        Color::red(),
        Color::green(),
        Color::blue(),
        Color::cyan(),
        Color::magenta(),
        Color::yellow(),
        Color::orange(),
        Color::pink(),
        Color::purple(),
        Color::white(),
        Color::black(),
    ];
    named
        .into_iter()
        .find(|named| named.r == color.r && named.g == color.g && named.b == color.b)
        .unwrap_or(color)
}

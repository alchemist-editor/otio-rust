//! ASC Color Decision List values, as several formats carry them.
//!
//! ALE puts them in an `ASC_SOP` column, an EDL in an `*ASC_SOP` comment, and
//! both land in the same place on the clip: a `cdl` entry in its metadata,
//! shaped the way upstream shapes it. Keeping the shape in one place is what
//! stops an EDL round trip through ALE from quietly renaming a field.

use otio_core::{Any, AnyDictionary};

/// Slope, offset and power, each a value per colour channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sop {
    /// Per-channel slope.
    pub slope: [f64; 3],
    /// Per-channel offset.
    pub offset: [f64; 3],
    /// Per-channel power.
    pub power: [f64; 3],
}

impl Default for Sop {
    /// Returns the identity: the values that change nothing.
    fn default() -> Self {
        Self {
            slope: [1.0; 3],
            offset: [0.0; 3],
            power: [1.0; 3],
        }
    }
}

/// A colour decision: slope, offset and power, and a saturation.
///
/// Either half may be absent, because a file may state one without the other.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cdl {
    /// Slope, offset and power, if the file stated them.
    pub sop: Option<Sop>,
    /// Saturation, if the file stated it.
    pub sat: Option<f64>,
}

impl Cdl {
    /// Returns whether the file stated neither half.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.sop.is_none() && self.sat.is_none()
    }

    /// Reads slope, offset and power out of a free-form string.
    ///
    /// This is deliberately lenient, the way upstream's ALE adapter is: it
    /// takes the decimal numbers in order and ignores everything between
    /// them, so `(0.9 1.0 1.1)(0 0 0)(1 1 1)` and the same values separated
    /// by commas both read. Nine numbers give slope, offset and power; a
    /// tenth is taken as the saturation. Fewer than nine gives nothing.
    ///
    /// Note that a number written without a decimal point is not seen, which
    /// is upstream's behaviour: its pattern requires the point. A `CDL` column
    /// reading `(1 1 1) (0 0 0) (1 1 1) (0.9)` therefore yields only the
    /// saturation's own digits, so it parses as nothing at all.
    #[must_use]
    pub fn parse_loose(input: &str) -> Self {
        let values = decimals(input);
        if values.len() < 9 {
            return Self::default();
        }

        Self {
            sop: Some(Sop {
                slope: [values[0], values[1], values[2]],
                offset: [values[3], values[4], values[5]],
                power: [values[6], values[7], values[8]],
            }),
            sat: (values.len() == 10).then(|| values[9]),
        }
    }

    /// Returns the `cdl` metadata dictionary for these values.
    ///
    /// Only the halves the file stated appear, so a clip that carried a
    /// saturation and nothing else does not gain an invented identity SOP.
    #[must_use]
    pub fn to_metadata(self) -> AnyDictionary {
        let mut cdl = AnyDictionary::new();
        if let Some(sop) = self.sop {
            let mut values = AnyDictionary::new();
            values.insert("slope".to_string(), channels(sop.slope));
            values.insert("offset".to_string(), channels(sop.offset));
            values.insert("power".to_string(), channels(sop.power));
            cdl.insert("asc_sop".to_string(), Any::Dictionary(values));
        }
        if let Some(sat) = self.sat {
            cdl.insert("asc_sat".to_string(), Any::Double(sat));
        }
        cdl
    }

    /// Reads back what [`Cdl::to_metadata`] wrote.
    ///
    /// A half that is absent or malformed reads as absent rather than as an
    /// error: metadata is free-form, and a writer should not refuse a
    /// document because something else put an unexpected value under `cdl`.
    #[must_use]
    pub fn from_metadata(metadata: &AnyDictionary) -> Self {
        let sop = metadata
            .get("asc_sop")
            .and_then(Any::as_dictionary)
            .and_then(|values| {
                Some(Sop {
                    slope: triple(values.get("slope")?)?,
                    offset: triple(values.get("offset")?)?,
                    power: triple(values.get("power")?)?,
                })
            });
        let sat = metadata.get("asc_sat").and_then(number);
        Self { sop, sat }
    }
}

/// Wraps three channel values as metadata.
fn channels(values: [f64; 3]) -> Any {
    Any::Vector(values.into_iter().map(Any::Double).collect())
}

/// Reads three channel values back out of metadata.
fn triple(value: &Any) -> Option<[f64; 3]> {
    let values = value.as_slice()?;
    let [first, second, third] = values else {
        return None;
    };
    Some([number(first)?, number(second)?, number(third)?])
}

/// Reads a number out of metadata, whichever numeric type it was stored as.
fn number(value: &Any) -> Option<f64> {
    match value {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a CDL value read back as an integer is small; the alternative is refusing it"
        )]
        Any::Int(value) => Some(*value as f64),
        #[expect(clippy::cast_precision_loss, reason = "as above")]
        Any::UInt(value) => Some(*value as f64),
        Any::Double(value) => Some(*value),
        _ => None,
    }
}

/// Returns every decimal number in a string, in order.
///
/// A number here is an optional minus sign, then digits, then a point, then
/// digits, which is upstream's `(-*\d+\.\d+)` on every input a real file
/// carries, including that an integer written without a point is not seen.
///
/// **Deviation.** Upstream writes the sign as `-*`, so it matches a run of
/// them and then hands `--1.0` to `float()`, which raises and turns into a
/// parse error for the whole line. Here a doubled sign simply does not start
/// a number, so `--1.0` yields `-1.0` from its second character on. Both
/// behaviours are only reachable from input that is already malformed.
fn decimals(input: &str) -> Vec<f64> {
    let bytes = input.as_bytes();
    let mut values = Vec::new();
    let mut start = 0;

    while start < bytes.len() {
        let mut at = start;
        if bytes.get(at) == Some(&b'-') {
            at += 1;
        }
        let digits = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == digits || bytes.get(at) != Some(&b'.') {
            start += 1;
            continue;
        }
        at += 1;
        let fraction = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == fraction {
            start += 1;
            continue;
        }

        match input[start..at].parse::<f64>() {
            Ok(value) => {
                values.push(value);
                start = at;
            }
            Err(_) => start += 1,
        }
    }

    values
}

#[cfg(test)]
mod tests {
    use super::{Cdl, Sop, decimals};

    #[test]
    fn reads_parenthesised_triples() {
        let cdl = Cdl::parse_loose(
            "(0.8714 0.9334 0.9947)(-0.087 -0.0922 -0.0808)(0.9988 1.0218 1.0101)",
        );
        assert_eq!(
            cdl.sop,
            Some(Sop {
                slope: [0.8714, 0.9334, 0.9947],
                offset: [-0.087, -0.0922, -0.0808],
                power: [0.9988, 1.0218, 1.0101],
            })
        );
        assert_eq!(cdl.sat, None);
    }

    #[test]
    fn a_tenth_value_is_the_saturation() {
        let cdl = Cdl::parse_loose(
            "(0.8714 0.9334 0.9947) (-0.0870 -0.0922 -0.0808) (0.9988 1.0218 1.0101) (0.9000)",
        );
        assert_eq!(cdl.sat, Some(0.9));
    }

    #[test]
    fn fewer_than_nine_values_is_nothing() {
        assert!(Cdl::parse_loose("(1.0 1.0 1.0)").is_empty());
    }

    #[test]
    fn integers_without_a_point_are_not_values() {
        // Upstream's pattern requires the decimal point, so a CDL column
        // written with whole numbers reads as empty rather than as identity.
        assert!(Cdl::parse_loose("(1 1 1) (0 0 0) (1 1 1) (0.9)").is_empty());
    }

    #[test]
    fn round_trips_through_metadata() {
        let cdl = Cdl {
            sop: Some(Sop::default()),
            sat: Some(1.2),
        };
        assert_eq!(Cdl::from_metadata(&cdl.to_metadata()), cdl);
    }

    #[test]
    fn a_negative_value_reads() {
        assert_eq!(decimals("slope -1.5 and 2.25"), vec![-1.5, 2.25]);
    }
}

//! OTIO metadata the way upstream's writer sees it: as Python values.
//!
//! Upstream's writer reads the timeline through OTIO's Python bindings, so a
//! metadata value reaches it as a Python `int`, `float`, `str`, `list` or
//! `dict`, and what the writer does with one is what Python does with it:
//! `x or y` tests truthiness, `int(x)` truncates a float and parses a string,
//! `AAFRational(x)` makes a rational of nearly anything, and a value handed
//! to pyaaf2 is encoded however pyaaf2's type for the property encodes it.
//! These helpers are those operations, over [`Any`].

use aaf::write::{Rational, WriteValue};
use otio_core::{Any, AnyDictionary};

use super::{Fail, unwritable};

/// A dictionary as upstream's writer holds one: a metadata dictionary, or the
/// empty `{}` it falls back to when metadata has none.
///
/// The difference matters because the writer adds keys to one and then walks
/// it. OTIO's `AnyDictionary` is a `std::map`, so its keys come out sorted
/// wherever they were added; the `{}` is a Python `dict`, whose keys come out
/// in the order they were added. The order decides the order properties are
/// set in, which is part of the file.
#[derive(Debug, Clone, Default)]
pub(crate) struct PyDict {
    entries: Vec<(String, Any)>,
    sorted: bool,
}

impl PyDict {
    /// OTIO metadata, whose keys stay sorted.
    pub(crate) fn metadata(dictionary: &AnyDictionary) -> Self {
        Self {
            entries: dictionary
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            sorted: true,
        }
    }

    /// `value.get(key, {})` for a value upstream treats as a dictionary.
    ///
    /// A missing key gives Python's `{}`; a value that is not a dictionary is
    /// what upstream fails on, calling `.get` on it.
    pub(crate) fn get_dict(&self, key: &str) -> Result<Self, Fail> {
        match self.get(key) {
            None => Ok(Self::default()),
            Some(Any::Dictionary(d)) => Ok(Self::metadata(d)),
            Some(other) => Err(unwritable(format!(
                "metadata '{key}' is a {}, where a dictionary was expected",
                other.type_name()
            ))),
        }
    }

    /// `value.get(key)`.
    pub(crate) fn get(&self, key: &str) -> Option<&Any> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `value[key] = new`: in place if the key is there, otherwise where the
    /// kind of dictionary puts a new key.
    pub(crate) fn set(&mut self, key: &str, new: Any) {
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k == key) {
            entry.1 = new;
            return;
        }
        let at = if self.sorted {
            self.entries.partition_point(|(k, _)| k.as_str() < key)
        } else {
            self.entries.len()
        };
        self.entries.insert(at, (key.to_owned(), new));
    }

    /// The entries, in the order Python walks them.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &Any)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// `metadata.get("AAF", {})` on an object's metadata.
pub(crate) fn aaf_metadata(metadata: &AnyDictionary) -> Result<PyDict, Fail> {
    PyDict::metadata(metadata).get_dict("AAF")
}

/// Python's truthiness.
pub(crate) fn truthy(value: &Any) -> bool {
    match value {
        Any::Null => false,
        Any::Bool(b) => *b,
        Any::Int(i) => *i != 0,
        Any::UInt(u) => *u != 0,
        Any::Double(d) => *d != 0.0,
        Any::String(s) => !s.is_empty(),
        Any::Vector(v) => !v.is_empty(),
        Any::Dictionary(d) => !d.is_empty(),
        _ => true,
    }
}

/// `a or b or ...`: the first truthy value, if any is.
pub(crate) fn first_truthy<'a>(
    values: impl IntoIterator<Item = Option<&'a Any>>,
) -> Option<&'a Any> {
    values.into_iter().flatten().find(|v| truthy(v))
}

/// Python's `int(value)`.
pub(crate) fn py_int(value: &Any) -> Result<i64, Fail> {
    match value {
        Any::Int(i) => Ok(*i),
        Any::UInt(u) => i64::try_from(*u).map_err(|_| unwritable(format!("{u} is too large"))),
        Any::Bool(b) => Ok(i64::from(*b)),
        Any::Double(d) => float_int(*d),
        Any::String(s) => {
            parse_int(s).ok_or_else(|| unwritable(format!("invalid literal for int(): '{s}'")))
        }
        other => Err(unwritable(format!(
            "int() cannot convert a {}",
            other.type_name()
        ))),
    }
}

/// `int(x)` of a float: towards zero, and refused for infinities and NaN.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn float_int(value: f64) -> Result<i64, Fail> {
    if value.is_finite() {
        Ok(value.trunc() as i64)
    } else {
        Err(unwritable(format!("cannot convert {value} to an integer")))
    }
}

/// Python's `int(str)`: optional whitespace and sign, then decimal digits,
/// with single underscores allowed between them.
fn parse_int(text: &str) -> Option<i64> {
    let s = text.trim();
    let (negative, digits) = match s.as_bytes().first()? {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let value: i64 = digits.replace('_', "").parse().ok()?;
    Some(if negative { -value } else { value })
}

/// `AAFRational(value)`.
pub(crate) fn py_rational(value: &Any) -> Result<Rational, Fail> {
    match value {
        Any::Int(i) => Ok(Rational::new(*i, 1)),
        Any::UInt(_) | Any::Bool(_) => Ok(Rational::new(py_int(value)?, 1)),
        Any::Double(d) => {
            Rational::from_f64(*d).ok_or_else(|| unwritable(format!("{d} cannot be a rational")))
        }
        Any::String(s) => Rational::parse(s).map_err(|e| unwritable(e.to_string())),
        other => Err(unwritable(format!(
            "a {} cannot be a rational",
            other.type_name()
        ))),
    }
}

/// `float(AAFRational(value))`.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn py_rational_float(value: &Any) -> Result<f64, Fail> {
    let r = py_rational(value)?;
    Ok(r.numerator as f64 / r.denominator as f64)
}

/// Python's `str(value)`, for the values upstream stringifies.
pub(crate) fn py_str(value: Option<&Any>) -> String {
    match value {
        None | Some(Any::Null) => "None".to_owned(),
        Some(Any::Bool(true)) => "True".to_owned(),
        Some(Any::Bool(false)) => "False".to_owned(),
        Some(Any::Int(i)) => i.to_string(),
        Some(Any::UInt(u)) => u.to_string(),
        Some(Any::Double(d)) => crate::py::python_float(*d),
        Some(Any::String(s)) => s.clone(),
        // Nothing upstream stringifies is any of these in a file it reads, and
        // Python would print an OTIO object's repr, which nothing here needs.
        Some(other) => other.type_name().to_owned(),
    }
}

/// Iterating a value, as `[int(x) for x in value]` does.
///
/// A list gives its items and a dictionary its keys; a string gives its
/// characters, one string each.
pub(crate) fn py_iter(value: &Any) -> Result<Vec<Any>, Fail> {
    match value {
        Any::Vector(items) => Ok(items.clone()),
        Any::Dictionary(d) => Ok(d.keys().map(|k| Any::String(k.clone())).collect()),
        Any::String(s) => Ok(s.chars().map(|c| Any::String(c.to_string())).collect()),
        other => Err(unwritable(format!(
            "a {} cannot be iterated",
            other.type_name()
        ))),
    }
}

/// A value to hand pyaaf2 as it is, or `None` for Python's `None`, which
/// pyaaf2 takes as "remove the property".
///
/// Integers, floats, strings, lists and dictionaries become what pyaaf2
/// would encode them as. Anything else is not a value upstream could have
/// handed pyaaf2 without failing.
pub(crate) fn to_write_value(value: &Any) -> Result<Option<WriteValue>, Fail> {
    Ok(match value {
        Any::Null => None,
        Any::Bool(b) => Some(WriteValue::Bool(*b)),
        Any::Int(i) => Some(WriteValue::Int(*i)),
        Any::UInt(_) => Some(WriteValue::Int(py_int(value)?)),
        Any::Double(d) => Some(WriteValue::Float(*d)),
        Any::String(s) => Some(WriteValue::Str(s.clone())),
        Any::Vector(items) => Some(WriteValue::Array(
            items
                .iter()
                .map(|item| {
                    to_write_value(item)?.ok_or_else(|| unwritable("None inside a list".to_owned()))
                })
                .collect::<Result<_, _>>()?,
        )),
        Any::Dictionary(d) => Some(WriteValue::Record(
            d.iter()
                .map(|(k, v)| {
                    let v = to_write_value(v)?
                        .ok_or_else(|| unwritable(format!("None as the member '{k}'")))?;
                    Ok((k.clone(), v))
                })
                .collect::<Result<_, Fail>>()?,
        )),
        other => {
            return Err(unwritable(format!(
                "pyaaf2 cannot store a {}",
                other.type_name()
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_parse_as_python_parses_them() {
        assert_eq!(parse_int(" 42 "), Some(42));
        assert_eq!(parse_int("-1_000"), Some(-1000));
        assert_eq!(parse_int("1.5"), None);
        assert_eq!(parse_int("_1"), None);
        assert_eq!(parse_int(""), None);
        assert_eq!(py_int(&Any::Double(-2.7)).ok(), Some(-2));
    }

    #[test]
    fn a_new_key_goes_where_the_kind_of_dictionary_puts_it() {
        let mut metadata = AnyDictionary::new();
        metadata.insert("Channels".to_owned(), Any::Int(2));
        let mut sorted = PyDict::metadata(&metadata);
        sorted.set("AverageBPS", Any::Int(1));
        sorted.set("Length", Any::Int(3));
        let keys: Vec<_> = sorted.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["AverageBPS", "Channels", "Length"]);

        let mut plain = PyDict::default();
        plain.set("Length", Any::Int(3));
        plain.set("AverageBPS", Any::Int(1));
        plain.set("Length", Any::Int(4));
        let keys: Vec<_> = plain.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["Length", "AverageBPS"]);
    }
}

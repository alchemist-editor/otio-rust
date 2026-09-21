//! Reading and writing the free-form metadata an OTIO object carries.
//!
//! # Paths
//!
//! Metadata is a tree of dictionaries and arrays, so a value is named by a
//! path rather than a key: `"cmx_3600.reel"` reaches into a nested
//! dictionary, `"comments[0]"` into an array, and the two mix freely. An
//! empty path, or a null one, names the object's whole metadata dictionary.
//!
//! The separators are the path's syntax, so a key that itself contains a `.`
//! or a `[` cannot be addressed this way. No adapter in this repository
//! writes such a key. A caller that meets one can still see it with
//! [`otio_metadata_key_at`] and read the whole object with
//! `otio_node_to_json`.
//!
//! # Writing
//!
//! A setter writes one value at a path whose parent already exists. To build
//! a nested structure, create the containers first with
//! [`otio_metadata_set_dictionary`] and [`otio_metadata_set_vector`], then
//! fill them. Setting a key of a dictionary adds it; setting an index of an
//! array requires the array to be long enough already, which is what
//! [`otio_metadata_set_vector`] takes a length for.

use std::ffi::c_char;

use otio_core::{Any, AnyDictionary, Document};

use crate::buffer::OtioBuffer;
use crate::handle::{
    OtioDocument, OtioNode, document, document_mut, optional_text, text, write_out,
};
use crate::node::{node, node_mut};
use crate::status::{Fault, OtioStatus, Outcome, guard};
use crate::time::{OtioRationalTime, OtioTimeRange, OtioTimeTransform};
use crate::value::{OtioBox2d, OtioColor, OtioV2d};

/// What kind of value sits at a path.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioValueKind {
    /// JSON `null`.
    Null = 0,
    /// A boolean.
    Bool = 1,
    /// A signed integer.
    Int = 2,
    /// An unsigned integer too large to be signed.
    UInt = 3,
    /// A number.
    Double = 4,
    /// A string.
    String = 5,
    /// A `RationalTime`.
    RationalTime = 6,
    /// A `TimeRange`.
    TimeRange = 7,
    /// A `TimeTransform`.
    TimeTransform = 8,
    /// A `Color`.
    Color = 9,
    /// A `V2d`.
    V2d = 10,
    /// A `Box2d`.
    Box2d = 11,
    /// An array.
    Vector = 12,
    /// A dictionary.
    Dictionary = 13,
    /// A whole OTIO object, named by a handle.
    Object = 14,
    /// A kind added to the core since this ABI was written.
    ///
    /// Nothing produces this today. It exists so that a core that grows a new
    /// metadata type reports something honest rather than a kind that means
    /// something else.
    Other = 15,
}

fn kind_of(value: &Any) -> OtioValueKind {
    match value {
        Any::Null => OtioValueKind::Null,
        Any::Bool(_) => OtioValueKind::Bool,
        Any::Int(_) => OtioValueKind::Int,
        Any::UInt(_) => OtioValueKind::UInt,
        Any::Double(_) => OtioValueKind::Double,
        Any::String(_) => OtioValueKind::String,
        Any::RationalTime(_) => OtioValueKind::RationalTime,
        Any::TimeRange(_) => OtioValueKind::TimeRange,
        Any::TimeTransform(_) => OtioValueKind::TimeTransform,
        Any::Color(_) => OtioValueKind::Color,
        Any::V2d(_) => OtioValueKind::V2d,
        Any::Box2d(_) => OtioValueKind::Box2d,
        Any::Vector(_) => OtioValueKind::Vector,
        Any::Dictionary(_) => OtioValueKind::Dictionary,
        Any::Object(_) => OtioValueKind::Object,
        // `Any` is `#[non_exhaustive]`, so a core that gains a value type
        // still compiles against this crate.
        _ => OtioValueKind::Other,
    }
}

/// One step of a path: a dictionary key or an array index.
#[derive(Debug, PartialEq, Eq)]
enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

/// Splits a path into its steps.
///
/// An empty path has no steps and names the metadata dictionary itself.
fn parse_path(path: &str) -> Outcome<Vec<Step<'_>>> {
    let mut steps = Vec::new();
    let mut rest = path;
    let mut first = true;

    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('[') {
            let end = stripped
                .find(']')
                .ok_or_else(|| Fault::invalid(format!("unclosed '[' in metadata path '{path}'")))?;
            let index: usize = stripped[..end].parse().map_err(|_| {
                Fault::invalid(format!(
                    "'{}' is not an array index in metadata path '{path}'",
                    &stripped[..end]
                ))
            })?;
            steps.push(Step::Index(index));
            rest = &stripped[end + 1..];
            first = false;
            continue;
        }

        if !first {
            rest = rest.strip_prefix('.').ok_or_else(|| {
                Fault::invalid(format!("expected '.' or '[' in metadata path '{path}'"))
            })?;
        }

        let end = rest.find(['.', '[']).unwrap_or(rest.len());
        if end == 0 {
            return Err(Fault::invalid(format!(
                "empty key in metadata path '{path}'"
            )));
        }
        steps.push(Step::Key(&rest[..end]));
        rest = &rest[end..];
        first = false;
    }

    Ok(steps)
}

/// Walks a path down from an object's metadata.
fn resolve<'a>(root: &'a AnyDictionary, path: &str) -> Outcome<Option<&'a Any>> {
    let steps = parse_path(path)?;
    let Some((first, rest)) = steps.split_first() else {
        return Ok(None);
    };

    let Step::Key(key) = first else {
        return Err(Fault::invalid(
            "a metadata path starts with a key, not an index",
        ));
    };
    let Some(mut current) = root.get(*key) else {
        return Ok(None);
    };

    for step in rest {
        current = match (step, current) {
            (Step::Key(key), Any::Dictionary(dictionary)) => match dictionary.get(*key) {
                Some(value) => value,
                None => return Ok(None),
            },
            (Step::Index(index), Any::Vector(values)) => match values.get(*index) {
                Some(value) => value,
                None => return Ok(None),
            },
            (Step::Key(_), other) => {
                return Err(Fault::new(
                    OtioStatus::CoreError,
                    format!("a {} has no keys", other.type_name()),
                ));
            }
            (Step::Index(_), other) => {
                return Err(Fault::new(
                    OtioStatus::CoreError,
                    format!("a {} has no indices", other.type_name()),
                ));
            }
        };
    }

    Ok(Some(current))
}

/// Borrows an object's metadata dictionary.
fn metadata(source: &Document, id: OtioNode) -> Outcome<&AnyDictionary> {
    let node = node(source, id)?;
    node.base().map(|base| &base.metadata).ok_or_else(|| {
        Fault::new(
            OtioStatus::CoreError,
            format!("a {} carries no metadata", node.schema_name()),
        )
    })
}

/// Borrows an object's metadata dictionary for writing.
fn metadata_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut AnyDictionary> {
    let node = node_mut(target, id)?;
    if node.base().is_none() {
        return Err(Fault::new(
            OtioStatus::CoreError,
            format!("a {} carries no metadata", node.schema_name()),
        ));
    }
    Ok(&mut node.base_mut().expect("checked just above").metadata)
}

/// Reads the value at a path, failing where there is none.
fn value_at<'a>(source: &'a Document, id: OtioNode, path: &str) -> Outcome<&'a Any> {
    let root = metadata(source, id)?;
    resolve(root, path)?.ok_or_else(|| Fault::no_value(&format!("metadata '{path}'")))
}

/// Puts a value at a path, whose parent must already exist.
fn put(target: &mut Document, id: OtioNode, path: &str, value: Any) -> Outcome<()> {
    let steps = parse_path(path)?;
    let (last, leading) = steps
        .split_last()
        .ok_or_else(|| Fault::invalid("a metadata path must name something to set"))?;
    let root = metadata_mut(target, id)?;

    let Some((first, middle)) = leading.split_first() else {
        // The path is one step, so it names a key of the metadata itself.
        let Step::Key(key) = last else {
            return Err(Fault::invalid(
                "a metadata path starts with a key, not an index",
            ));
        };
        root.insert((*key).to_string(), value);
        return Ok(());
    };

    let Step::Key(key) = first else {
        return Err(Fault::invalid(
            "a metadata path starts with a key, not an index",
        ));
    };
    let mut current = root
        .get_mut(*key)
        .ok_or_else(|| Fault::no_value(&format!("metadata '{key}'")))?;
    for step in middle {
        current = descend(current, step)?;
    }
    put_at(current, last, value)
}

/// Steps one level down a path for writing.
fn descend<'a>(current: &'a mut Any, step: &Step<'_>) -> Outcome<&'a mut Any> {
    match (step, current) {
        (Step::Key(key), Any::Dictionary(dictionary)) => dictionary
            .get_mut(*key)
            .ok_or_else(|| Fault::no_value(&format!("metadata '{key}'"))),
        (Step::Index(index), Any::Vector(values)) => {
            let len = values.len();
            values
                .get_mut(*index)
                .ok_or_else(|| Fault::invalid(format!("index {index} is past the end ({len})")))
        }
        (Step::Key(_), other) => Err(Fault::new(
            OtioStatus::CoreError,
            format!("a {} has no keys", other.type_name()),
        )),
        (Step::Index(_), other) => Err(Fault::new(
            OtioStatus::CoreError,
            format!("a {} has no indices", other.type_name()),
        )),
    }
}

/// Writes the value at the last step of a path.
fn put_at(current: &mut Any, step: &Step<'_>, value: Any) -> Outcome<()> {
    match (step, current) {
        (Step::Key(key), Any::Dictionary(dictionary)) => {
            dictionary.insert((*key).to_string(), value);
            Ok(())
        }
        (Step::Index(index), Any::Vector(values)) => {
            let len = values.len();
            let slot = values
                .get_mut(*index)
                .ok_or_else(|| Fault::invalid(format!("index {index} is past the end ({len})")))?;
            *slot = value;
            Ok(())
        }
        (Step::Key(_), other) => Err(Fault::new(
            OtioStatus::CoreError,
            format!("a {} has no keys", other.type_name()),
        )),
        (Step::Index(_), other) => Err(Fault::new(
            OtioStatus::CoreError,
            format!("a {} has no indices", other.type_name()),
        )),
    }
}

/// Reads the `path` argument, where null means the metadata dictionary.
unsafe fn path_arg<'a>(path: *const c_char) -> Outcome<&'a str> {
    Ok(unsafe { optional_text(path, "path") }?.unwrap_or_default())
}

/// Reports that a value is not the type the call asked for.
fn wrong_type(value: &Any, wanted: &str) -> Fault {
    Fault::new(
        OtioStatus::CoreError,
        format!(
            "metadata holds a {} where a {wanted} was asked for",
            value.type_name()
        ),
    )
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Returns what kind of value sits at a path.
///
/// An empty or null path names the metadata dictionary itself, which is always
/// `OTIO_VALUE_DICTIONARY`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_kind(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_kind: *mut OtioValueKind,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        let root = metadata(source, node_handle)?;
        let kind = match resolve(root, path)? {
            Some(value) => kind_of(value),
            None if path.is_empty() => OtioValueKind::Dictionary,
            None => return Err(Fault::no_value(&format!("metadata '{path}'"))),
        };
        unsafe { write_out(out_kind, kind, "out_kind") }
    })
}

/// Returns whether anything sits at a path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_contains(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_contains: *mut bool,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        let root = metadata(source, node_handle)?;
        let contains = path.is_empty() || resolve(root, path)?.is_some();
        unsafe { write_out(out_contains, contains, "out_contains") }
    })
}

/// Returns how many entries a dictionary or an array at a path holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_len(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_len: *mut usize,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        let root = metadata(source, node_handle)?;
        let len = if path.is_empty() {
            root.len()
        } else {
            match resolve(root, path)?
                .ok_or_else(|| Fault::no_value(&format!("metadata '{path}'")))?
            {
                Any::Dictionary(dictionary) => dictionary.len(),
                Any::Vector(values) => values.len(),
                other => return Err(wrong_type(other, "dictionary or array")),
            }
        };
        unsafe { write_out(out_len, len, "out_len") }
    })
}

/// Returns the key at an index of a dictionary at a path.
///
/// Keys are ordered, so walking the indices walks them the same way twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_key_at(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    index: usize,
    out_key: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        let root = metadata(source, node_handle)?;
        let dictionary = if path.is_empty() {
            root
        } else {
            match resolve(root, path)?
                .ok_or_else(|| Fault::no_value(&format!("metadata '{path}'")))?
            {
                Any::Dictionary(dictionary) => dictionary,
                other => return Err(wrong_type(other, "dictionary")),
            }
        };
        let key = dictionary
            .keys()
            .nth(index)
            .ok_or_else(|| Fault::invalid(format!("no metadata key at index {index}")))?;
        unsafe { write_out(out_key, OtioBuffer::from_str(key), "out_key") }
    })
}

/// Reads a boolean from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_bool(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut bool,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::Bool(value) => unsafe { write_out(out_value, *value, "out_value") },
            other => Err(wrong_type(other, "bool")),
        }
    })
}

/// Reads a signed integer from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_int(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut i64,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::Int(value) => unsafe { write_out(out_value, *value, "out_value") },
            other => Err(wrong_type(other, "int")),
        }
    })
}

/// Reads an unsigned integer from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_uint(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut u64,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::UInt(value) => unsafe { write_out(out_value, *value, "out_value") },
            other => Err(wrong_type(other, "unsigned int")),
        }
    })
}

/// Reads a number from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_double(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut f64,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::Double(value) => unsafe { write_out(out_value, *value, "out_value") },
            other => Err(wrong_type(other, "double")),
        }
    })
}

/// Reads a time from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_rational_time(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::RationalTime(value) => unsafe {
                write_out(out_value, (*value).into(), "out_value")
            },
            other => Err(wrong_type(other, "RationalTime")),
        }
    })
}

/// Reads a span from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_time_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::TimeRange(value) => unsafe { write_out(out_value, (*value).into(), "out_value") },
            other => Err(wrong_type(other, "TimeRange")),
        }
    })
}

/// Reads a transform from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_time_transform(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioTimeTransform,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::TimeTransform(value) => unsafe {
                write_out(out_value, (*value).into(), "out_value")
            },
            other => Err(wrong_type(other, "TimeTransform")),
        }
    })
}

/// Reads a point from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_v2d(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioV2d,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::V2d(value) => unsafe { write_out(out_value, (*value).into(), "out_value") },
            other => Err(wrong_type(other, "V2d")),
        }
    })
}

/// Reads a rectangle from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_box2d(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioBox2d,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::Box2d(value) => unsafe { write_out(out_value, (*value).into(), "out_value") },
            other => Err(wrong_type(other, "Box2d")),
        }
    })
}

/// Reads a handle to an OTIO object held in the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_object(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::Object(value) => unsafe {
                write_out(out_value, OtioNode::from_id(*value), "out_value")
            },
            other => Err(wrong_type(other, "OTIO object")),
        }
    })
}

/// Reads a string from the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_string(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::String(value) => unsafe {
                write_out(out_value, OtioBuffer::from_str(value), "out_value")
            },
            other => Err(wrong_type(other, "string")),
        }
    })
}

/// Reads a colour, and the name that goes with it, from the metadata.
///
/// `out_name` may be null if the name is not wanted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_get_color(
    source: *const OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    out_value: *mut OtioColor,
    out_name: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { path_arg(path) }?;
        let source = unsafe { document(source) }?;
        match value_at(source, node_handle, path)? {
            Any::Color(color) => {
                if !out_name.is_null() {
                    unsafe { write_out(out_name, OtioBuffer::from_str(&color.name), "out_name") }?;
                }
                unsafe { write_out(out_value, OtioColor::from(color), "out_value") }
            }
            other => Err(wrong_type(other, "Color")),
        }
    })
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Writes a boolean into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_bool(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: bool,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Bool(value))
    })
}

/// Writes a signed integer into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_int(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: i64,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Int(value))
    })
}

/// Writes an unsigned integer into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_uint(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: u64,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::UInt(value))
    })
}

/// Writes a number into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_double(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: f64,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Double(value))
    })
}

/// Writes a time into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_rational_time(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::RationalTime(value.into()))
    })
}

/// Writes a span into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_time_range(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::TimeRange(value.into()))
    })
}

/// Writes a transform into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_time_transform(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioTimeTransform,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::TimeTransform(value.into()))
    })
}

/// Writes a point into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_v2d(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioV2d,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::V2d(value.into()))
    })
}

/// Writes a rectangle into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_box2d(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioBox2d,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Box2d(value.into()))
    })
}

/// Writes a handle to an OTIO object into the metadata.
///
/// The object stays where it is and the metadata names it. Nothing checks
/// that the handle is live, because a document being built may not hold the
/// object yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_object(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioNode,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Object(value.to_id()))
    })
}

/// Writes a string into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_string(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: *const c_char,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let value = unsafe { text(value, "value") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::String(value))
    })
}

/// Writes a colour into the metadata. `name` may be null for an unnamed one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_color(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    value: OtioColor,
    name: *const c_char,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let name = unsafe { optional_text(name, "name") }?.unwrap_or_default();
        let color = value.to_color(name);
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Color(color))
    })
}

/// Writes a null into the metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_null(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(target, node_handle, &path, Any::Null)
    })
}

/// Writes an empty dictionary into the metadata, to be filled through deeper
/// paths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_dictionary(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(
            target,
            node_handle,
            &path,
            Any::Dictionary(AnyDictionary::new()),
        )
    })
}

/// Writes an array of `len` nulls into the metadata, to be filled by index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_set_vector(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
    len: usize,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        put(
            target,
            node_handle,
            &path,
            Any::Vector(vec![Any::Null; len]),
        )
    })
}

/// Removes whatever sits at a path.
///
/// Removing a key of a dictionary takes the key with it; removing an element
/// of an array shortens the array.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_remove(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    path: *const c_char,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?.to_string();
        let steps = parse_path(&path)?;
        let (last, leading) = steps
            .split_last()
            .ok_or_else(|| Fault::invalid("a metadata path must name something to remove"))?;
        let target = unsafe { document_mut(target) }?;
        let root = metadata_mut(target, node_handle)?;

        let Some((first, middle)) = leading.split_first() else {
            let Step::Key(key) = last else {
                return Err(Fault::invalid(
                    "a metadata path starts with a key, not an index",
                ));
            };
            return root
                .remove(*key)
                .map(|_| ())
                .ok_or_else(|| Fault::no_value(&format!("metadata '{key}'")));
        };

        let Step::Key(key) = first else {
            return Err(Fault::invalid(
                "a metadata path starts with a key, not an index",
            ));
        };
        let mut current = root
            .get_mut(*key)
            .ok_or_else(|| Fault::no_value(&format!("metadata '{key}'")))?;
        for step in middle {
            current = descend(current, step)?;
        }

        match (last, current) {
            (Step::Key(key), Any::Dictionary(dictionary)) => dictionary
                .remove(*key)
                .map(|_| ())
                .ok_or_else(|| Fault::no_value(&format!("metadata '{key}'"))),
            (Step::Index(index), Any::Vector(values)) => {
                if *index >= values.len() {
                    return Err(Fault::invalid(format!(
                        "index {index} is past the end ({})",
                        values.len()
                    )));
                }
                values.remove(*index);
                Ok(())
            }
            (Step::Key(_), other) => Err(wrong_type(other, "dictionary")),
            (Step::Index(_), other) => Err(wrong_type(other, "array")),
        }
    })
}

/// Empties an object's metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_metadata_clear(
    target: *mut OtioDocument,
    node_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        metadata_mut(target, node_handle)?.clear();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{Step, parse_path};

    #[test]
    fn a_plain_key_is_one_step() {
        assert_eq!(parse_path("reel").unwrap(), vec![Step::Key("reel")]);
    }

    #[test]
    fn dots_and_indices_mix() {
        assert_eq!(
            parse_path("cmx_3600.comments[2].text").unwrap(),
            vec![
                Step::Key("cmx_3600"),
                Step::Key("comments"),
                Step::Index(2),
                Step::Key("text"),
            ]
        );
    }

    #[test]
    fn an_empty_path_names_the_dictionary_itself() {
        assert!(parse_path("").unwrap().is_empty());
    }

    #[test]
    fn a_malformed_path_is_rejected() {
        assert!(parse_path("a[").is_err());
        assert!(parse_path("a[x]").is_err());
        assert!(parse_path("a..b").is_err());
    }
}

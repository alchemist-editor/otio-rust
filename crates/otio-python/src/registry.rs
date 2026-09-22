//! Schemas defined in Python, schema versions, and reading and writing JSON.
//!
//! # A schema defined in Python
//!
//! Upstream lets Python code define a schema of its own:
//!
//! ```python
//! @otio.core.register_type
//! class Thing(otio.core.SerializableObject):
//!     _serializable_label = "Thing.1"
//!     foo = otio.core.serializable_field("foo")
//! ```
//!
//! In upstream's C++ such an object is an ordinary `SerializableObject` whose
//! data sits in its "dynamic fields", with its type record pointed at the
//! registered schema; the Python class is only remembered so that reading a
//! file can build an instance of it. This does the same. The core's
//! [`registry`](otio_core::registry) records the schema, and a document holds
//! an object of it as a [`DynamicObject`] — a schema name, a version, a field
//! map and, for a subclass of `SerializableObjectWithMetadata`, a name and
//! metadata. Nothing Python-specific goes into the document, so such an
//! object round-trips through a file, a copy or the C ABI like any other,
//! and a program that never registered the schema sees it as an unknown one.
//!
//! What stays on this side is the map from schema name to Python class. When
//! a dynamic object needs a wrapper, [`wrap_dynamic`] builds an instance of
//! the registered class — calling it with no arguments, as upstream's type
//! registry does — and points it at the object, keeping whatever the class's
//! `__init__` set for fields the object does not have. `serializable_field`
//! properties read and write the field map through `_dynamic_fields`.
//!
//! # Version functions written in Python
//!
//! `register_upgrade_function` and `register_downgrade_function` hand the core
//! a closure that calls back into Python with the object's fields as a plain
//! `dict`, and reads the dict back when the function returns. An exception
//! the function raises is kept here and raised again, unchanged, by the call
//! that was reading or writing; see [`take_pending_error`].

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use otio_core::json::{Number, Value};
use otio_core::registry::{self, DynamicBase, SchemaKind, SchemaVersionMap, VersionFunction};
use otio_core::schema::{DynamicObject, Node};
use otio_core::{Any, AnyDictionary, Error, WriteOptions};

use pyo3::exceptions::{PyNotImplementedError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyType};
use pyo3::{IntoPyObjectExt, Py, PyAny};

use crate::arena::Shared;
use crate::objects::{
    Handle, PyComposable, PyEffect, PyMarker, PyMediaReference, PySerializableCollection,
    PySerializableObject, PySerializableObjectWithMetadata, PyTimeline, core_error, handle_of,
};
use crate::values::{any_to_python, home_of, python_to_any};

/// Schema name to the Python class registered for it.
static CLASSES: PyOnceLock<Py<PyDict>> = PyOnceLock::new();

fn classes(py: Python<'_>) -> &Bound<'_, PyDict> {
    CLASSES
        .get_or_init(py, || PyDict::new(py).unbind())
        .bind(py)
}

thread_local! {
    /// The exception a Python version function raised, waiting to be raised
    /// again once the core has unwound.
    static PENDING: RefCell<Option<PyErr>> = const { RefCell::new(None) };
}

/// Takes the exception a Python version function raised during the last
/// core call on this thread, if there is one.
pub fn take_pending_error() -> Option<PyErr> {
    PENDING.with(|pending| pending.borrow_mut().take())
}

/// The class registered for `schema_name`, if any.
pub fn registered_class<'py>(py: Python<'py>, schema_name: &str) -> Option<Bound<'py, PyAny>> {
    classes(py).get_item(schema_name).ok().flatten()
}

/// Builds the wrapper for a dynamic object: an instance of its registered
/// class if it has one, and of the base class it derives from otherwise.
///
/// Returns `None` when the schema has no registered class.
pub fn wrap_dynamic<'py>(
    py: Python<'py>,
    handle: &Handle,
    schema_name: &str,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let Some(class) = registered_class(py, schema_name) else {
        return Ok(None);
    };
    // Upstream's type registry makes an object of a Python-defined schema by
    // calling its class with no arguments and then reading the file's fields
    // over whatever `__init__` set. The object already exists here, so the
    // instance is built and then pointed at it, the fields `__init__` set
    // kept only where the object has none of its own.
    let instance = class.call0()?;
    let fresh = handle_of(&instance)?;
    let (shared, id) = handle.live()?;
    shared.absorb(&fresh.shared)?;
    let (_, fresh_id) = fresh.live()?;
    let made = shared.write(|document| Ok(document.remove(fresh_id)))?;
    shared.forget(fresh_id)?;
    if let Some(made) = made {
        let (fields, base) = match made {
            Node::Dynamic(made) => (made.fields, made.base),
            Node::SerializableObjectWithMetadata(base) => (AnyDictionary::new(), Some(base)),
            _ => (AnyDictionary::new(), None),
        };
        shared.write(|document| {
            if let Some(Node::Dynamic(object)) = document.get_mut(id) {
                for (key, value) in fields {
                    object.fields.entry(key).or_insert(value);
                }
                if object.base.is_none() {
                    object.base = base;
                }
            }
            Ok(())
        })?;
    }
    instance.cast::<PySerializableObject>()?.borrow_mut().0 = Handle { shared, id };
    Ok(Some(instance))
}

/// What a class registered as a schema derives from, or an error for a class
/// this port cannot hold as a dynamic object.
fn dynamic_base(class: &Bound<'_, PyAny>) -> PyResult<DynamicBase> {
    let py = class.py();
    let class = class.cast::<PyType>()?;
    // Every class below `SerializableObjectWithMetadata` descends from one of
    // these, and has fields of its own a dynamic object cannot carry.
    let deeper = [
        py.get_type::<PyComposable>(),
        py.get_type::<PyMarker>(),
        py.get_type::<PyEffect>(),
        py.get_type::<PyMediaReference>(),
        py.get_type::<PyTimeline>(),
        py.get_type::<PySerializableCollection>(),
    ];
    for built_in in &deeper {
        if class.is_subclass(built_in)? {
            return Err(PyNotImplementedError::new_err(format!(
                "registering a subclass of {} as a schema of its own is not supported; \
                 derive from SerializableObject or SerializableObjectWithMetadata",
                built_in.name()?
            )));
        }
    }
    if class.is_subclass(&py.get_type::<PySerializableObjectWithMetadata>())? {
        Ok(DynamicBase::SerializableObjectWithMetadata)
    } else if class.is_subclass(&py.get_type::<PySerializableObject>())? {
        Ok(DynamicBase::SerializableObject)
    } else {
        Err(PyTypeError::new_err(format!(
            "{} is not a SerializableObject",
            class.name()?
        )))
    }
}

/// Registers a Python class as the schema `schema_name` at `schema_version`.
///
/// As upstream's, a second registration of the same name changes nothing.
#[pyfunction]
fn register_serializable_object_type(
    class_object: &Bound<'_, PyAny>,
    schema_name: &str,
    schema_version: u32,
) -> PyResult<()> {
    let base = dynamic_base(class_object)?;
    if registry::register_type(schema_name, schema_version, base) {
        classes(class_object.py()).set_item(schema_name, class_object)?;
    }
    Ok(())
}

/// Makes `serializable_obejct` an object of the registered schema
/// `schema_name`.
///
/// Upstream's own spelling of the argument name is kept.
#[pyfunction]
fn set_type_record(serializable_obejct: &Bound<'_, PyAny>, schema_name: &str) -> PyResult<()> {
    let handle = handle_of(serializable_obejct)?;
    let Some(kind) = registry::schema_kind(schema_name) else {
        let type_name = serializable_obejct.get_type().name()?;
        return Err(PyValueError::new_err(format!(
            "schema is not registered/known: Cannot set type record on instance of type \
             {type_name}: schema {schema_name} unregistered"
        )));
    };
    let version = registry::schema_version(schema_name).unwrap_or(1);
    handle.with_mut(|node| {
        if kind == SchemaKind::BuiltIn {
            return if node.schema_name() == registry::canonical_schema_name(schema_name) {
                Ok(())
            } else {
                Err(PyNotImplementedError::new_err(format!(
                    "a {} cannot become a {schema_name}",
                    node.schema_name()
                )))
            };
        }
        let base = match node {
            Node::SerializableObject => None,
            Node::SerializableObjectWithMetadata(base) => Some(std::mem::take(base)),
            Node::Dynamic(dynamic) => {
                dynamic.schema_name = schema_name.to_string();
                dynamic.schema_version = version;
                return Ok(());
            }
            other => {
                return Err(PyNotImplementedError::new_err(format!(
                    "a {} cannot become a {schema_name}",
                    other.schema_name()
                )));
            }
        };
        *node = Node::Dynamic(DynamicObject {
            schema_name: schema_name.to_string(),
            schema_version: version,
            base,
            fields: AnyDictionary::new(),
        });
        Ok(())
    })
}

/// Stands in for upstream's hook into its C++ reference counting, which
/// keeps a Python object alive while C++ still holds it. Objects here live in
/// documents, and their wrappers are kept by identity already, so there is
/// nothing to install.
#[pyfunction]
#[pyo3(signature = (so, apply_now))]
fn install_external_keepalive_monitor(so: &Bound<'_, PyAny>, apply_now: bool) {
    let _ = (so, apply_now);
}

/// Builds an object of `schema_name` at `schema_version` from `data`, as
/// reading it from a file would: upgraded to the registered version, or
/// unknown if nobody registered the schema.
#[pyfunction]
fn instance_from_schema(
    py: Python<'_>,
    schema_name: &str,
    schema_version: u32,
    data: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let home = home_of(data).unwrap_or_default();
    let data = match python_to_any(&home, data)? {
        Any::Dictionary(entries) => entries,
        Any::Null => AnyDictionary::new(),
        _ => return Err(PyTypeError::new_err("data must be a dictionary")),
    };
    let document = home.read(|document| {
        with_pending(registry::instance_from_schema(
            document,
            schema_name,
            schema_version,
            &data,
        ))
    })?;
    let root = document
        .root()
        .ok_or_else(|| PyValueError::new_err("nothing was built"))?;
    let shared = Shared::new();
    shared.write(|slot| {
        *slot = document;
        Ok(())
    })?;
    Ok(crate::objects::wrap(py, &Handle { shared, id: root })?.unbind())
}

/// Every registered schema and the version it is written at.
#[pyfunction]
fn type_version_map() -> BTreeMap<String, u32> {
    registry::type_version_map()
}

/// The schema versions each upstream release wrote, by release label.
#[pyfunction]
fn release_to_schema_version_map() -> BTreeMap<String, BTreeMap<String, u32>> {
    registry::release_to_schema_version_map()
}

/// Wraps a Python function as a version function for the core.
fn version_function(schema_name: &str, function: Py<PyAny>) -> VersionFunction {
    let schema = schema_name.to_string();
    Arc::new(move |fields: &mut AnyDictionary| {
        Python::attach(|py| {
            let result = (|| -> PyResult<AnyDictionary> {
                let dict = plain_to_python(py, fields)?;
                function.call1(py, (dict.clone(),))?;
                plain_from_python(&dict)
            })();
            result.map_or_else(
                |error| {
                    let message = error.to_string();
                    PENDING.with(|pending| *pending.borrow_mut() = Some(error));
                    Err(Error::VersionFunctionFailed {
                        schema: schema.clone(),
                        message,
                    })
                },
                |updated| {
                    *fields = updated;
                    Ok(())
                },
            )
        })
    })
}

/// A field map, in the self-contained form version functions see, as a
/// Python `dict`.
fn plain_to_python<'py>(py: Python<'py>, fields: &AnyDictionary) -> PyResult<Bound<'py, PyDict>> {
    // Nothing in the self-contained form is a handle, so no document is
    // needed to convert it; an empty one stands in.
    let home = Shared::new();
    let dict = PyDict::new(py);
    for (key, value) in fields {
        dict.set_item(key, any_to_python(py, &home, value)?)?;
    }
    Ok(dict)
}

/// A `dict` a version function returned, back in the self-contained form.
///
/// A function may have put an OTIO object in it; that is written out and
/// read back as a dictionary, which is the form the core expects.
fn plain_from_python(dict: &Bound<'_, PyDict>) -> PyResult<AnyDictionary> {
    let home = home_of(dict.as_any()).unwrap_or_default();
    let value = python_to_any(&home, dict.as_any())?;
    let Any::Dictionary(entries) = value else {
        return Err(PyTypeError::new_err("a version function must leave a dict"));
    };
    if !holds_objects(&Any::Dictionary(entries.clone())) {
        return Ok(entries);
    }
    let options = WriteOptions {
        indent: None,
        ..WriteOptions::default()
    };
    let text = home.read(|document| {
        core_error(otio_core::to_string_with(
            document,
            &Any::Dictionary(entries),
            &options,
        ))
    })?;
    match plain_any(&core_error(
        otio_core::json::parse(&text).map_err(Error::from),
    )?) {
        Any::Dictionary(entries) => Ok(entries),
        _ => Err(PyTypeError::new_err("a version function must leave a dict")),
    }
}

fn holds_objects(value: &Any) -> bool {
    match value {
        Any::Object(_) => true,
        Any::Vector(items) => items.iter().any(holds_objects),
        Any::Dictionary(entries) => entries.values().any(holds_objects),
        _ => false,
    }
}

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

/// Registers a Python function upgrading `schema_name` to
/// `version_to_upgrade_to`.
#[pyfunction]
fn register_upgrade_function(
    schema_name: &str,
    version_to_upgrade_to: u32,
    upgrade_function: Py<PyAny>,
) -> bool {
    registry::register_upgrade_function(
        schema_name,
        version_to_upgrade_to,
        version_function(schema_name, upgrade_function),
    )
}

/// Registers a Python function downgrading `schema_name` from
/// `version_to_downgrade_from`.
#[pyfunction]
fn register_downgrade_function(
    schema_name: &str,
    version_to_downgrade_from: u32,
    downgrade_function: Py<PyAny>,
) -> bool {
    registry::register_downgrade_function(
        schema_name,
        version_to_downgrade_from,
        version_function(schema_name, downgrade_function),
    )
}

/// Turns a core result into a Python one, raising a version function's own
/// exception again if that is what went wrong.
pub fn with_pending<T>(result: otio_core::Result<T>) -> PyResult<T> {
    if matches!(result, Err(Error::VersionFunctionFailed { .. })) {
        if let Some(error) = take_pending_error() {
            return Err(error);
        }
    }
    core_error(result)
}

/// The layout a Python `indent` asks for when writing a string: upstream
/// indents only for a positive number and writes compact JSON otherwise.
const fn string_indent(indent: i64) -> Option<usize> {
    if indent > 0 {
        Some(indent as usize)
    } else {
        None
    }
}

/// Writes any value — an object, a list, a dict or a plain value — as JSON.
fn write_value(
    value: &Bound<'_, PyAny>,
    schema_version_targets: SchemaVersionMap,
    indent: Option<usize>,
) -> PyResult<String> {
    let (home, any) = match handle_of(value) {
        Ok(handle) => {
            let (shared, id) = handle.live()?;
            (shared, Any::Object(id))
        }
        Err(_) => {
            // A value with no objects in it needs no document at all.
            let home = home_of(value).unwrap_or_default();
            let any = python_to_any(&home, value)?;
            (home, any)
        }
    };
    let options = WriteOptions {
        indent,
        schema_version_targets,
    };
    if options.schema_version_targets.is_empty() {
        return home
            .read(|document| core_error(otio_core::to_string_with(document, &any, &options)));
    }
    // A downgrade may call back into Python, and that code may touch the
    // objects being written; the document is copied out so that no borrow
    // of it is held across the call.
    let document = home.read(|document| Ok(document.clone()))?;
    with_pending(otio_core::to_string_with(&document, &any, &options))
}

/// Writes a value as a JSON string, downgraded to `schema_version_targets`.
#[pyfunction]
#[pyo3(signature = (value, schema_version_targets, indent))]
fn _serialize_json_to_string(
    value: &Bound<'_, PyAny>,
    schema_version_targets: SchemaVersionMap,
    indent: i64,
) -> PyResult<String> {
    write_value(value, schema_version_targets, string_indent(indent))
}

/// Writes a value to a JSON file, downgraded to `schema_version_targets`.
///
/// Upstream's file writer always indents, by four spaces unless told
/// otherwise, and reports a file it cannot write as the `OSError` for its
/// `errno`.
#[pyfunction]
#[pyo3(signature = (value, filename, schema_version_targets, indent))]
fn _serialize_json_to_file(
    value: &Bound<'_, PyAny>,
    filename: &str,
    schema_version_targets: SchemaVersionMap,
    indent: i64,
) -> PyResult<bool> {
    let indent = usize::try_from(indent).unwrap_or(otio_core::DEFAULT_INDENT);
    let text = write_value(value, schema_version_targets, Some(indent))?;
    std::fs::write(filename, text).map_err(|error| os_error(value.py(), &error, filename))?;
    Ok(true)
}

/// The `OSError` subclass Python raises for an I/O error, filename attached.
fn os_error(py: Python<'_>, error: &std::io::Error, filename: &str) -> PyErr {
    let Some(errno) = error.raw_os_error() else {
        return PyErr::from(std::io::Error::new(error.kind(), error.to_string()));
    };
    // `OSError(errno, strerror, filename)` picks the subclass for `errno`
    // itself — `FileNotFoundError`, `IsADirectoryError` and the rest — as
    // `PyErr_SetFromErrnoWithFilename`, which upstream calls, does.
    let strerror = py
        .import("os")
        .and_then(|os| os.call_method1("strerror", (errno,)))
        .and_then(|text| text.extract::<String>())
        .unwrap_or_else(|_| error.to_string());
    match py
        .get_type::<pyo3::exceptions::PyOSError>()
        .call1((errno, strerror, filename))
    {
        Ok(instance) => PyErr::from_value(instance),
        Err(error) => error,
    }
}

/// Reads JSON into whatever it holds: an object, a list, a dict or a value.
pub fn read_value(py: Python<'_>, input: &str) -> PyResult<Py<PyAny>> {
    let (document, root) = with_pending(otio_core::from_str_any(input))?;
    let shared = Shared::new();
    shared.write(|slot| {
        *slot = document;
        Ok(())
    })?;
    any_to_python(py, &shared, &root)
}

/// Reads JSON text into objects.
#[pyfunction]
fn deserialize_json_from_string(py: Python<'_>, input: &str) -> PyResult<Py<PyAny>> {
    read_value(py, input)
}

/// Reads a JSON file into objects.
#[pyfunction]
fn deserialize_json_from_file(py: Python<'_>, filename: &str) -> PyResult<Py<PyAny>> {
    let text = std::fs::read_to_string(filename).map_err(|error| os_error(py, &error, filename))?;
    read_value(py, &text)
}

/// An object of a schema nobody registered, kept whole.
#[pyclass(
    name = "UnknownSchema",
    module = "opentimelineio._otio",
    extends = PySerializableObject,
    subclass
)]
pub struct PyUnknownSchema;

impl PyUnknownSchema {
    fn with_unknown<T>(
        slf: &PyRef<'_, Self>,
        f: impl FnOnce(&otio_core::schema::UnknownSchema) -> PyResult<T>,
    ) -> PyResult<T> {
        slf.as_super().0.with(|node| match node {
            Node::Unknown(unknown) => f(unknown),
            _ => Err(PyValueError::new_err("not an unknown schema")),
        })
    }
}

#[pymethods]
impl PyUnknownSchema {
    /// A copy of the object's fields: changing it does not change the
    /// object.
    #[getter]
    fn data(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let data = Self::with_unknown(&slf, |unknown| Ok(unknown.data.clone()))?;
        let home = slf.as_super().0.live()?.0;
        let dict = PyDict::new(py);
        for (key, value) in &data {
            dict.set_item(key, any_to_python(py, &home, value)?)?;
        }
        dict.into_py_any(py)
    }

    /// The schema name the object was read with.
    #[getter]
    fn original_schema_name(slf: PyRef<'_, Self>) -> PyResult<String> {
        Self::with_unknown(&slf, |unknown| Ok(unknown.original_schema_name.clone()))
    }

    /// The schema version the object was read with.
    #[getter]
    fn original_schema_version(slf: PyRef<'_, Self>) -> PyResult<u32> {
        Self::with_unknown(&slf, |unknown| Ok(unknown.original_schema_version))
    }
}

/// Upstream's `_testing.test_big_uint`: an unsigned integer too large for a
/// signed one survives being stored in metadata.
#[pyfunction]
fn test_big_uint() -> bool {
    let giant_number = u64::try_from(i64::MAX).unwrap_or_default() + 4;
    let mut document = otio_core::Document::new();
    let id = document.insert(Node::SerializableObjectWithMetadata(
        otio_core::schema::Base::default(),
    ));
    let Some(base) = document.get_mut(id).and_then(Node::base_mut) else {
        return false;
    };
    base.metadata
        .insert("giant_number".to_string(), Any::UInt(giant_number));
    matches!(
        document
            .get(id)
            .and_then(Node::base)
            .and_then(|base| base.metadata.get("giant_number")),
        Some(Any::UInt(value)) if *value == giant_number
    )
}

/// Registers everything here on the extension module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyUnknownSchema>()?;
    module.add_function(wrap_pyfunction!(register_serializable_object_type, module)?)?;
    module.add_function(wrap_pyfunction!(set_type_record, module)?)?;
    module.add_function(wrap_pyfunction!(
        install_external_keepalive_monitor,
        module
    )?)?;
    module.add_function(wrap_pyfunction!(instance_from_schema, module)?)?;
    module.add_function(wrap_pyfunction!(type_version_map, module)?)?;
    module.add_function(wrap_pyfunction!(release_to_schema_version_map, module)?)?;
    module.add_function(wrap_pyfunction!(register_upgrade_function, module)?)?;
    module.add_function(wrap_pyfunction!(register_downgrade_function, module)?)?;
    module.add_function(wrap_pyfunction!(_serialize_json_to_string, module)?)?;
    module.add_function(wrap_pyfunction!(_serialize_json_to_file, module)?)?;
    module.add_function(wrap_pyfunction!(deserialize_json_from_string, module)?)?;
    module.add_function(wrap_pyfunction!(deserialize_json_from_file, module)?)?;

    // Upstream's regression-test hooks live in a `_testing` submodule of
    // each of its two extension modules. There is one extension module here,
    // and `opentime` has already made the submodule, so this joins it.
    let testing = match module.getattr("_testing") {
        Ok(existing) => existing.cast_into::<PyModule>()?,
        Err(_) => {
            let testing = PyModule::new(module.py(), "_testing")?;
            module.add_submodule(&testing)?;
            testing
        }
    };
    testing.add_function(wrap_pyfunction!(test_big_uint, &testing)?)?;
    Ok(())
}

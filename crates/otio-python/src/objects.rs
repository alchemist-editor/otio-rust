//! The object model: the classes `opentimelineio.core` and
//! `opentimelineio.schema` are built from.
//!
//! Each class here wraps a node in a document; see [`crate::arena`] for why,
//! and for the rules every method follows about borrowing.

use otio_core::schema::{Base, Composable, Node};
use otio_core::{Any, AnyDictionary, Error, NodeId};

use pyo3::exceptions::{PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyIterator, PyString};
use pyo3::{IntoPyObjectExt, Py, PyAny};

use crate::arena::Shared;
use crate::values::{any_to_python, python_to_any};

/// Turns an `otio-core` failure into a Python exception.
///
/// Upstream raises a handful of dedicated exception types from
/// `opentimelineio.exceptions`; until those exist here, everything arrives as
/// `ValueError`, which is what its binding layer falls back to.
pub fn core_error<T>(result: Result<T, Error>) -> PyResult<T> {
    result.map_err(|error: Error| PyValueError::new_err(error.to_string()))
}

/// A node in a document, as Python sees it.
///
/// Every class in this module holds one of these and nothing else, so a
/// subclass costs no extra storage and two wrappers for the same node stay in
/// step because neither holds a copy of anything.
#[derive(Clone)]
pub struct Handle {
    pub shared: Shared,
    pub id: NodeId,
}

impl Handle {
    /// Puts `node` in a document of its own and returns a handle to it.
    pub fn alone(node: Node) -> Self {
        let shared = Shared::new();
        let id = shared
            .write(|document| Ok(document.insert(node)))
            .expect("a fresh document cannot be poisoned");
        Self { shared, id }
    }

    /// Runs `f` on the node, for reading.
    pub fn with<T>(&self, f: impl FnOnce(&Node) -> PyResult<T>) -> PyResult<T> {
        self.shared
            .read(|document| f(core_error(document.try_get(self.id))?))
    }

    /// Runs `f` on the node, for writing.
    pub fn with_mut<T>(&self, f: impl FnOnce(&mut Node) -> PyResult<T>) -> PyResult<T> {
        self.shared
            .write(|document| f(core_error(document.try_get_mut(self.id))?))
    }

    /// Runs `f` on the node's name and metadata.
    fn with_base<T>(&self, f: impl FnOnce(&Base) -> PyResult<T>) -> PyResult<T> {
        self.with(|node| {
            let base = node.base().ok_or_else(|| {
                PyValueError::new_err(format!("a {} has no name or metadata", node.schema_name()))
            })?;
            f(base)
        })
    }

    /// Runs `f` on the node's name and metadata, for writing.
    fn with_base_mut<T>(&self, f: impl FnOnce(&mut Base) -> PyResult<T>) -> PyResult<T> {
        self.with_mut(|node| {
            let schema = node.schema_name().to_string();
            let base = node.base_mut().ok_or_else(|| {
                PyValueError::new_err(format!("a {schema} has no name or metadata"))
            })?;
            f(base)
        })
    }
}

/// An object with no fields of its own.
#[pyclass(
    name = "SerializableObject",
    module = "opentimelineio.core",
    subclass,
    weakref
)]
pub struct PySerializableObject(pub Handle);

#[pymethods]
impl PySerializableObject {
    #[new]
    fn new() -> Self {
        Self(Handle::alone(Node::SerializableObject))
    }

    /// The schema name this object serializes under.
    #[getter]
    fn schema_name(&self) -> PyResult<String> {
        self.0.with(|node| Ok(node.schema_name().to_string()))
    }

    /// The schema version this object serializes under.
    #[getter]
    fn schema_version(&self) -> PyResult<u32> {
        self.0.with(|node| Ok(node.schema_version()))
    }

    /// Whether two objects hold the same data.
    ///
    /// Upstream calls this `is_equivalent_to`, and it is what its test
    /// helpers compare with: two objects built separately are equivalent
    /// when they serialize the same, even though they are not the same
    /// object.
    fn is_equivalent_to(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        let Ok(other) = handle_of(other) else {
            return Ok(false);
        };
        Ok(write_one(&self.0)? == write_one(&other)?)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        // Upstream compares wrapped objects by identity, not by value:
        // `is_equivalent_to` is the one that looks at the data.
        handle_of(other).map_or(Ok(false), |other| {
            Ok(self.0.shared.is(&other.shared) && self.0.id == other.id)
        })
    }
}

/// An object carrying a name and metadata.
#[pyclass(
    name = "SerializableObjectWithMetadata",
    module = "opentimelineio.core",
    extends = PySerializableObject,
    subclass
)]
pub struct PySerializableObjectWithMetadata;

#[pymethods]
impl PySerializableObjectWithMetadata {
    #[new]
    #[pyo3(signature = (name = String::new(), metadata = None))]
    fn new(
        name: String,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let base = base_from(name, metadata)?;
        let handle = Handle::alone(Node::SerializableObjectWithMetadata(base));
        Ok(PyClassInitializer::from(PySerializableObject(handle)).add_subclass(Self))
    }

    #[getter]
    fn name(slf: PyRef<'_, Self>) -> PyResult<String> {
        slf.as_super().0.with_base(|base| Ok(base.name.clone()))
    }

    #[setter]
    fn set_name(slf: PyRef<'_, Self>, name: String) -> PyResult<()> {
        slf.as_super().0.with_base_mut(|base| {
            base.name = name;
            Ok(())
        })
    }

    /// The object's metadata, as a mapping that writes through.
    ///
    /// Upstream hands back a live view, so `obj.metadata["k"] = v` changes
    /// the object rather than a copy; its own tests do exactly that.
    #[getter]
    fn metadata(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let handle = slf.as_super().0.clone();
        PyMetadata(handle).into_py_any(py)
    }

    #[setter]
    fn set_metadata(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let entries = dictionary_from(value)?;
        slf.as_super().0.with_base_mut(|base| {
            base.metadata = entries;
            Ok(())
        })
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = &slf.as_super().0;
        Ok(format!(
            "SerializableObjectWithMetadata({}, {})",
            name_str(handle)?,
            metadata_repr(handle, py)?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = &slf.as_super().0;
        Ok(format!(
            "otio.core.SerializableObjectWithMetadata(name={}, metadata={})",
            name_repr(py, handle)?,
            metadata_repr(handle, py)?
        ))
    }
}

/// Something that can sit in a composition.
#[pyclass(
    name = "Composable",
    module = "opentimelineio.core",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PyComposable;

#[pymethods]
impl PyComposable {
    #[new]
    #[pyo3(signature = (name = String::new(), metadata = None))]
    fn new(
        name: String,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let base = base_from(name, metadata)?;
        let handle = Handle::alone(Node::Composable(Composable { base, parent: None }));
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    /// Whether this object is visible when its composition is flattened.
    fn visible(slf: PyRef<'_, Self>) -> PyResult<bool> {
        slf.as_super().as_super().0.with(|node| Ok(node.visible()))
    }

    /// Whether this object overlaps the ones beside it, as a transition does.
    fn overlapping(slf: PyRef<'_, Self>) -> PyResult<bool> {
        slf.as_super()
            .as_super()
            .0
            .with(|node| Ok(node.overlapping()))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = &slf.as_super().as_super().0;
        Ok(format!(
            "Composable({}, {})",
            name_str(handle)?,
            metadata_repr(handle, py)?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = &slf.as_super().as_super().0;
        Ok(format!(
            "otio.core.Composable(name={}, metadata={})",
            name_repr(py, handle)?,
            metadata_repr(handle, py)?
        ))
    }
}

/// A live view of one object's metadata.
///
/// It holds the object, not a copy of its metadata, so every read and write
/// goes to the document. `_core_utils.py` upstream does the same thing by
/// grafting `MutableMapping` onto a C++ type; here the methods are written
/// out and the Python layer registers the class with `MutableMapping`.
#[pyclass(name = "AnyDictionaryProxy", module = "opentimelineio.core")]
pub struct PyMetadata(Handle);

#[pymethods]
impl PyMetadata {
    fn __getitem__(&self, py: Python<'_>, key: &str) -> PyResult<Py<PyAny>> {
        self.0.with(|node| {
            let value = node
                .base()
                .and_then(|base| base.metadata.get(key))
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))?;
            any_to_python(py, value)
        })
    }

    fn __setitem__(&self, key: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let value = python_to_any(value)?;
        self.0.with_base_mut(|base| {
            base.metadata.insert(key.to_string(), value);
            Ok(())
        })
    }

    fn __delitem__(&self, key: &str) -> PyResult<()> {
        self.0.with_base_mut(|base| {
            base.metadata
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))
        })
    }

    fn __len__(&self) -> PyResult<usize> {
        self.0
            .with(|node| Ok(node.base().map_or(0, |base| base.metadata.len())))
    }

    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let keys: Vec<String> = self.0.with(|node| {
            Ok(node
                .base()
                .map(|base| base.metadata.keys().cloned().collect())
                .unwrap_or_default())
        })?;
        let list = keys.into_py_any(py)?;
        PyIterator::from_object(list.bind(py))?.into_py_any(py)
    }

    fn __contains__(&self, key: &str) -> PyResult<bool> {
        self.0.with(|node| {
            Ok(node
                .base()
                .is_some_and(|base| base.metadata.contains_key(key)))
        })
    }

    /// Returns this metadata copied into an ordinary dictionary.
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        self.0.with(|node| {
            if let Some(base) = node.base() {
                for (key, value) in &base.metadata {
                    dict.set_item(key, any_to_python(py, value)?)?;
                }
            }
            Ok(())
        })?;
        dict.into_py_any(py)
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.to_dict(py)?.bind(py).eq(other)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.to_dict(py)?.bind(py).repr()?.to_string())
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        self.__repr__(py)
    }
}

/// Returns the handle inside any wrapped object.
fn handle_of(value: &Bound<'_, PyAny>) -> PyResult<Handle> {
    Ok(value
        .extract::<PyRef<'_, PySerializableObject>>()?
        .0
        .clone())
}

/// Builds a name-and-metadata block from constructor arguments.
fn base_from(name: String, metadata: Option<&Bound<'_, PyAny>>) -> PyResult<Base> {
    Ok(Base {
        name,
        metadata: metadata
            .map(dictionary_from)
            .transpose()?
            .unwrap_or_default(),
    })
}

/// Reads a Python mapping into a metadata dictionary.
fn dictionary_from(value: &Bound<'_, PyAny>) -> PyResult<AnyDictionary> {
    match python_to_any(value)? {
        Any::Dictionary(entries) => Ok(entries),
        other => Err(PyValueError::new_err(format!(
            "metadata must be a mapping, not a {}",
            other.type_name()
        ))),
    }
}

/// Renders an object's name the way Python's `str()` would.
fn name_str(handle: &Handle) -> PyResult<String> {
    handle.with(|node| Ok(node.name().to_string()))
}

/// Renders an object's name the way Python's `repr()` would, quotes and all.
///
/// `__str__` and `__repr__` differ here and nowhere else in these classes:
/// upstream builds one from `str(self.name)` and the other from
/// `repr(self.name)`, and its own test compares both.
fn name_repr(py: Python<'_>, handle: &Handle) -> PyResult<String> {
    Ok(PyString::new(py, &name_str(handle)?).repr()?.to_string())
}

/// Renders an object's metadata the way Python's `str()` would.
fn metadata_repr(handle: &Handle, py: Python<'_>) -> PyResult<String> {
    PyMetadata(handle.clone()).__repr__(py)
}

/// Serializes one object, for comparing two of them.
fn write_one(handle: &Handle) -> PyResult<String> {
    handle.shared.read(|document| {
        core_error(otio_core::to_string_pretty_from(
            document,
            handle.id,
            otio_core::DEFAULT_INDENT,
        ))
    })
}

/// Registers the object model on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PySerializableObject>()?;
    module.add_class::<PySerializableObjectWithMetadata>()?;
    module.add_class::<PyComposable>()?;
    module.add_class::<PyMetadata>()?;
    Ok(())
}

/// Reads a document from JSON and returns its root object.
pub fn read_from_string(py: Python<'_>, json: &str) -> PyResult<Py<PyAny>> {
    let document = core_error(otio_core::from_str(json))?;
    let root = document
        .root()
        .ok_or_else(|| PyValueError::new_err("the document has no root object"))?;
    let shared = Shared::new();
    shared.write(|slot| {
        *slot = document;
        Ok(())
    })?;
    Ok(wrap(py, &Handle { shared, id: root })?.unbind())
}

/// Writes one object out as JSON.
pub fn write_to_string(value: &Bound<'_, PyAny>, indent: usize) -> PyResult<String> {
    let handle = handle_of(value)?;
    handle.shared.read(|document| {
        core_error(otio_core::to_string_pretty_from(
            document, handle.id, indent,
        ))
    })
}

/// Builds the Python wrapper for a node, reusing the one it already has.
pub fn wrap<'py>(py: Python<'py>, handle: &Handle) -> PyResult<Bound<'py, PyAny>> {
    let shared = handle.shared.clone();
    let id = handle.id;
    shared.clone().wrapper_for(py, id, || {
        let handle = Handle { shared, id };
        let schema = handle.with(|node| Ok(node.schema_name().to_string()))?;
        let object = PySerializableObject(handle);
        match schema.as_str() {
            "Composable" => Ok(Py::new(
                py,
                PyClassInitializer::from(object)
                    .add_subclass(PySerializableObjectWithMetadata)
                    .add_subclass(PyComposable),
            )?
            .into_bound_py_any(py)?),
            "SerializableObjectWithMetadata" => Ok(Py::new(
                py,
                PyClassInitializer::from(object).add_subclass(PySerializableObjectWithMetadata),
            )?
            .into_bound_py_any(py)?),
            _ => Ok(Py::new(py, object)?.into_bound_py_any(py)?),
        }
    })
}

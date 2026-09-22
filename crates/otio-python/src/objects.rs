//! The object model: the classes `opentimelineio.core` and
//! `opentimelineio.schema` are built from.
//!
//! Each class here wraps a node in a document; see [`crate::arena`] for why,
//! and for the rules every method follows about borrowing.

use opentime::{RationalTime, TimeRange};

use std::collections::BTreeMap;

use otio_core::schema::{
    Base, Clip, Composable, Composition, EffectData, ExternalReference, Gap, GeneratorReference,
    ImageSequenceReference, ItemData, Marker, MediaReferenceData, MissingFramePolicy,
    MissingReference, Node, SerializableCollection, Stack, Timeline, Track, Transition,
};
use otio_core::{Any, AnyDictionary, Error, NeighborGapPolicy, NodeId};

use pyo3::exceptions::{
    PyIndexError, PyKeyError, PyNotImplementedError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyIterator, PyList, PyString, PyTuple, PyType};
use pyo3::{IntoPyObject, IntoPyObjectExt, Py, PyAny};

use crate::arena::Shared;
use crate::errors::{CannotComputeAvailableRangeError, NotAChildError, UnsupportedSchemaError};
use crate::opentime::{PyRationalTime, PyTimeRange};
use crate::values::{PyBox2d, PyColor, any_to_python, python_to_any};

/// Turns an `otio-core` failure into a Python exception.
///
/// Which exception matters: upstream's own tests catch several of these by
/// type rather than by message, so the mapping below follows its
/// `ErrorStatusHandler` case for case. Anything with no dedicated type
/// becomes `ValueError`, which is upstream's fallback too.
pub fn core_error<T>(result: Result<T, Error>) -> PyResult<T> {
    result.map_err(|error: Error| {
        // Upstream appends the `str()` of the object an error concerns. The
        // error names it by handle; the document it belongs to is the one
        // being borrowed when the error arose.
        let object = error
            .object()
            .and_then(|id| Shared::borrowed().map(|shared| Handle { shared, id }));
        exception(error, object)
    })
}

/// Turns an `otio-core` failure about `object` into a Python exception, as
/// [`core_error`] does.
fn exception(error: Error, object: Option<Handle>) -> PyErr {
    let text = ErrorText {
        message: error.to_string(),
        object,
    };
    match error {
        Error::UnsupportedSchemaVersion { .. } => UnsupportedSchemaError::new_err(text),
        // A Python version function's own exception, when there is one
        // to raise, is raised by `registry::with_pending` instead.
        Error::VersionFunctionFailed { .. } => {
            crate::registry::take_pending_error().unwrap_or_else(|| PyValueError::new_err(text))
        }
        // Upstream's base classes leave some questions to their
        // subclasses and report NOT_IMPLEMENTED for them.
        Error::NotImplemented { .. } | Error::NoLayout => PyNotImplementedError::new_err(text),
        Error::NotAChild { .. } | Error::NotAChildOf { .. } | Error::NotDescendedFrom { .. } => {
            NotAChildError::new_err(text)
        }
        Error::NoAvailableRange { .. } => CannotComputeAvailableRangeError::new_err(text),
        Error::IllegalIndex { .. } | Error::NoImagesInSequence { .. } => {
            PyIndexError::new_err(text)
        }
        _ => PyValueError::new_err(text),
    }
}

/// An exception's message: the error's text and, as upstream's
/// `ErrorStatusHandler` gives it, `": "` and the `str()` of the object it
/// concerns.
///
/// The `str()` is taken only when Python asks for the exception's value,
/// which is after the call that failed has returned and let go of the
/// document; `str()` reads the object, and so could not be taken before.
struct ErrorText {
    message: String,
    object: Option<Handle>,
}

impl pyo3::PyErrArguments for ErrorText {
    fn arguments(self, py: Python<'_>) -> Py<PyAny> {
        let described = self
            .object
            // Were the document still borrowed on this thread, reading the
            // object would deadlock; better a message without it.
            .filter(|handle| !handle.shared.is_borrowed())
            .and_then(|handle| wrap(py, &handle).and_then(|object| object.str()).ok())
            .map(|text| format!("{}: {text}", self.message));
        let message = described.unwrap_or(self.message);
        PyString::new(py, &message).into_any().unbind()
    }
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

    /// Returns where this node lives now.
    ///
    /// A handle is only meaningful in the arena it came from, and appending
    /// an object to another moves it; see [`crate::arena`]. Every access goes
    /// through here so that a wrapper taken before the move still works.
    pub fn live(&self) -> PyResult<(Shared, NodeId)> {
        self.shared.translate(self.id)
    }

    /// Runs `f` on the node, for reading.
    pub fn with<T>(&self, f: impl FnOnce(&Node) -> PyResult<T>) -> PyResult<T> {
        let (shared, id) = self.live()?;
        shared.read(|document| f(core_error(document.try_get(id))?))
    }

    /// Runs `f` on the node, for writing.
    pub fn with_mut<T>(&self, f: impl FnOnce(&mut Node) -> PyResult<T>) -> PyResult<T> {
        let (shared, id) = self.live()?;
        shared.write(|document| f(core_error(document.try_get_mut(id))?))
    }

    /// Returns a handle to another node in the same document.
    ///
    /// `id` must be one read out of this node while it was borrowed, so it is
    /// an id in the *current* document. Pairing it with `self.shared` would
    /// be wrong whenever this handle has been forwarded: that field still
    /// names the document the object started in, and translating a current id
    /// through an old forwarding map can land on a different object
    /// altogether. This resolves first, so the pair always agree.
    pub fn sibling(&self, id: NodeId) -> PyResult<Self> {
        let (shared, _) = self.live()?;
        Ok(Self { shared, id })
    }

    /// Returns whether two handles name the same object.
    pub fn same(&self, other: &Self) -> PyResult<bool> {
        let (here, id) = self.live()?;
        let (there, other_id) = other.live()?;
        Ok(here.is(&there)? && id == other_id)
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
///
/// `dict` is upstream's `py::dynamic_attr()`: every one of its classes takes
/// arbitrary Python attributes, and its own tests set one
/// (`gap._serializable_label = "Filler.1"`).
#[pyclass(
    name = "SerializableObject",
    module = "opentimelineio._otio",
    subclass,
    weakref,
    dict
)]
pub struct PySerializableObject(pub Handle);

#[pymethods]
impl PySerializableObject {
    /// Builds an empty object.
    ///
    /// A Python subclass's arguments are its `__init__`'s business, as they
    /// are under pybind11, whose constructors live in `__init__` rather than
    /// `__new__`: upstream's own plugin classes take arguments of their own
    /// and pass none on. Only this class itself refuses them.
    #[new]
    #[classmethod]
    #[pyo3(signature = (*args, **kwargs))]
    fn new(
        cls: &Bound<'_, PyType>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let exact = cls.is(cls.py().get_type::<Self>());
        if exact && (!args.is_empty() || kwargs.is_some_and(|kwargs| !kwargs.is_empty())) {
            return Err(PyTypeError::new_err(
                "SerializableObject() takes no arguments",
            ));
        }
        Ok(Self(Handle::alone(Node::SerializableObject)))
    }

    /// Records this wrapper as the one for its node.
    ///
    /// Python calls `__init__` after `__new__`, and `__new__` is where every
    /// constructor here builds the object; this is the first point at which
    /// the Python object exists to be remembered. Every class in this module
    /// inherits it, so every constructor registers.
    ///
    /// The arguments are ignored: each subclass's `__new__` has already read
    /// them.
    ///
    /// The one exception is a Python subclass of
    /// `SerializableObjectWithMetadata`, whose `__new__` left the name and
    /// metadata alone for its own `__init__` to deal with; if that `__init__`
    /// passes them on, or there is none, they are taken here, as pybind11's
    /// `__init__` takes them upstream.
    #[pyo3(signature = (*args, **kwargs))]
    fn __init__(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let handle = slf.borrow().0.clone();
        let given = !args.is_empty() || kwargs.is_some_and(|kwargs| !kwargs.is_empty());
        let exact = slf
            .get_type()
            .is(slf.py().get_type::<PySerializableObjectWithMetadata>());
        if given && !exact {
            let takes_name = handle.with(|node| {
                Ok(matches!(
                    node,
                    Node::SerializableObjectWithMetadata(_)
                        | Node::Dynamic(otio_core::schema::DynamicObject { base: Some(_), .. })
                ))
            })?;
            if takes_name {
                let (name, metadata) = name_and_metadata(args, kwargs)?;
                let entries = match metadata {
                    Some(metadata) if !metadata.is_none() => {
                        dictionary_from(&handle.shared, &metadata)?
                    }
                    _ => AnyDictionary::new(),
                };
                handle.with_base_mut(|base| {
                    base.name = name;
                    base.metadata = entries;
                    Ok(())
                })?;
            }
        }
        let (shared, id) = handle.live()?;
        shared.remember(id, slf.as_any())
    }

    /// The schema name this object serializes under.
    ///
    /// An unknown schema answers `UnknownSchema`, as upstream's does: the
    /// name it was read with is its `original_schema_name`.
    fn schema_name(&self) -> PyResult<String> {
        self.0.with(|node| {
            Ok(match node {
                Node::Unknown(_) => "UnknownSchema".to_string(),
                _ => node.schema_name().to_string(),
            })
        })
    }

    /// The schema version this object serializes under.
    fn schema_version(&self) -> PyResult<u32> {
        self.0.with(|node| {
            Ok(match node {
                Node::Unknown(_) => 1,
                _ => node.schema_version(),
            })
        })
    }

    /// Whether this object's schema is one nobody registered.
    #[getter]
    fn is_unknown_schema(&self) -> PyResult<bool> {
        self.0.with(|node| Ok(matches!(node, Node::Unknown(_))))
    }

    /// The fields this object carries beyond its class's own, as a mapping
    /// that writes through.
    ///
    /// Upstream gives every object these; `serializable_field` properties
    /// keep their values here. Here only upstream's two root classes and
    /// schemas registered from Python hold them: any other object reads as
    /// having none, and refuses one being set.
    #[getter]
    fn _dynamic_fields(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        PyMetadata {
            handle: self.0.clone(),
            which: Bag::Dynamic,
            path: Vec::new(),
        }
        .into_py_any(py)
    }

    /// Writes this object as a JSON string.
    #[pyo3(signature = (indent = 4))]
    fn to_json_string(slf: &Bound<'_, Self>, indent: i64) -> PyResult<String> {
        let serialize = slf
            .py()
            .import("opentimelineio.core")?
            .getattr("serialize_json_to_string")?;
        serialize.call1((slf, slf.py().None(), indent))?.extract()
    }

    /// Writes this object to a JSON file.
    #[pyo3(signature = (file_name, indent = 4))]
    fn to_json_file(slf: &Bound<'_, Self>, file_name: &str, indent: i64) -> PyResult<bool> {
        let serialize = slf
            .py()
            .import("opentimelineio.core")?
            .getattr("serialize_json_to_file")?;
        serialize
            .call1((slf, file_name, slf.py().None(), indent))?
            .extract()
    }

    /// Reads an object from a JSON string.
    #[staticmethod]
    fn from_json_string(py: Python<'_>, input: &str) -> PyResult<Py<PyAny>> {
        crate::registry::read_value(py, input)
    }

    /// Reads an object from a JSON file.
    #[staticmethod]
    fn from_json_file(py: Python<'_>, file_name: &str) -> PyResult<Py<PyAny>> {
        py.import("opentimelineio.core")?
            .getattr("deserialize_json_from_file")?
            .call1((file_name,))
            .map(Bound::unbind)
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
        handle_of(other).map_or(Ok(false), |other| self.0.same(&other))
    }

    /// Returns a copy of this object and everything below it.
    ///
    /// The copy has no parent, as upstream's does not: it is a new object,
    /// not a second reference to this one in the same composition. It is
    /// made as upstream's `clone` makes it, so an object held in two places
    /// is copied twice, and an object that holds itself is a `ValueError`.
    fn deepcopy(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let (shared, id) = self.0.live()?;
        let copy = shared.write(|document| core_error(document.clone_object(id)))?;
        Ok(wrap(py, &Handle { shared, id: copy })?.unbind())
    }

    /// As [`Self::deepcopy`]. Not one of upstream's methods, but kept for
    /// code written against this port before it was brought in line.
    fn copy(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.deepcopy(py)
    }

    /// As [`Self::deepcopy`]. Upstream's C++ name for the same thing.
    fn clone(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.deepcopy(py)
    }

    #[pyo3(signature = (_memo = None))]
    fn __deepcopy__(
        &self,
        py: Python<'_>,
        _memo: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        self.deepcopy(py)
    }

    /// Refused, as upstream refuses it: an object owns what it holds, so a
    /// copy that shared it would put one object in two places.
    fn __copy__(&self) -> PyResult<Py<PyAny>> {
        Err(PyValueError::new_err(
            "SerializableObjects may not be shallow copied.",
        ))
    }
}

/// An object carrying a name and metadata.
#[pyclass(
    name = "SerializableObjectWithMetadata",
    module = "opentimelineio._otio",
    extends = PySerializableObject,
    subclass
)]
pub struct PySerializableObjectWithMetadata;

#[pymethods]
impl PySerializableObjectWithMetadata {
    /// Builds an object with a name and metadata.
    ///
    /// For a Python subclass the arguments are left to its `__init__`; see
    /// [`PySerializableObject::new`].
    #[new]
    #[classmethod]
    #[pyo3(signature = (*args, **kwargs))]
    fn new(
        cls: &Bound<'_, PyType>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let (name, metadata) = if cls.is(cls.py().get_type::<Self>()) {
            name_and_metadata(args, kwargs)?
        } else {
            (String::new(), None)
        };
        let metadata = metadata.filter(|metadata| !metadata.is_none());
        let handle = alone_with(
            Node::SerializableObjectWithMetadata,
            name,
            metadata.as_ref(),
        )?;
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
        PyMetadata::of(handle).into_py_any(py)
    }

    #[setter]
    fn set_metadata(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let entries = dictionary_from(&slf.as_super().0.shared, value)?;
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
    module = "opentimelineio._otio",
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
        let handle = alone_with(
            |base| Node::Composable(Composable { base, parent: None }),
            name,
            metadata,
        )?;
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    /// Whether this object is visible when its composition is flattened.
    fn visible(slf: PyRef<'_, Self>) -> PyResult<bool> {
        slf.as_super().as_super().0.with(|node| Ok(node.visible()))
    }

    /// The composition holding this object, if any.
    fn parent(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let handle = slf.as_super().as_super().0.clone();
        let parent = handle.with(|node| Ok(node.parent()))?;
        match parent {
            None => Ok(py.None()),
            Some(id) => Ok(wrap(py, &handle.sibling(id)?)?.unbind()),
        }
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

/// Something that sits in time, with a source range, effects and markers.
#[pyclass(
    name = "Item",
    module = "opentimelineio._otio",
    extends = PyComposable,
    subclass
)]
pub struct PyItem;

/// The handle under an `Item` or one of its subclasses.
///
/// The three `as_super()` hops are the inheritance chain upstream has:
/// `Item` is a `Composable` is a `SerializableObjectWithMetadata` is a
/// `SerializableObject`, and only the last of those holds anything.
fn item_handle(slf: &PyRef<'_, PyItem>) -> Handle {
    slf.as_super().as_super().as_super().0.clone()
}

#[pymethods]
impl PyItem {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        source_range = None,
        effects = None,
        markers = None,
        enabled = true,
        color = None,
        metadata = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        source_range: Option<PyTimeRange>,
        effects: Option<&Bound<'_, PyAny>>,
        markers: Option<&Bound<'_, PyAny>>,
        enabled: bool,
        color: Option<PyColor>,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_item(
            Node::Item,
            name,
            source_range,
            effects,
            markers,
            enabled,
            color,
            metadata,
        )?;
        Ok(composable_initializer(handle).add_subclass(Self))
    }

    #[getter]
    fn source_range(slf: PyRef<'_, Self>) -> PyResult<Option<PyTimeRange>> {
        item_handle(&slf).with(|node| {
            Ok(node
                .item()
                .and_then(|item| item.source_range)
                .map(PyTimeRange))
        })
    }

    #[setter]
    fn set_source_range(slf: PyRef<'_, Self>, range: Option<PyTimeRange>) -> PyResult<()> {
        with_item_mut(&item_handle(&slf), |item| {
            item.source_range = range.map(|range| range.0);
            Ok(())
        })
    }

    #[getter]
    fn enabled(slf: PyRef<'_, Self>) -> PyResult<bool> {
        item_handle(&slf).with(|node| Ok(node.item().is_some_and(|item| item.enabled)))
    }

    #[setter]
    fn set_enabled(slf: PyRef<'_, Self>, enabled: bool) -> PyResult<()> {
        with_item_mut(&item_handle(&slf), |item| {
            item.enabled = enabled;
            Ok(())
        })
    }

    /// A display tint for editorial tools.
    #[getter]
    fn color(slf: PyRef<'_, Self>) -> PyResult<Option<PyColor>> {
        item_handle(&slf)
            .with(|node| Ok(node.item().and_then(|item| item.color.clone()).map(PyColor)))
    }

    #[setter]
    fn set_color(slf: PyRef<'_, Self>, color: Option<PyColor>) -> PyResult<()> {
        with_item_mut(&item_handle(&slf), |item| {
            item.color = color.map(|color| color.0);
            Ok(())
        })
    }

    #[getter]
    fn effects(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        PyNodeList {
            handle: item_handle(&slf),
            which: Which::Effects,
        }
        .into_py_any(py)
    }

    #[getter]
    fn markers(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        PyNodeList {
            handle: item_handle(&slf),
            which: Which::Markers,
        }
        .into_py_any(py)
    }

    /// How long this item lasts once its trim is taken into account.
    fn duration(slf: PyRef<'_, Self>) -> PyResult<PyRationalTime> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared.read(|document| Ok(PyRationalTime(core_error(document.duration(id))?)))
    }

    /// The full span of media behind this item, ignoring any trim.
    fn available_range(slf: PyRef<'_, Self>) -> PyResult<PyTimeRange> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared.read(|document| Ok(PyTimeRange(core_error(document.available_range(id))?)))
    }

    /// The part of the media this item actually uses.
    fn trimmed_range(slf: PyRef<'_, Self>) -> PyResult<PyTimeRange> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared.read(|document| Ok(PyTimeRange(core_error(document.trimmed_range(id))?)))
    }

    /// The part of the media an audience actually sees, once the effects of
    /// any transitions either side are taken into account.
    fn visible_range(slf: PyRef<'_, Self>) -> PyResult<PyTimeRange> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared.read(|document| Ok(PyTimeRange(core_error(document.visible_range(id))?)))
    }

    /// Where this item sits in its parent's clock.
    fn range_in_parent(slf: PyRef<'_, Self>) -> PyResult<PyTimeRange> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared.read(|document| Ok(PyTimeRange(core_error(document.range_in_parent(id))?)))
    }

    /// Where this item sits in its parent's clock, trimmed to the parent's
    /// own source range.
    fn trimmed_range_in_parent(slf: PyRef<'_, Self>) -> PyResult<Option<PyTimeRange>> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared
            .read(|document| Ok(core_error(document.trimmed_range_in_parent(id))?.map(PyTimeRange)))
    }

    /// Restates `time`, which is in this item's clock, in `to_item`'s.
    fn transformed_time(
        slf: PyRef<'_, Self>,
        time: PyRationalTime,
        to_item: &Bound<'_, PyAny>,
    ) -> PyResult<PyRationalTime> {
        let (shared, from, to) = pair(&item_handle(&slf), to_item, not_descended_from)?;
        shared.read(|document| {
            Ok(PyRationalTime(core_error(
                document.transformed_time(time.0, from, to),
            )?))
        })
    }

    /// Restates `time_range`, which is in this item's clock, in `to_item`'s.
    fn transformed_time_range(
        slf: PyRef<'_, Self>,
        time_range: PyTimeRange,
        to_item: &Bound<'_, PyAny>,
    ) -> PyResult<PyTimeRange> {
        let (shared, from, to) = pair(&item_handle(&slf), to_item, not_descended_from)?;
        shared.read(|document| {
            Ok(PyTimeRange(core_error(document.transformed_time_range(
                time_range.0,
                from,
                to,
            ))?))
        })
    }

    /// The image bounds of the media behind this item, if known.
    #[getter]
    fn available_image_bounds(slf: PyRef<'_, Self>) -> PyResult<Option<PyBox2d>> {
        let handle = item_handle(&slf);
        let (shared, id) = handle.live()?;
        shared.read(|document| Ok(core_error(document.available_image_bounds(id))?.map(PyBox2d)))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        item_str(py, "Item", &item_handle(&slf))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        item_repr(py, "otio.core.Item", &item_handle(&slf))
    }
}

/// An empty span of time.
#[pyclass(
    name = "Gap",
    module = "opentimelineio._otio",
    extends = PyItem,
    subclass
)]
pub struct PyGap;

#[pymethods]
impl PyGap {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        duration = None,
        source_range = None,
        effects = None,
        markers = None,
        metadata = None,
        enabled = true,
    ))]
    fn new(
        name: String,
        duration: Option<PyRationalTime>,
        source_range: Option<PyTimeRange>,
        effects: Option<&Bound<'_, PyAny>>,
        markers: Option<&Bound<'_, PyAny>>,
        metadata: Option<&Bound<'_, PyAny>>,
        enabled: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        // Upstream offers two ways to say how long a gap is and refuses both
        // at once, because they could disagree.
        let range = match (duration, source_range) {
            (Some(_), Some(_)) => {
                return Err(PyTypeError::new_err(
                    "Cannot instantiate Gap with both a source_range and a duration",
                ));
            }
            (Some(duration), None) => Some(TimeRange::new(
                RationalTime::new(0.0, duration.0.rate()),
                duration.0,
            )),
            // Upstream's constructor defaults the range to an empty one
            // rather than to none, and writes it out as such.
            (None, range) => Some(range.map_or_else(TimeRange::default, |range| range.0)),
        };

        let handle = new_item(
            |item| Node::Gap(Gap { item }),
            name,
            range.map(PyTimeRange),
            effects,
            markers,
            enabled,
            None,
            metadata,
        )?;
        Ok(composable_initializer(handle)
            .add_subclass(PyItem)
            .add_subclass(Self))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        item_str(py, "Gap", &item_handle(&slf.into_super()))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        item_repr(py, "otio.schema.Gap", &item_handle(&slf.into_super()))
    }
}

/// A labelled point or span on an item.
#[pyclass(
    name = "Marker",
    module = "opentimelineio._otio",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PyMarker;

/// The handle under a `Marker` or an `Effect`.
fn metadata_handle<T>(slf: &PyRef<'_, T>) -> Handle
where
    T: pyo3::PyClass<BaseType = PySerializableObjectWithMetadata>,
{
    slf.as_super().as_super().0.clone()
}

#[pymethods]
impl PyMarker {
    #[new]
    // Upstream's constructor defaults the colour to red, though its C++
    // defaults it to green; an explicit `None` leaves it unset.
    #[pyo3(signature = (
        name = String::new(),
        marked_range = None,
        color = Some(PyColor(otio_core::Color::red())),
        metadata = None,
        comment = String::new(),
    ))]
    fn new(
        name: String,
        marked_range: Option<PyTimeRange>,
        color: Option<PyColor>,
        metadata: Option<&Bound<'_, PyAny>>,
        comment: String,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            |base| {
                Node::Marker(Marker {
                    base,
                    color: color.map(|color| color.0),
                    marked_range: marked_range.map(|range| range.0).unwrap_or_default(),
                    comment,
                })
            },
            name,
            metadata,
        )?;
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    #[getter]
    fn marked_range(slf: PyRef<'_, Self>) -> PyResult<PyTimeRange> {
        metadata_handle(&slf).with(|node| match node {
            Node::Marker(marker) => Ok(PyTimeRange(marker.marked_range)),
            _ => Err(PyValueError::new_err("not a marker")),
        })
    }

    #[setter]
    fn set_marked_range(slf: PyRef<'_, Self>, range: PyTimeRange) -> PyResult<()> {
        metadata_handle(&slf).with_mut(|node| match node {
            Node::Marker(marker) => {
                marker.marked_range = range.0;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a marker")),
        })
    }

    #[getter]
    fn comment(slf: PyRef<'_, Self>) -> PyResult<String> {
        metadata_handle(&slf).with(|node| match node {
            Node::Marker(marker) => Ok(marker.comment.clone()),
            _ => Err(PyValueError::new_err("not a marker")),
        })
    }

    #[setter]
    fn set_comment(slf: PyRef<'_, Self>, comment: String) -> PyResult<()> {
        metadata_handle(&slf).with_mut(|node| match node {
            Node::Marker(marker) => {
                marker.comment = comment;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a marker")),
        })
    }

    #[getter]
    fn color(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        metadata_handle(&slf).with(|node| match node {
            Node::Marker(marker) => marker
                .color
                .clone()
                .map_or_else(|| Ok(py.None()), |color| PyColor(color).into_py_any(py)),
            _ => Err(PyValueError::new_err("not a marker")),
        })
    }

    #[setter]
    fn set_color(slf: PyRef<'_, Self>, color: Option<PyColor>) -> PyResult<()> {
        metadata_handle(&slf).with_mut(|node| match node {
            Node::Marker(marker) => {
                marker.color = color.map(|color| color.0);
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a marker")),
        })
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = metadata_handle(&slf);
        let (range, color) = marker_parts(py, &handle)?;
        Ok(format!(
            "Marker({}, {}, {}, {})",
            name_str(&handle)?,
            range.bind(py).str()?,
            color.bind(py).str()?,
            metadata_repr(&handle, py)?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = metadata_handle(&slf);
        let (range, color) = marker_parts(py, &handle)?;
        Ok(format!(
            "otio.schema.Marker(name={}, marked_range={}, color={}, metadata={})",
            name_repr(py, &handle)?,
            range.bind(py).repr()?,
            color.bind(py).repr()?,
            metadata_repr(&handle, py)?
        ))
    }
}

/// An alteration applied to an item.
#[pyclass(
    name = "Effect",
    module = "opentimelineio._otio",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PyEffect;

#[pymethods]
impl PyEffect {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        effect_name = String::new(),
        metadata = None,
        enabled = true,
    ))]
    fn new(
        name: String,
        effect_name: String,
        metadata: Option<&Bound<'_, PyAny>>,
        enabled: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            |base| {
                Node::Effect(EffectData {
                    base,
                    effect_name,
                    enabled,
                })
            },
            name,
            metadata,
        )?;
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    #[getter]
    fn effect_name(slf: PyRef<'_, Self>) -> PyResult<String> {
        with_effect(&metadata_handle(&slf), |effect| {
            Ok(effect.effect_name.clone())
        })
    }

    #[setter]
    fn set_effect_name(slf: PyRef<'_, Self>, effect_name: String) -> PyResult<()> {
        with_effect_mut(&metadata_handle(&slf), |effect| {
            effect.effect_name = effect_name;
            Ok(())
        })
    }

    #[getter]
    fn enabled(slf: PyRef<'_, Self>) -> PyResult<bool> {
        with_effect(&metadata_handle(&slf), |effect| Ok(effect.enabled))
    }

    #[setter]
    fn set_enabled(slf: PyRef<'_, Self>, enabled: bool) -> PyResult<()> {
        with_effect_mut(&metadata_handle(&slf), |effect| {
            effect.enabled = enabled;
            Ok(())
        })
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = metadata_handle(&slf);
        Ok(format!(
            "Effect({}, {}, {}, {})",
            name_str(&handle)?,
            effect_name_of(&handle)?,
            metadata_repr(&handle, py)?,
            if Self::enabled(slf)? { "True" } else { "False" }
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = metadata_handle(&slf);
        Ok(format!(
            "otio.schema.Effect(name={}, effect_name={}, metadata={}, enabled={})",
            name_repr(py, &handle)?,
            py_repr(py, &effect_name_of(&handle)?)?,
            metadata_repr(&handle, py)?,
            if Self::enabled(slf)? { "True" } else { "False" }
        ))
    }
}

/// The base class of every effect that changes an item's timing.
#[pyclass(
    name = "TimeEffect",
    module = "opentimelineio._otio",
    extends = PyEffect,
    subclass
)]
pub struct PyTimeEffect;

#[pymethods]
impl PyTimeEffect {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        effect_name = String::new(),
        metadata = None,
        enabled = true,
    ))]
    fn new(
        name: String,
        effect_name: String,
        metadata: Option<&Bound<'_, PyAny>>,
        enabled: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            |base| {
                Node::TimeEffect(EffectData {
                    base,
                    effect_name,
                    enabled,
                })
            },
            name,
            metadata,
        )?;
        Ok(effect_initializer(handle).add_subclass(Self))
    }
}

/// A constant-rate speed change.
#[pyclass(
    name = "LinearTimeWarp",
    module = "opentimelineio._otio",
    extends = PyTimeEffect,
    subclass
)]
pub struct PyLinearTimeWarp;

#[pymethods]
impl PyLinearTimeWarp {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        time_scalar = 1.0,
        metadata = None,
        enabled = true,
    ))]
    fn new(
        name: String,
        time_scalar: f64,
        metadata: Option<&Bound<'_, PyAny>>,
        enabled: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            |base| Node::LinearTimeWarp {
                effect: EffectData {
                    base,
                    effect_name: "LinearTimeWarp".to_string(),
                    enabled,
                },
                time_scalar,
            },
            name,
            metadata,
        )?;
        Ok(effect_initializer(handle)
            .add_subclass(PyTimeEffect)
            .add_subclass(Self))
    }

    #[getter]
    fn time_scalar(slf: PyRef<'_, Self>) -> PyResult<f64> {
        time_scalar_of(&metadata_handle(&slf.into_super().into_super()))
    }

    #[setter]
    fn set_time_scalar(slf: PyRef<'_, Self>, value: f64) -> PyResult<()> {
        set_time_scalar(&metadata_handle(&slf.into_super().into_super()), value)
    }
}

/// A hold on a single frame.
#[pyclass(
    name = "FreezeFrame",
    module = "opentimelineio._otio",
    extends = PyLinearTimeWarp,
    subclass
)]
pub struct PyFreezeFrame;

#[pymethods]
impl PyFreezeFrame {
    #[new]
    #[pyo3(signature = (name = String::new(), metadata = None, enabled = true))]
    fn new(
        name: String,
        metadata: Option<&Bound<'_, PyAny>>,
        enabled: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        // A freeze frame is a time warp whose scalar is always zero: no time
        // passes in the media while time passes on the track.
        let handle = alone_with(
            |base| Node::FreezeFrame {
                effect: EffectData {
                    base,
                    effect_name: "FreezeFrame".to_string(),
                    enabled,
                },
                time_scalar: 0.0,
            },
            name,
            metadata,
        )?;
        Ok(effect_initializer(handle)
            .add_subclass(PyTimeEffect)
            .add_subclass(PyLinearTimeWarp)
            .add_subclass(Self))
    }
}

/// The class initializer every `Effect` subclass starts from.
fn effect_initializer(handle: Handle) -> PyClassInitializer<PyEffect> {
    PyClassInitializer::from(PySerializableObject(handle))
        .add_subclass(PySerializableObjectWithMetadata)
        .add_subclass(PyEffect)
}

/// An effect's speed multiplier.
fn time_scalar_of(handle: &Handle) -> PyResult<f64> {
    handle.with(|node| match node {
        Node::LinearTimeWarp { time_scalar, .. } | Node::FreezeFrame { time_scalar, .. } => {
            Ok(*time_scalar)
        }
        _ => Err(PyValueError::new_err("not a time warp")),
    })
}

/// Sets an effect's speed multiplier.
fn set_time_scalar(handle: &Handle, value: f64) -> PyResult<()> {
    handle.with_mut(|node| match node {
        Node::LinearTimeWarp { time_scalar, .. } | Node::FreezeFrame { time_scalar, .. } => {
            *time_scalar = value;
            Ok(())
        }
        _ => Err(PyValueError::new_err("not a time warp")),
    })
}

/// Builds an item in a document of its own, with its lists filled in.
#[allow(clippy::too_many_arguments)]
fn new_item(
    build: impl FnOnce(ItemData) -> Node,
    name: String,
    source_range: Option<PyTimeRange>,
    effects: Option<&Bound<'_, PyAny>>,
    markers: Option<&Bound<'_, PyAny>>,
    enabled: bool,
    color: Option<PyColor>,
    metadata: Option<&Bound<'_, PyAny>>,
) -> PyResult<Handle> {
    let handle = alone_with(
        |base| {
            build(ItemData {
                base,
                source_range: source_range.map(|range| range.0),
                enabled,
                color: color.map(|color| color.0),
                ..ItemData::new()
            })
        },
        name,
        metadata,
    )?;

    // The lists are filled in afterwards for the same reason metadata is:
    // each object given here lives in its own document until it is moved.
    for (which, given) in [(Which::Effects, effects), (Which::Markers, markers)] {
        let Some(given) = given else { continue };
        let list = PyNodeList {
            handle: handle.clone(),
            which,
        };
        for value in given.try_iter()? {
            let id = list.adopt(&value?)?;
            list.with_list(|list| {
                list.push(id);
                Ok(())
            })?;
        }
    }
    Ok(handle)
}

/// Runs `f` on an object's item fields.
fn with_item_mut<T>(handle: &Handle, f: impl FnOnce(&mut ItemData) -> PyResult<T>) -> PyResult<T> {
    handle.with_mut(|node| {
        let item = node
            .item_mut()
            .ok_or_else(|| PyValueError::new_err("this object does not sit in time"))?;
        f(item)
    })
}

/// The class initializer every `Composable` subclass starts from.
fn composable_initializer(handle: Handle) -> PyClassInitializer<PyComposable> {
    PyClassInitializer::from(PySerializableObject(handle))
        .add_subclass(PySerializableObjectWithMetadata)
        .add_subclass(PyComposable)
}

/// Renders the six fields upstream prints for every item.
fn item_fields(py: Python<'_>, handle: &Handle, quoted: bool) -> PyResult<[String; 6]> {
    let list = |which| -> PyResult<String> {
        let list = PyNodeList {
            handle: handle.clone(),
            which,
        };
        Ok(list.to_list(py)?.bind(py).repr()?.to_string())
    };
    // A time range prints differently for `str` and `repr`, and upstream uses
    // one in each, so the formatting is left to Python rather than guessed at.
    let range = handle.with(|node| {
        Ok(node
            .item()
            .and_then(|item| item.source_range)
            .map(PyTimeRange))
    })?;
    let range = match range {
        None => py.None(),
        Some(range) => range.into_py_any(py)?,
    };
    let range = if quoted {
        range.bind(py).repr()?.to_string()
    } else {
        range.bind(py).str()?.to_string()
    };
    let enabled = handle.with(|node| Ok(node.item().is_some_and(|item| item.enabled)))?;
    Ok([
        if quoted {
            name_repr(py, handle)?
        } else {
            name_str(handle)?
        },
        range,
        list(Which::Effects)?,
        list(Which::Markers)?,
        if enabled { "True" } else { "False" }.to_string(),
        metadata_repr(handle, py)?,
    ])
}

/// Renders an item the way upstream's `__str__` does.
fn item_str(py: Python<'_>, schema: &str, handle: &Handle) -> PyResult<String> {
    let [name, range, effects, markers, enabled, metadata] = item_fields(py, handle, false)?;
    Ok(format!(
        "{schema}({name}, {range}, {effects}, {markers}, {enabled}, {metadata})"
    ))
}

/// Renders an item the way upstream's `__repr__` does.
fn item_repr(py: Python<'_>, schema: &str, handle: &Handle) -> PyResult<String> {
    let [name, range, effects, markers, enabled, metadata] = item_fields(py, handle, true)?;
    Ok(format!(
        "{schema}(name={name}, source_range={range}, effects={effects}, \
         markers={markers}, enabled={enabled}, metadata={metadata})"
    ))
}

/// A marker's range and colour, as Python objects, for printing.
///
/// They are handed to Python rather than formatted here because each prints
/// differently for `str` and `repr`, and upstream uses one in each.
fn marker_parts(py: Python<'_>, handle: &Handle) -> PyResult<(Py<PyAny>, Py<PyAny>)> {
    handle.with(|node| match node {
        Node::Marker(marker) => Ok((
            PyTimeRange(marker.marked_range).into_py_any(py)?,
            marker
                .color
                .clone()
                .map_or_else(|| Ok(py.None()), |color| PyColor(color).into_py_any(py))?,
        )),
        _ => Err(PyValueError::new_err("not a marker")),
    })
}

/// An effect's name.
fn effect_name_of(handle: &Handle) -> PyResult<String> {
    with_effect(handle, |effect| Ok(effect.effect_name.clone()))
}

/// Runs `f` on an object's effect fields.
fn with_effect<T>(handle: &Handle, f: impl FnOnce(&EffectData) -> PyResult<T>) -> PyResult<T> {
    handle.with(|node| {
        let effect = node
            .effect()
            .ok_or_else(|| PyValueError::new_err("this object is not an effect"))?;
        f(effect)
    })
}

/// Runs `f` on an object's effect fields, for writing.
fn with_effect_mut<T>(
    handle: &Handle,
    f: impl FnOnce(&mut EffectData) -> PyResult<T>,
) -> PyResult<T> {
    handle.with_mut(|node| {
        let effect = node
            .effect_mut()
            .ok_or_else(|| PyValueError::new_err("this object is not an effect"))?;
        f(effect)
    })
}

/// Which list of an item a [`PyNodeList`] stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// The item's effects.
    Effects,
    /// The item's markers.
    Markers,
}

impl Which {
    /// The attribute name, for error messages.
    const fn name(self) -> &'static str {
        match self {
            Self::Effects => "effects",
            Self::Markers => "markers",
        }
    }

    /// Borrows the list this stands for.
    fn of(self, item: &ItemData) -> &Vec<NodeId> {
        match self {
            Self::Effects => &item.effects,
            Self::Markers => &item.markers,
        }
    }

    /// Borrows the list this stands for, mutably.
    fn of_mut(self, item: &mut ItemData) -> &mut Vec<NodeId> {
        match self {
            Self::Effects => &mut item.effects,
            Self::Markers => &mut item.markers,
        }
    }
}

/// An item's effects or markers, as a sequence that writes through.
///
/// Upstream hands back a live view, so `item.markers.append(m)` changes the
/// item rather than a copy of its list, and its own tests do exactly that.
/// Appending moves the object into this item's document; see [`crate::arena`]
/// for why that is necessary and what it costs.
#[pyclass(name = "AnyVectorProxy", module = "opentimelineio.core")]
pub struct PyNodeList {
    handle: Handle,
    which: Which,
}

impl PyNodeList {
    /// Returns the document these objects live in.
    pub fn home(&self) -> Shared {
        self.handle.shared.clone()
    }

    /// Reads the list of handles.
    fn ids(&self) -> PyResult<Vec<NodeId>> {
        Ok(self
            .handle
            .with(|node| Ok(node.item().map(|item| self.which.of(item).clone())))?
            .unwrap_or_default())
    }

    /// Turns a Python index into one this list holds, counting from the end
    /// as Python does.
    fn at(&self, index: isize) -> PyResult<usize> {
        let len = self.ids()?.len();
        let length = isize::try_from(len).map_err(|_| PyIndexError::new_err("list is too long"))?;
        let resolved = if index < 0 { index + length } else { index };
        usize::try_from(resolved)
            .ok()
            .filter(|resolved| *resolved < len)
            .ok_or_else(|| {
                PyIndexError::new_err(format!("{} index out of range", self.which.name()))
            })
    }

    /// Moves `value` into this item's document and returns its handle there.
    fn adopt(&self, value: &Bound<'_, PyAny>) -> PyResult<NodeId> {
        let incoming = handle_of(value)?;
        self.handle.shared.absorb(&incoming.shared)?;
        let (_, id) = incoming.live()?;
        Ok(id)
    }

    /// Returns a wrapper for one of these objects.
    fn wrapper<'py>(&self, py: Python<'py>, id: NodeId) -> PyResult<Bound<'py, PyAny>> {
        wrap(py, &self.handle.sibling(id)?)
    }

    /// Runs `f` on the list, for writing.
    fn with_list<T>(&self, f: impl FnOnce(&mut Vec<NodeId>) -> PyResult<T>) -> PyResult<T> {
        let which = self.which;
        self.handle.with_mut(|node| {
            let item = node
                .item_mut()
                .ok_or_else(|| PyValueError::new_err("this object has no effects or markers"))?;
            f(which.of_mut(item))
        })
    }
}

#[pymethods]
impl PyNodeList {
    fn __len__(&self) -> PyResult<usize> {
        Ok(self.ids()?.len())
    }

    /// Reads one element, by an index that has already been bounds-checked
    /// against nothing: negative counts from the end, as Python does.
    ///
    /// The `__internal_` names are upstream's. Slicing, `append`, `extend`,
    /// `remove`, `pop`, `index` and `count` are all written once in Python in
    /// terms of these four and `__len__`; see `_core_utils.py`.
    fn __internal_getitem__(&self, py: Python<'_>, index: isize) -> PyResult<Py<PyAny>> {
        let at = self.at(index)?;
        let id = self.ids()?[at];
        Ok(self.wrapper(py, id)?.unbind())
    }

    fn __internal_setitem__(&self, index: isize, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let at = self.at(index)?;
        let id = self.adopt(value)?;
        self.with_list(|list| {
            list[at] = id;
            Ok(())
        })
    }

    fn __internal_delitem__(&self, index: isize) -> PyResult<()> {
        let at = self.at(index)?;
        self.with_list(|list| {
            list.remove(at);
            Ok(())
        })
    }

    /// Inserts `value` before `index`, clamping as `list.insert` does.
    #[pyo3(name = "__internal_insert")]
    fn internal_insert(&self, index: isize, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let at = clamped_index(index, self.ids()?.len())?;
        let id = self.adopt(value)?;
        self.with_list(|list| {
            list.insert(at, id);
            Ok(())
        })
    }

    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = self.to_list(py)?;
        PyIterator::from_object(list.bind(py))?.into_py_any(py)
    }

    /// Returns these objects copied into an ordinary list.
    fn to_list(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = PyList::empty(py);
        for id in self.ids()? {
            list.append(self.wrapper(py, id)?)?;
        }
        list.into_py_any(py)
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.to_list(py)?.bind(py).eq(other)
    }
}

/// A live view of one object's metadata.
///
/// It holds the object, not a copy of its metadata, so every read and write
/// goes to the document. `_core_utils.py` upstream does the same thing by
/// grafting `MutableMapping` onto a C++ type; here the methods are written
/// out and the Python layer registers the class with `MutableMapping`.
#[pyclass(name = "AnyDictionaryProxy", module = "opentimelineio.core")]
pub struct PyMetadata {
    handle: Handle,
    which: Bag,
    /// The chain of keys leading from that dictionary down to the one this
    /// stands for. Empty for the dictionary itself.
    ///
    /// Metadata nests, and upstream hands back a live view at every level, so
    /// `clip.metadata["a"]["b"] = 1` changes the clip. A nested view cannot
    /// hold a borrow of the inner dictionary — a borrow lasts one call, see
    /// [`crate::arena`] — so it holds the way back to it instead.
    path: Vec<String>,
}

/// Which dictionary on an object a [`PyMetadata`] stands for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bag {
    /// The object's `metadata`, which every named object has.
    Metadata,
    /// A generator reference's `parameters`, which only it has.
    Parameters,
    /// The dynamic fields of an object upstream's two root classes, or a
    /// schema registered from Python, describe.
    Dynamic,
}

impl PyMetadata {
    /// A view of an object's metadata.
    fn of(handle: Handle) -> Self {
        Self {
            handle,
            which: Bag::Metadata,
            path: Vec::new(),
        }
    }

    /// A view of a generator reference's parameters.
    fn of_parameters(handle: Handle) -> Self {
        Self {
            handle,
            which: Bag::Parameters,
            path: Vec::new(),
        }
    }

    /// A view of one dictionary nested inside this one.
    fn nested(&self, key: &str) -> Self {
        let mut path = self.path.clone();
        path.push(key.to_string());
        Self {
            handle: self.handle.clone(),
            which: self.which,
            path,
        }
    }

    /// The document these entries live in.
    fn home(&self) -> &Shared {
        &self.handle.shared
    }

    /// Runs `f` on the dictionary this stands for.
    fn with_entries<T>(&self, f: impl FnOnce(&AnyDictionary) -> PyResult<T>) -> PyResult<T> {
        let which = self.which;
        let path = &self.path;
        self.handle.with(|node| {
            let root = match which {
                Bag::Metadata => match node.base() {
                    Some(base) => &base.metadata,
                    // An object with no metadata reads as an empty mapping
                    // rather than an error, which is what upstream's base
                    // class does.
                    None => return f(&AnyDictionary::new()),
                },
                Bag::Parameters => match node {
                    Node::GeneratorReference(reference) => &reference.parameters,
                    _ => return Err(PyValueError::new_err("not a generator reference")),
                },
                Bag::Dynamic => match node {
                    Node::Dynamic(dynamic) => &dynamic.fields,
                    _ => return f(&AnyDictionary::new()),
                },
            };
            let mut entries = root;
            for key in path {
                entries = match entries.get(key) {
                    Some(Any::Dictionary(nested)) => nested,
                    _ => return Err(PyKeyError::new_err(key.clone())),
                };
            }
            f(entries)
        })
    }

    /// Runs `f` on the dictionary this stands for, for writing.
    fn with_entries_mut<T>(
        &self,
        f: impl FnOnce(&mut AnyDictionary) -> PyResult<T>,
    ) -> PyResult<T> {
        let which = self.which;
        let path = &self.path;
        self.handle.with_mut(|node| {
            let schema = node.schema_name().to_string();
            let root = match which {
                Bag::Metadata => {
                    &mut node
                        .base_mut()
                        .ok_or_else(|| {
                            PyValueError::new_err(format!("a {schema} has no metadata"))
                        })?
                        .metadata
                }
                Bag::Parameters => match node {
                    Node::GeneratorReference(reference) => &mut reference.parameters,
                    _ => return Err(PyValueError::new_err("not a generator reference")),
                },
                Bag::Dynamic => dynamic_fields_mut(node)?,
            };
            let mut entries = root;
            for key in path {
                entries = match entries.get_mut(key) {
                    Some(Any::Dictionary(nested)) => nested,
                    _ => return Err(PyKeyError::new_err(key.clone())),
                };
            }
            f(entries)
        })
    }

    /// Returns these entries copied out, so they can be converted without the
    /// document still borrowed.
    fn entries(&self) -> PyResult<AnyDictionary> {
        self.with_entries(|entries| Ok(entries.clone()))
    }
}

#[pymethods]
impl PyMetadata {
    fn __getitem__(&self, py: Python<'_>, key: &str) -> PyResult<Py<PyAny>> {
        // The value is copied out before it is turned into a Python object,
        // because a metadata value may itself be an object, and building its
        // wrapper reads the document again. See [`crate::arena`]: a borrow
        // lasts one call and no longer.
        let value = self.with_entries(|entries| {
            entries
                .get(key)
                .cloned()
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))
        })?;
        // A nested dictionary comes back as another live view, not a copy, so
        // that `metadata["a"]["b"] = 1` reaches the object.
        if matches!(value, Any::Dictionary(_)) {
            return self.nested(key).into_py_any(py);
        }
        any_to_python(py, self.home(), &value)
    }

    fn __setitem__(&self, key: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let value = python_to_any(self.home(), value)?;
        self.with_entries_mut(|entries| {
            entries.insert(key.to_string(), value);
            Ok(())
        })
    }

    fn __delitem__(&self, key: &str) -> PyResult<()> {
        self.with_entries_mut(|entries| {
            entries
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))
        })
    }

    fn __len__(&self) -> PyResult<usize> {
        self.with_entries(|entries| Ok(entries.len()))
    }

    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let keys: Vec<String> =
            self.with_entries(|entries| Ok(entries.keys().cloned().collect()))?;
        let list = keys.into_py_any(py)?;
        PyIterator::from_object(list.bind(py))?.into_py_any(py)
    }

    fn __contains__(&self, key: &str) -> PyResult<bool> {
        self.with_entries(|entries| Ok(entries.contains_key(key)))
    }

    /// Returns this metadata copied into an ordinary dictionary.
    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let entries = self.entries()?;
        let dict = PyDict::new(py);
        for (key, value) in &entries {
            dict.set_item(key, any_to_python(py, self.home(), value)?)?;
        }
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

/// Reads `SerializableObjectWithMetadata`'s arguments, `(name="",
/// metadata=None)`, out of an argument list.
fn name_and_metadata<'py>(
    args: &Bound<'py, PyTuple>,
    kwargs: Option<&Bound<'py, PyDict>>,
) -> PyResult<(String, Option<Bound<'py, PyAny>>)> {
    if args.len() > 2 {
        return Err(PyTypeError::new_err(format!(
            "SerializableObjectWithMetadata() takes at most 2 positional arguments \
             ({} given)",
            args.len()
        )));
    }
    let mut name = args.get_item(0).ok();
    let mut metadata = args.get_item(1).ok();
    if let Some(kwargs) = kwargs {
        for (key, value) in kwargs {
            let slot = match key.extract::<String>()?.as_str() {
                "name" => &mut name,
                "metadata" => &mut metadata,
                other => {
                    return Err(PyTypeError::new_err(format!(
                        "SerializableObjectWithMetadata() got an unexpected keyword \
                         argument '{other}'"
                    )));
                }
            };
            if slot.replace(value).is_some() {
                return Err(PyTypeError::new_err(
                    "SerializableObjectWithMetadata() got multiple values for an argument",
                ));
            }
        }
    }
    let name = name.map_or_else(|| Ok(String::new()), |name| name.extract::<String>())?;
    Ok((name, metadata))
}

/// An object's dynamic fields, for writing.
///
/// Upstream's two root classes gain a field map the first time one is set,
/// becoming the dynamic object the core holds such things as; see
/// [`crate::registry`]. Any other built-in object has fields of its own and
/// no room for more.
fn dynamic_fields_mut(node: &mut Node) -> PyResult<&mut AnyDictionary> {
    let base = match node {
        Node::SerializableObject => None,
        Node::SerializableObjectWithMetadata(base) => Some(std::mem::take(base)),
        Node::Dynamic(_) => None,
        other => {
            return Err(PyNotImplementedError::new_err(format!(
                "a {} cannot hold dynamic fields",
                other.schema_name()
            )));
        }
    };
    if !matches!(node, Node::Dynamic(_)) {
        let schema_name = node.schema_name().to_string();
        *node = Node::Dynamic(otio_core::schema::DynamicObject {
            schema_name,
            schema_version: 1,
            base,
            fields: AnyDictionary::new(),
        });
    }
    match node {
        Node::Dynamic(dynamic) => Ok(&mut dynamic.fields),
        _ => unreachable!("made dynamic just above"),
    }
}

/// Somewhere media might be.
///
/// Upstream registers this as a schema in its own right as well as using it
/// as a base class, so a file may legitimately carry one.
#[pyclass(
    name = "MediaReference",
    module = "opentimelineio._otio",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PyMediaReference;

/// The handle under a `MediaReference` or one of its subclasses.
fn media_handle(slf: &PyRef<'_, PyMediaReference>) -> Handle {
    slf.as_super().as_super().0.clone()
}

/// Builds a media reference in a document of its own.
fn new_media(
    build: impl FnOnce(MediaReferenceData) -> Node,
    name: String,
    available_range: Option<PyTimeRange>,
    available_image_bounds: Option<PyBox2d>,
    metadata: Option<&Bound<'_, PyAny>>,
) -> PyResult<Handle> {
    alone_with(
        |base| {
            build(MediaReferenceData {
                base,
                available_range: available_range.map(|range| range.0),
                available_image_bounds: available_image_bounds.map(|bounds| bounds.0),
            })
        },
        name,
        metadata,
    )
}

/// The class initializer every `MediaReference` subclass starts from.
fn media_initializer(handle: Handle) -> PyClassInitializer<PyMediaReference> {
    PyClassInitializer::from(PySerializableObject(handle))
        .add_subclass(PySerializableObjectWithMetadata)
        .add_subclass(PyMediaReference)
}

/// Runs `f` on an object's media reference fields.
fn with_media<T>(
    handle: &Handle,
    f: impl FnOnce(&MediaReferenceData) -> PyResult<T>,
) -> PyResult<T> {
    handle.with(|node| {
        let media = node
            .media()
            .ok_or_else(|| PyValueError::new_err("this object is not a media reference"))?;
        f(media)
    })
}

/// Runs `f` on an object's media reference fields, for writing.
fn with_media_mut<T>(
    handle: &Handle,
    f: impl FnOnce(&mut MediaReferenceData) -> PyResult<T>,
) -> PyResult<T> {
    handle.with_mut(|node| {
        let media = node
            .media_mut()
            .ok_or_else(|| PyValueError::new_err("this object is not a media reference"))?;
        f(media)
    })
}

/// Renders a media reference the way upstream's `__str__` does.
///
/// Every field is printed with `repr()` in both, which is upstream's doing:
/// `mediaReference.py` builds `__str__` from `repr` of each part.
fn media_str(py: Python<'_>, schema: &str, handle: &Handle) -> PyResult<String> {
    let [name, range, bounds, metadata] = media_fields(py, handle)?;
    Ok(format!("{schema}({name}, {range}, {bounds}, {metadata})"))
}

/// Renders a media reference the way upstream's `__repr__` does.
fn media_repr(py: Python<'_>, schema: &str, handle: &Handle) -> PyResult<String> {
    let [name, range, bounds, metadata] = media_fields(py, handle)?;
    Ok(format!(
        "{schema}(name={name}, available_range={range}, \
         available_image_bounds={bounds}, metadata={metadata})"
    ))
}

/// The four fields upstream prints for a media reference, each as `repr()`.
fn media_fields(py: Python<'_>, handle: &Handle) -> PyResult<[String; 4]> {
    let range = with_media(handle, |media| Ok(media.available_range.map(PyTimeRange)))?;
    let bounds = with_media(handle, |media| {
        Ok(media.available_image_bounds.map(PyBox2d))
    })?;
    Ok([
        name_repr(py, handle)?,
        optional_repr(py, range)?,
        optional_repr(py, bounds)?,
        metadata_repr(handle, py)?,
    ])
}

/// Renders an optional value the way Python's `repr()` would.
fn optional_repr<T>(py: Python<'_>, value: Option<T>) -> PyResult<String>
where
    T: for<'py> IntoPyObject<'py>,
{
    let object = match value {
        None => py.None(),
        Some(value) => value.into_py_any(py)?,
    };
    Ok(object.bind(py).repr()?.to_string())
}

#[pymethods]
impl PyMediaReference {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        available_range = None,
        metadata = None,
        available_image_bounds = None,
    ))]
    fn new(
        name: String,
        available_range: Option<PyTimeRange>,
        metadata: Option<&Bound<'_, PyAny>>,
        available_image_bounds: Option<PyBox2d>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_media(
            Node::MediaReference,
            name,
            available_range,
            available_image_bounds,
            metadata,
        )?;
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    #[getter]
    fn available_range(slf: PyRef<'_, Self>) -> PyResult<Option<PyTimeRange>> {
        with_media(&media_handle(&slf), |media| {
            Ok(media.available_range.map(PyTimeRange))
        })
    }

    #[setter]
    fn set_available_range(slf: PyRef<'_, Self>, range: Option<PyTimeRange>) -> PyResult<()> {
        with_media_mut(&media_handle(&slf), |media| {
            media.available_range = range.map(|range| range.0);
            Ok(())
        })
    }

    #[getter]
    fn available_image_bounds(slf: PyRef<'_, Self>) -> PyResult<Option<PyBox2d>> {
        with_media(&media_handle(&slf), |media| {
            Ok(media.available_image_bounds.map(PyBox2d))
        })
    }

    #[setter]
    fn set_available_image_bounds(slf: PyRef<'_, Self>, bounds: Option<PyBox2d>) -> PyResult<()> {
        with_media_mut(&media_handle(&slf), |media| {
            media.available_image_bounds = bounds.map(|bounds| bounds.0);
            Ok(())
        })
    }

    /// Whether this stands in for media whose location is unknown.
    #[getter]
    fn is_missing_reference(slf: PyRef<'_, Self>) -> PyResult<bool> {
        media_handle(&slf).with(|node| Ok(matches!(node, Node::MissingReference(_))))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        media_str(py, "MediaReference", &media_handle(&slf))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        media_repr(py, "otio.core.MediaReference", &media_handle(&slf))
    }
}

/// Media that is known to exist but whose location is not.
#[pyclass(
    name = "MissingReference",
    module = "opentimelineio._otio",
    extends = PyMediaReference,
    subclass
)]
pub struct PyMissingReference;

#[pymethods]
impl PyMissingReference {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        available_range = None,
        metadata = None,
        available_image_bounds = None,
    ))]
    fn new(
        name: String,
        available_range: Option<PyTimeRange>,
        metadata: Option<&Bound<'_, PyAny>>,
        available_image_bounds: Option<PyBox2d>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_media(
            |media| Node::MissingReference(MissingReference { media }),
            name,
            available_range,
            available_image_bounds,
            metadata,
        )?;
        Ok(media_initializer(handle).add_subclass(Self))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        media_str(py, "MissingReference", &media_handle(slf.as_super()))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        media_repr(
            py,
            "otio.schema.MissingReference",
            &media_handle(slf.as_super()),
        )
    }
}

/// Media stored at a URL.
#[pyclass(
    name = "ExternalReference",
    module = "opentimelineio._otio",
    extends = PyMediaReference,
    subclass
)]
pub struct PyExternalReference;

#[pymethods]
impl PyExternalReference {
    // Upstream's first argument here is the URL rather than the name, which
    // is why this constructor does not match the others.
    #[new]
    #[pyo3(signature = (
        target_url = String::new(),
        available_range = None,
        metadata = None,
        available_image_bounds = None,
    ))]
    fn new(
        target_url: String,
        available_range: Option<PyTimeRange>,
        metadata: Option<&Bound<'_, PyAny>>,
        available_image_bounds: Option<PyBox2d>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_media(
            move |media| {
                Node::ExternalReference(ExternalReference {
                    media,
                    target_url: target_url.clone(),
                })
            },
            String::new(),
            available_range,
            available_image_bounds,
            metadata,
        )?;
        Ok(media_initializer(handle).add_subclass(Self))
    }

    #[getter]
    fn target_url(slf: PyRef<'_, Self>) -> PyResult<String> {
        media_handle(slf.as_super()).with(|node| match node {
            Node::ExternalReference(reference) => Ok(reference.target_url.clone()),
            _ => Err(PyValueError::new_err("not an external reference")),
        })
    }

    #[setter]
    fn set_target_url(slf: PyRef<'_, Self>, url: String) -> PyResult<()> {
        media_handle(slf.as_super()).with_mut(|node| match node {
            Node::ExternalReference(reference) => {
                reference.target_url = url;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not an external reference")),
        })
    }

    // Upstream prints only the URL for an external reference, and with double
    // quotes in `__str__` because it interpolates rather than using `repr`.
    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let url = Self::target_url(slf)?;
        let _ = py;
        Ok(format!("ExternalReference(\"{url}\")"))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let url = py_repr(py, &Self::target_url(slf)?)?;
        Ok(format!("otio.schema.ExternalReference(target_url={url})"))
    }
}

/// Media produced by a generator, such as colour bars or a slug.
#[pyclass(
    name = "GeneratorReference",
    module = "opentimelineio._otio",
    extends = PyMediaReference,
    subclass
)]
pub struct PyGeneratorReference;

#[pymethods]
impl PyGeneratorReference {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        generator_kind = String::new(),
        available_range = None,
        parameters = None,
        metadata = None,
        available_image_bounds = None,
    ))]
    fn new(
        name: String,
        generator_kind: String,
        available_range: Option<PyTimeRange>,
        parameters: Option<&Bound<'_, PyAny>>,
        metadata: Option<&Bound<'_, PyAny>>,
        available_image_bounds: Option<PyBox2d>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_media(
            move |media| {
                Node::GeneratorReference(GeneratorReference {
                    media,
                    generator_kind: generator_kind.clone(),
                    parameters: AnyDictionary::new(),
                })
            },
            name,
            available_range,
            available_image_bounds,
            metadata,
        )?;
        // As with metadata: anything given here may be an object living in a
        // document of its own, so it is converted once the handle exists.
        if let Some(parameters) = parameters {
            let entries = dictionary_from(&handle.shared, parameters)?;
            handle.with_mut(|node| match node {
                Node::GeneratorReference(reference) => {
                    reference.parameters = entries;
                    Ok(())
                }
                _ => Err(PyValueError::new_err("not a generator reference")),
            })?;
        }
        Ok(media_initializer(handle).add_subclass(Self))
    }

    #[getter]
    fn generator_kind(slf: PyRef<'_, Self>) -> PyResult<String> {
        media_handle(slf.as_super()).with(|node| match node {
            Node::GeneratorReference(reference) => Ok(reference.generator_kind.clone()),
            _ => Err(PyValueError::new_err("not a generator reference")),
        })
    }

    #[setter]
    fn set_generator_kind(slf: PyRef<'_, Self>, kind: String) -> PyResult<()> {
        media_handle(slf.as_super()).with_mut(|node| match node {
            Node::GeneratorReference(reference) => {
                reference.generator_kind = kind;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a generator reference")),
        })
    }

    /// The generator's settings, as a mapping that writes through.
    #[getter]
    fn parameters(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        PyMetadata::of_parameters(media_handle(slf.as_super())).into_py_any(py)
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = media_handle(slf.as_super());
        let kind = with_generator(&handle, |reference| Ok(reference.generator_kind.clone()))?;
        let bounds = with_media(&handle, |media| {
            Ok(media.available_image_bounds.map(PyBox2d))
        })?;
        Ok(format!(
            "GeneratorReference(\"{}\", \"{}\", {}, {}, {})",
            name_str(&handle)?,
            kind,
            PyMetadata::of_parameters(handle.clone()).__repr__(py)?,
            optional_str(py, bounds)?,
            metadata_repr(&handle, py)?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = media_handle(slf.as_super());
        let kind = with_generator(&handle, |reference| Ok(reference.generator_kind.clone()))?;
        let bounds = with_media(&handle, |media| {
            Ok(media.available_image_bounds.map(PyBox2d))
        })?;
        Ok(format!(
            "otio.schema.GeneratorReference(name={}, generator_kind={}, \
             parameters={}, available_image_bounds={}, metadata={})",
            name_repr(py, &handle)?,
            py_repr(py, &kind)?,
            PyMetadata::of_parameters(handle.clone()).__repr__(py)?,
            optional_repr(py, bounds)?,
            metadata_repr(&handle, py)?
        ))
    }
}

/// Runs `f` on a generator reference's own fields.
fn with_generator<T>(
    handle: &Handle,
    f: impl FnOnce(&GeneratorReference) -> PyResult<T>,
) -> PyResult<T> {
    handle.with(|node| match node {
        Node::GeneratorReference(reference) => f(reference),
        _ => Err(PyValueError::new_err("not a generator reference")),
    })
}

/// Renders an optional value the way Python's `str()` would.
fn optional_str<T>(py: Python<'_>, value: Option<T>) -> PyResult<String>
where
    T: for<'py> IntoPyObject<'py>,
{
    let object = match value {
        None => py.None(),
        Some(value) => value.into_py_any(py)?,
    };
    Ok(object.bind(py).str()?.to_string())
}

/// Media stored as a numbered sequence of image files.
#[pyclass(
    name = "ImageSequenceReference",
    module = "opentimelineio._otio",
    extends = PyMediaReference,
    subclass
)]
pub struct PyImageSequenceReference;

#[pymethods]
impl PyImageSequenceReference {
    #[new]
    #[pyo3(signature = (
        target_url_base = String::new(),
        name_prefix = String::new(),
        name_suffix = String::new(),
        start_frame = 1,
        frame_step = 1,
        rate = 1.0,
        frame_zero_padding = 0,
        missing_frame_policy = PyMissingFramePolicy::Error,
        available_range = None,
        metadata = None,
        available_image_bounds = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        target_url_base: String,
        name_prefix: String,
        name_suffix: String,
        start_frame: i64,
        frame_step: i64,
        rate: f64,
        frame_zero_padding: i64,
        missing_frame_policy: PyMissingFramePolicy,
        available_range: Option<PyTimeRange>,
        metadata: Option<&Bound<'_, PyAny>>,
        available_image_bounds: Option<PyBox2d>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_media(
            move |media| {
                Node::ImageSequenceReference(ImageSequenceReference {
                    media,
                    target_url_base: target_url_base.clone(),
                    name_prefix: name_prefix.clone(),
                    name_suffix: name_suffix.clone(),
                    start_frame,
                    frame_step,
                    rate,
                    frame_zero_padding,
                    missing_frame_policy: missing_frame_policy.into(),
                })
            },
            String::new(),
            available_range,
            available_image_bounds,
            metadata,
        )?;
        Ok(media_initializer(handle).add_subclass(Self))
    }

    /// Everything leading up to the file name.
    #[getter]
    fn target_url_base(slf: PyRef<'_, Self>) -> PyResult<String> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.target_url_base.clone())
        })
    }

    #[setter]
    fn set_target_url_base(slf: PyRef<'_, Self>, value: String) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.target_url_base = value;
            Ok(())
        })
    }

    /// Everything in the file name before the frame number.
    #[getter]
    fn name_prefix(slf: PyRef<'_, Self>) -> PyResult<String> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.name_prefix.clone())
        })
    }

    #[setter]
    fn set_name_prefix(slf: PyRef<'_, Self>, value: String) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.name_prefix = value;
            Ok(())
        })
    }

    /// Everything in the file name after the frame number.
    #[getter]
    fn name_suffix(slf: PyRef<'_, Self>) -> PyResult<String> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.name_suffix.clone())
        })
    }

    #[setter]
    fn set_name_suffix(slf: PyRef<'_, Self>, value: String) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.name_suffix = value;
            Ok(())
        })
    }

    /// The first frame number used in file names.
    #[getter]
    fn start_frame(slf: PyRef<'_, Self>) -> PyResult<i64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.start_frame)
        })
    }

    #[setter]
    fn set_start_frame(slf: PyRef<'_, Self>, value: i64) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.start_frame = value;
            Ok(())
        })
    }

    /// How much the frame number advances between images.
    #[getter]
    fn frame_step(slf: PyRef<'_, Self>) -> PyResult<i64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.frame_step)
        })
    }

    #[setter]
    fn set_frame_step(slf: PyRef<'_, Self>, value: i64) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.frame_step = value;
            Ok(())
        })
    }

    /// The rate the sequence plays back at, were every frame present.
    #[getter]
    fn rate(slf: PyRef<'_, Self>) -> PyResult<f64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| Ok(sequence.rate))
    }

    #[setter]
    fn set_rate(slf: PyRef<'_, Self>, value: f64) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.rate = value;
            Ok(())
        })
    }

    /// How many digits the frame number is padded to.
    #[getter]
    fn frame_zero_padding(slf: PyRef<'_, Self>) -> PyResult<i64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.frame_zero_padding)
        })
    }

    #[setter]
    fn set_frame_zero_padding(slf: PyRef<'_, Self>, value: i64) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.frame_zero_padding = value;
            Ok(())
        })
    }

    /// What a player should do about an image file that is not there.
    #[getter]
    fn missing_frame_policy(slf: PyRef<'_, Self>) -> PyResult<PyMissingFramePolicy> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.missing_frame_policy.into())
        })
    }

    #[setter]
    fn set_missing_frame_policy(slf: PyRef<'_, Self>, value: PyMissingFramePolicy) -> PyResult<()> {
        with_sequence_mut(&media_handle(slf.as_super()), |sequence| {
            sequence.missing_frame_policy = value.into();
            Ok(())
        })
    }

    /// The last frame number in the sequence.
    fn end_frame(slf: PyRef<'_, Self>) -> PyResult<i64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.end_frame())
        })
    }

    /// How many images the sequence holds.
    fn number_of_images_in_sequence(slf: PyRef<'_, Self>) -> PyResult<i64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(sequence.number_of_images_in_sequence())
        })
    }

    /// The frame number shown at `time`.
    fn frame_for_time(slf: PyRef<'_, Self>, time: PyRationalTime) -> PyResult<i64> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            core_error(sequence.frame_for_time(time.0))
        })
    }

    /// The URL of one image, counting from zero.
    fn target_url_for_image_number(slf: PyRef<'_, Self>, image_number: i64) -> PyResult<String> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            core_error(sequence.target_url_for_image_number(image_number))
        })
    }

    /// When one image is shown, counting from zero.
    fn presentation_time_for_image_number(
        slf: PyRef<'_, Self>,
        image_number: i64,
    ) -> PyResult<PyRationalTime> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            Ok(PyRationalTime(core_error(
                sequence.presentation_time_for_image_number(image_number),
            )?))
        })
    }

    /// The first and last frame numbers covered by `time_range`.
    fn frame_range_for_time_range(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        time_range: PyTimeRange,
    ) -> PyResult<Py<PyAny>> {
        let handle = media_handle(slf.as_super());
        let (first, last) = with_sequence(&handle, |sequence| {
            Ok((
                core_error(sequence.frame_for_time(time_range.0.start_time()))?,
                core_error(sequence.frame_for_time(time_range.0.end_time_inclusive()))?,
            ))
        })?;
        (first, last).into_py_any(py)
    }

    /// A URL with `symbol` where the frame number would be.
    ///
    /// Tools use this to build a wildcard path such as
    /// `show_shot.%04d.exr`.
    fn abstract_target_url(slf: PyRef<'_, Self>, symbol: &str) -> PyResult<String> {
        with_sequence(&media_handle(slf.as_super()), |sequence| {
            let separator = if sequence.target_url_base.ends_with('/') {
                ""
            } else {
                "/"
            };
            Ok(format!(
                "{}{separator}{}{symbol}{}",
                sequence.target_url_base, sequence.name_prefix, sequence.name_suffix
            ))
        })
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = media_handle(slf.as_super());
        let [base, prefix, suffix, rest @ ..] = sequence_fields(py, &handle, false)?;
        Ok(format!(
            "ImageSequenceReference(\"{base}\", \"{prefix}\", \"{suffix}\", {})",
            rest.join(", ")
        ))
    }

    // Upstream's `repr` for this one class has no `otio.schema.` prefix,
    // unlike every other; the difference is upstream's and is kept.
    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = media_handle(slf.as_super());
        let [
            base,
            prefix,
            suffix,
            start,
            step,
            rate,
            padding,
            policy,
            range,
            bounds,
            metadata,
        ] = sequence_fields(py, &handle, true)?;
        Ok(format!(
            "ImageSequenceReference(target_url_base={base}, name_prefix={prefix}, \
             name_suffix={suffix}, start_frame={start}, frame_step={step}, rate={rate}, \
             frame_zero_padding={padding}, missing_frame_policy={policy}, \
             available_range={range}, available_image_bounds={bounds}, metadata={metadata})"
        ))
    }
}

/// The eleven fields upstream prints for an image sequence.
///
/// `__str__` interpolates the three strings bare and uses `str()` for the
/// rest; `__repr__` uses `repr()` throughout.
fn sequence_fields(py: Python<'_>, handle: &Handle, quoted: bool) -> PyResult<[String; 11]> {
    let sequence = with_sequence(handle, |sequence| Ok(sequence.clone()))?;
    let policy: PyMissingFramePolicy = sequence.missing_frame_policy.into();
    let render = |value: Py<PyAny>| -> PyResult<String> {
        Ok(if quoted {
            value.bind(py).repr()?.to_string()
        } else {
            value.bind(py).str()?.to_string()
        })
    };
    let optional = |value: Option<Py<PyAny>>| -> PyResult<String> {
        render(value.unwrap_or_else(|| py.None()))
    };
    Ok([
        if quoted {
            py_repr(py, &sequence.target_url_base)?
        } else {
            sequence.target_url_base.clone()
        },
        if quoted {
            py_repr(py, &sequence.name_prefix)?
        } else {
            sequence.name_prefix.clone()
        },
        if quoted {
            py_repr(py, &sequence.name_suffix)?
        } else {
            sequence.name_suffix.clone()
        },
        sequence.start_frame.to_string(),
        sequence.frame_step.to_string(),
        render(sequence.rate.into_py_any(py)?)?,
        sequence.frame_zero_padding.to_string(),
        render(policy.into_py_any(py)?)?,
        optional(
            sequence
                .media
                .available_range
                .map(|range| PyTimeRange(range).into_py_any(py))
                .transpose()?,
        )?,
        optional(
            sequence
                .media
                .available_image_bounds
                .map(|bounds| PyBox2d(bounds).into_py_any(py))
                .transpose()?,
        )?,
        metadata_repr(handle, py)?,
    ])
}

/// Runs `f` on an image sequence's own fields.
fn with_sequence<T>(
    handle: &Handle,
    f: impl FnOnce(&ImageSequenceReference) -> PyResult<T>,
) -> PyResult<T> {
    handle.with(|node| match node {
        Node::ImageSequenceReference(sequence) => f(sequence),
        _ => Err(PyValueError::new_err("not an image sequence reference")),
    })
}

/// Runs `f` on an image sequence's own fields, for writing.
fn with_sequence_mut<T>(
    handle: &Handle,
    f: impl FnOnce(&mut ImageSequenceReference) -> PyResult<T>,
) -> PyResult<T> {
    handle.with_mut(|node| match node {
        Node::ImageSequenceReference(sequence) => f(sequence),
        _ => Err(PyValueError::new_err("not an image sequence reference")),
    })
}

/// What a player should do about an image file that is not there.
///
/// Upstream nests this inside `ImageSequenceReference`; a PyO3 class cannot be
/// declared inside another, so the Python layer puts it back.
#[pyclass(
    name = "MissingFramePolicy",
    module = "opentimelineio._otio",
    eq,
    eq_int,
    from_py_object
)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyMissingFramePolicy {
    /// Stop and report the missing frame.
    #[pyo3(name = "error")]
    Error = 0,
    /// Show the last frame that was there.
    #[pyo3(name = "hold")]
    Hold = 1,
    /// Show black.
    #[pyo3(name = "black")]
    Black = 2,
}

#[pymethods]
impl PyMissingFramePolicy {
    /// Prints as pybind11's enums do, which is what upstream's tests compare
    /// against: `<MissingFramePolicy.error: 0>`.
    fn __repr__(&self) -> String {
        format!("<MissingFramePolicy.{}: {}>", self.name(), *self as u8)
    }

    fn __str__(&self) -> String {
        format!("MissingFramePolicy.{}", self.name())
    }
}

impl PyMissingFramePolicy {
    /// The policy's name as it appears in JSON and in Python.
    const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Hold => "hold",
            Self::Black => "black",
        }
    }
}

impl From<MissingFramePolicy> for PyMissingFramePolicy {
    fn from(policy: MissingFramePolicy) -> Self {
        match policy {
            MissingFramePolicy::Error => Self::Error,
            MissingFramePolicy::Hold => Self::Hold,
            MissingFramePolicy::Black => Self::Black,
        }
    }
}

impl From<PyMissingFramePolicy> for MissingFramePolicy {
    fn from(policy: PyMissingFramePolicy) -> Self {
        match policy {
            PyMissingFramePolicy::Error => Self::Error,
            PyMissingFramePolicy::Hold => Self::Hold,
            PyMissingFramePolicy::Black => Self::Black,
        }
    }
}

/// A span of editable media.
#[pyclass(
    name = "Clip",
    module = "opentimelineio._otio",
    extends = PyItem,
    subclass
)]
pub struct PyClip;

#[pymethods]
impl PyClip {
    /// The key a clip's media reference is filed under when no other is named.
    #[classattr]
    #[allow(non_snake_case)]
    fn DEFAULT_MEDIA_KEY() -> &'static str {
        otio_core::DEFAULT_MEDIA_KEY
    }

    #[new]
    #[pyo3(signature = (
        name = String::new(),
        media_reference = None,
        source_range = None,
        metadata = None,
        effects = None,
        markers = None,
        active_media_reference = otio_core::DEFAULT_MEDIA_KEY.to_string(),
        color = None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        media_reference: Option<&Bound<'_, PyAny>>,
        source_range: Option<PyTimeRange>,
        metadata: Option<&Bound<'_, PyAny>>,
        effects: Option<&Bound<'_, PyAny>>,
        markers: Option<&Bound<'_, PyAny>>,
        active_media_reference: String,
        color: Option<PyColor>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let key = active_media_reference.clone();
        let handle = new_item(
            move |item| {
                Node::Clip(Clip {
                    item,
                    media_references: BTreeMap::new(),
                    active_media_reference_key: key.clone(),
                })
            },
            name,
            source_range,
            effects,
            markers,
            true,
            color,
            metadata,
        )?;
        // Upstream gives a clip with no reference a `MissingReference`, so
        // that `clip.media_reference` is never `None`.
        let reference = match media_reference {
            Some(given) if !given.is_none() => adopt_into(&handle, given)?,
            _ => handle.shared.write(|document| {
                Ok(document.insert(Node::MissingReference(MissingReference::default())))
            })?,
        };
        set_media_reference(&handle, &active_media_reference, reference)?;
        Ok(composable_initializer(handle)
            .add_subclass(PyItem)
            .add_subclass(Self))
    }

    /// The media this clip is currently drawing from.
    #[getter]
    fn media_reference(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let handle = item_handle(slf.as_super());
        let key = Self::active_media_reference_key(slf)?;
        let id = with_clip(&handle, |clip| Ok(clip.media_references.get(&key).copied()))?;
        match id {
            None => Ok(py.None()),
            Some(id) => Ok(wrap(py, &handle.sibling(id)?)?.unbind()),
        }
    }

    #[setter]
    fn set_media_reference(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let handle = item_handle(slf.as_super());
        let key = with_clip(&handle, |clip| Ok(clip.active_media_reference_key.clone()))?;
        let id = if value.is_none() {
            handle.shared.write(|document| {
                Ok(document.insert(Node::MissingReference(MissingReference::default())))
            })?
        } else {
            adopt_into(&handle, value)?
        };
        set_media_reference(&handle, &key, id)
    }

    #[getter]
    fn active_media_reference_key(slf: PyRef<'_, Self>) -> PyResult<String> {
        with_clip(&item_handle(slf.as_super()), |clip| {
            Ok(clip.active_media_reference_key.clone())
        })
    }

    #[setter]
    fn set_active_media_reference_key(slf: PyRef<'_, Self>, key: String) -> PyResult<()> {
        let handle = item_handle(slf.as_super());
        // Upstream refuses a key that names no reference, because the clip
        // would then have no media at all.
        let known = with_clip(&handle, |clip| Ok(clip.media_references.contains_key(&key)))?;
        if !known {
            return Err(exception(Error::NoActiveMediaReference { key }, None));
        }
        handle.with_mut(|node| match node {
            Node::Clip(clip) => {
                clip.active_media_reference_key = key;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a clip")),
        })
    }

    /// Every media reference this clip knows about, keyed by name.
    fn media_references(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let handle = item_handle(slf.as_super());
        let ids = with_clip(&handle, |clip| Ok(clip.media_references.clone()))?;
        let references = PyDict::new(py);
        for (key, id) in ids {
            let wrapper = wrap(py, &handle.sibling(id)?)?;
            references.set_item(key, wrapper)?;
        }
        references.into_py_any(py)
    }

    #[pyo3(signature = (media_references, new_active_key))]
    fn set_media_references(
        slf: PyRef<'_, Self>,
        media_references: &Bound<'_, PyDict>,
        new_active_key: String,
    ) -> PyResult<()> {
        let handle = item_handle(slf.as_super());
        let mut replacement = BTreeMap::new();
        for (key, value) in media_references {
            let key: String = key.extract()?;
            // Upstream refuses an empty key, because the key is how a caller
            // names the reference and "" names nothing.
            if key.is_empty() {
                return Err(PyValueError::new_err(
                    "The media references contain an empty key",
                ));
            }
            replacement.insert(key, adopt_into(&handle, &value)?);
        }
        if !replacement.contains_key(&new_active_key) {
            return Err(exception(
                Error::NoActiveMediaReference {
                    key: new_active_key,
                },
                None,
            ));
        }
        handle.with_mut(|node| match node {
            Node::Clip(clip) => {
                clip.media_references = replacement;
                clip.active_media_reference_key = new_active_key;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a clip")),
        })
    }

    /// Yields this clip, so that walking a composition for clips can treat a
    /// bare clip like a composition holding one.
    #[pyo3(signature = (search_range = None, shallow_search = false))]
    fn find_clips(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        let _ = (search_range, shallow_search);
        let found = PyList::empty(py);
        found.append(wrap(py, &item_handle(slf.as_super()))?)?;
        found.into_py_any(py)
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = item_handle(slf.as_super());
        let reference = Self::media_reference(slf, py)?;
        let [name, range, effects, markers, _, metadata] = item_fields(py, &handle, false)?;
        Ok(format!(
            "Clip(\"{name}\", {}, {range}, {metadata}, {effects}, {markers})",
            reference.bind(py).str()?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = item_handle(slf.as_super());
        let color =
            handle.with(|node| Ok(node.item().and_then(|item| item.color.clone()).map(PyColor)))?;
        let reference = Self::media_reference(slf, py)?;
        let [name, range, effects, markers, _, metadata] = item_fields(py, &handle, true)?;
        Ok(format!(
            "otio.schema.Clip(name={name}, media_reference={}, source_range={range}, \
             color={}, metadata={metadata}, effects={effects}, markers={markers})",
            reference.bind(py).repr()?,
            optional_repr(py, color)?
        ))
    }
}

/// Runs `f` on a clip's own fields.
fn with_clip<T>(handle: &Handle, f: impl FnOnce(&Clip) -> PyResult<T>) -> PyResult<T> {
    handle.with(|node| match node {
        Node::Clip(clip) => f(clip),
        _ => Err(PyValueError::new_err("not a clip")),
    })
}

/// Files `reference` under `key` on a clip.
fn set_media_reference(handle: &Handle, key: &str, reference: NodeId) -> PyResult<()> {
    handle.with_mut(|node| match node {
        Node::Clip(clip) => {
            clip.media_references.insert(key.to_string(), reference);
            Ok(())
        }
        _ => Err(PyValueError::new_err("not a clip")),
    })
}

/// Moves `value` into `home`'s document and returns its handle there.
///
/// A no-op when it is already in the same document; see [`crate::arena`].
fn adopt_into(home: &Handle, value: &Bound<'_, PyAny>) -> PyResult<NodeId> {
    let incoming = handle_of(value)?;
    home.shared.absorb(&incoming.shared)?;
    let (_, id) = incoming.live()?;
    Ok(id)
}

/// An item that holds other composables.
#[pyclass(
    name = "Composition",
    module = "opentimelineio._otio",
    extends = PyItem,
    subclass
)]
pub struct PyComposition;

/// The handle under a `Composition` or one of its subclasses.
fn composition_handle(slf: &PyRef<'_, PyComposition>) -> Handle {
    item_handle(slf.as_super())
}

/// The class initializer every `Composition` subclass starts from.
fn composition_initializer(handle: Handle) -> PyClassInitializer<PyComposition> {
    composable_initializer(handle)
        .add_subclass(PyItem)
        .add_subclass(PyComposition)
}

/// Builds a composition in a document of its own, with its children adopted.
#[allow(clippy::too_many_arguments)]
fn new_composition(
    build: impl FnOnce(ItemData) -> Node,
    name: String,
    children: Option<&Bound<'_, PyAny>>,
    source_range: Option<PyTimeRange>,
    effects: Option<&Bound<'_, PyAny>>,
    markers: Option<&Bound<'_, PyAny>>,
    color: Option<PyColor>,
    metadata: Option<&Bound<'_, PyAny>>,
) -> PyResult<Handle> {
    let handle = new_item(
        build,
        name,
        source_range,
        effects,
        markers,
        true,
        color,
        metadata,
    )?;
    if let Some(children) = children {
        for child in children.try_iter()? {
            let child = child?;
            let id = adopt_into(&handle, &child)?;
            let (shared, parent) = handle.live()?;
            shared.write(|document| core_error(document.append_child(parent, id)))?;
        }
    }
    Ok(handle)
}

#[pymethods]
impl PyComposition {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        children = None,
        source_range = None,
        metadata = None,
    ))]
    fn new(
        name: String,
        children: Option<&Bound<'_, PyAny>>,
        source_range: Option<PyTimeRange>,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_composition(
            |item| {
                Node::Composition(Composition {
                    item,
                    children: Vec::new(),
                })
            },
            name,
            children,
            source_range,
            None,
            None,
            None,
            metadata,
        )?;
        Ok(composable_initializer(handle)
            .add_subclass(PyItem)
            .add_subclass(Self))
    }

    /// What this composition is called in error messages.
    #[getter]
    fn composition_kind(slf: PyRef<'_, Self>) -> PyResult<String> {
        composition_handle(&slf).with(|node| Ok(node.schema_name().to_string()))
    }

    fn __len__(slf: PyRef<'_, Self>) -> PyResult<usize> {
        Ok(children_of(&composition_handle(&slf))?.len())
    }

    fn __internal_getitem__(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        index: isize,
    ) -> PyResult<Py<PyAny>> {
        let handle = composition_handle(&slf);
        let children = children_of(&handle)?;
        let at = child_index(index, children.len(), READ_PAST_END)?;
        Ok(wrap(py, &handle.sibling(children[at])?)?.unbind())
    }

    fn __internal_setitem__(
        slf: PyRef<'_, Self>,
        index: isize,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let handle = composition_handle(&slf);
        let at = child_index(index, children_of(&handle)?.len(), WRITE_PAST_END)?;
        let id = adopt_into(&handle, value)?;
        let (shared, parent) = handle.live()?;
        let index = at;
        let at = i64::try_from(at).map_err(|_| PyIndexError::new_err("index is too large"))?;
        shared.write(|document| {
            // As upstream's `Composition::set_child`: putting a child back
            // where it already is does nothing, and one that sits in a
            // composition, this one included, is refused before anything
            // is removed.
            if core_error(document.children_of(parent))?.get(index) == Some(&id) {
                return Ok(());
            }
            if core_error(document.try_get(id))?.parent().is_some() {
                return Err(core_error::<()>(Err(Error::ChildAlreadyParented)).unwrap_err());
            }
            core_error(document.remove_child(parent, at))?;
            core_error(document.insert_child(parent, at, id))
        })
    }

    fn __internal_delitem__(slf: PyRef<'_, Self>, index: isize) -> PyResult<()> {
        let handle = composition_handle(&slf);
        let at = child_index(index, children_of(&handle)?.len(), WRITE_PAST_END)?;
        let (shared, parent) = handle.live()?;
        let at = i64::try_from(at).map_err(|_| PyIndexError::new_err("index is too large"))?;
        shared.write(|document| core_error(document.remove_child(parent, at)).map(|_| ()))
    }

    #[pyo3(name = "__internal_insert")]
    fn internal_insert(
        slf: PyRef<'_, Self>,
        index: isize,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let handle = composition_handle(&slf);
        let at = clamped_index(index, children_of(&handle)?.len())?;
        let id = adopt_into(&handle, value)?;
        let (shared, parent) = handle.live()?;
        let at = i64::try_from(at).map_err(|_| PyIndexError::new_err("index is too large"))?;
        shared.write(|document| core_error(document.insert_child(parent, at, id)))
    }

    fn __iter__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = children_list(py, &composition_handle(&slf))?;
        PyIterator::from_object(list.bind(py))?.into_py_any(py)
    }

    /// Whether `other` sits anywhere below this composition.
    fn is_parent_of(slf: PyRef<'_, Self>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        let handle = composition_handle(&slf);
        let other = handle_of(other)?;
        if !handle.shared.is(&other.shared)? {
            return Ok(false);
        }
        let (shared, parent) = handle.live()?;
        let (_, child) = other.live()?;
        shared.read(|document| core_error(document.is_parent_of(parent, child)))
    }

    fn range_of_child_at_index(slf: PyRef<'_, Self>, index: i64) -> PyResult<PyTimeRange> {
        let (shared, id) = composition_handle(&slf).live()?;
        shared.read(|document| {
            Ok(PyTimeRange(core_error(
                document.range_of_child_at_index(id, index),
            )?))
        })
    }

    fn trimmed_range_of_child_at_index(slf: PyRef<'_, Self>, index: i64) -> PyResult<PyTimeRange> {
        let (shared, id) = composition_handle(&slf).live()?;
        shared.read(|document| {
            Ok(PyTimeRange(core_error(
                document.trimmed_range_of_child_at_index(id, index),
            )?))
        })
    }

    // Upstream takes a `reference_space` here and ignores it; the argument is
    // kept so that calls written against upstream still work.
    #[pyo3(signature = (child, reference_space = None))]
    fn range_of_child(
        slf: PyRef<'_, Self>,
        child: &Bound<'_, PyAny>,
        reference_space: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyTimeRange> {
        let _ = reference_space;
        let (shared, parent, child) = pair(&composition_handle(&slf), child, not_descended_from)?;
        shared.read(|document| {
            Ok(PyTimeRange(core_error(
                document.range_of_child(parent, child),
            )?))
        })
    }

    #[pyo3(signature = (child, reference_space = None))]
    fn trimmed_range_of_child(
        slf: PyRef<'_, Self>,
        child: &Bound<'_, PyAny>,
        reference_space: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Option<PyTimeRange>> {
        let _ = reference_space;
        let (shared, parent, child) = pair(&composition_handle(&slf), child, not_descended_from)?;
        shared.read(|document| {
            Ok(core_error(document.trimmed_range_of_child(parent, child))?.map(PyTimeRange))
        })
    }

    fn trim_child_range(
        slf: PyRef<'_, Self>,
        child_range: PyTimeRange,
    ) -> PyResult<Option<PyTimeRange>> {
        let (shared, id) = composition_handle(&slf).live()?;
        shared.read(|document| {
            Ok(core_error(document.trim_child_range(id, child_range.0))?.map(PyTimeRange))
        })
    }

    /// As [`Self::trim_child_range`]; upstream spells it both ways.
    fn trimmed_child_range(
        slf: PyRef<'_, Self>,
        child_range: PyTimeRange,
    ) -> PyResult<Option<PyTimeRange>> {
        Self::trim_child_range(slf, child_range)
    }

    fn range_of_all_children(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let handle = composition_handle(&slf);
        let (shared, id) = handle.live()?;
        let ranges = shared.read(|document| core_error(document.range_of_all_children(id)))?;
        let found = PyDict::new(py);
        for (child, range) in ranges {
            let wrapper = wrap(
                py,
                &Handle {
                    shared: shared.clone(),
                    id: child,
                },
            )?;
            found.set_item(wrapper, PyTimeRange(range))?;
        }
        found.into_py_any(py)
    }

    #[pyo3(signature = (search_time, shallow_search = false))]
    fn child_at_time(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        search_time: PyRationalTime,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        let handle = composition_handle(&slf);
        let (shared, id) = handle.live()?;
        let found = shared.read(|document| {
            core_error(document.child_at_time(id, search_time.0, shallow_search))
        })?;
        match found {
            None => Ok(py.None()),
            Some(child) => Ok(wrap(
                py,
                &Handle {
                    shared: shared.clone(),
                    id: child,
                },
            )?
            .unbind()),
        }
    }

    fn children_in_range(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        search_range: PyTimeRange,
    ) -> PyResult<Py<PyAny>> {
        let handle = composition_handle(&slf);
        let (shared, id) = handle.live()?;
        let found =
            shared.read(|document| core_error(document.children_in_range(id, search_range.0)))?;
        wrappers(py, &shared, &found)
    }

    #[pyo3(signature = (descended_from_type = None, search_range = None, shallow_search = false))]
    fn find_children(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        descended_from_type: Option<&Bound<'_, PyAny>>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        find_children_below(
            py,
            &composition_handle(&slf),
            descended_from_type,
            search_range,
            shallow_search,
        )
    }

    #[pyo3(signature = (search_range = None, shallow_search = false))]
    fn find_clips(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        find_clips_below(py, &composition_handle(&slf), search_range, shallow_search)
    }

    /// Whether any clip sits below this composition.
    fn has_clips(slf: PyRef<'_, Self>) -> PyResult<bool> {
        let (shared, id) = composition_handle(&slf).live()?;
        Ok(!shared
            .read(|document| core_error(document.find_clips(id)))?
            .is_empty())
    }

    /// The gaps this child leaves at each end of its media, as a pair.
    fn handles_of_child(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        child: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let (shared, parent, child) = pair(&composition_handle(&slf), child, not_a_child_of)?;
        let (before, after) =
            shared.read(|document| core_error(document.handles_of_child(parent, child)))?;
        (before.map(PyRationalTime), after.map(PyRationalTime)).into_py_any(py)
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        composition_str(py, "Composition", &composition_handle(&slf))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        composition_repr(py, "otio.core.Composition", &composition_handle(&slf))
    }
}

/// A sequence of items laid end to end.
#[pyclass(
    name = "Track",
    module = "opentimelineio._otio",
    extends = PyComposition,
    subclass
)]
pub struct PyTrack;

#[pymethods]
impl PyTrack {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        children = None,
        source_range = None,
        kind = otio_core::TRACK_KIND_VIDEO.to_string(),
        metadata = None,
        color = None,
    ))]
    fn new(
        name: String,
        children: Option<&Bound<'_, PyAny>>,
        source_range: Option<PyTimeRange>,
        kind: String,
        metadata: Option<&Bound<'_, PyAny>>,
        color: Option<PyColor>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_composition(
            move |item| {
                Node::Track(Track {
                    item,
                    children: Vec::new(),
                    kind: kind.clone(),
                })
            },
            name,
            children,
            source_range,
            None,
            None,
            color,
            metadata,
        )?;
        Ok(composition_initializer(handle).add_subclass(Self))
    }

    #[getter]
    fn kind(slf: PyRef<'_, Self>) -> PyResult<String> {
        composition_handle(slf.as_super()).with(|node| match node {
            Node::Track(track) => Ok(track.kind.clone()),
            _ => Err(PyValueError::new_err("not a track")),
        })
    }

    #[setter]
    fn set_kind(slf: PyRef<'_, Self>, kind: String) -> PyResult<()> {
        composition_handle(slf.as_super()).with_mut(|node| match node {
            Node::Track(track) => {
                track.kind = kind;
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a track")),
        })
    }

    /// The items either side of `item` on this track, as a pair.
    ///
    /// With `policy` set to `around_transitions`, a transition at either end
    /// gets a zero-length gap beside it, which is what a tool needs in order
    /// to draw the transition's handles.
    #[pyo3(signature = (item, policy = NeighborPolicy::Never))]
    fn neighbors_of(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        item: &Bound<'_, PyAny>,
        policy: NeighborPolicy,
    ) -> PyResult<Py<PyAny>> {
        let (shared, parent, child) =
            pair(&composition_handle(slf.as_super()), item, not_a_child_of)?;
        let policy = match policy {
            NeighborPolicy::Never => NeighborGapPolicy::Never,
            NeighborPolicy::AroundTransitions => NeighborGapPolicy::AroundTransitions,
        };
        let (before, after) = shared
            .write(|document| core_error(document.neighbors_of_mut(parent, child, policy)))?;
        let wrap_one = |id: Option<NodeId>| -> PyResult<Py<PyAny>> {
            match id {
                None => Ok(py.None()),
                Some(id) => Ok(wrap(
                    py,
                    &Handle {
                        shared: shared.clone(),
                        id,
                    },
                )?
                .unbind()),
            }
        };
        (wrap_one(before)?, wrap_one(after)?).into_py_any(py)
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        composition_str(py, "Track", &composition_handle(slf.as_super()))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        composition_repr(py, "otio.schema.Track", &composition_handle(slf.as_super()))
    }
}

/// Whether a track's neighbour search invents gaps around transitions.
///
/// Upstream makes this an enum nested inside `Track`; the Python layer puts it
/// back there, since a nested class cannot be declared here.
#[pyclass(
    name = "NeighborGapPolicy",
    module = "opentimelineio._otio",
    eq,
    eq_int,
    from_py_object
)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NeighborPolicy {
    /// Report nothing beside an item at the end of a track.
    #[pyo3(name = "never")]
    Never = 0,
    /// Put a zero-length gap beside a transition at the end of a track.
    #[pyo3(name = "around_transitions")]
    AroundTransitions = 1,
}

/// A set of items layered over the same span of time.
#[pyclass(
    name = "Stack",
    module = "opentimelineio._otio",
    extends = PyComposition,
    subclass
)]
pub struct PyStack;

#[pymethods]
impl PyStack {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        children = None,
        source_range = None,
        markers = None,
        effects = None,
        metadata = None,
    ))]
    fn new(
        name: String,
        children: Option<&Bound<'_, PyAny>>,
        source_range: Option<PyTimeRange>,
        markers: Option<&Bound<'_, PyAny>>,
        effects: Option<&Bound<'_, PyAny>>,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_composition(
            |item| {
                Node::Stack(Stack {
                    item,
                    children: Vec::new(),
                })
            },
            name,
            children,
            source_range,
            effects,
            markers,
            None,
            metadata,
        )?;
        Ok(composition_initializer(handle).add_subclass(Self))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        composition_str(py, "Stack", &composition_handle(slf.as_super()))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        composition_repr(py, "otio.schema.Stack", &composition_handle(slf.as_super()))
    }
}

/// A whole edit: a stack of tracks with a start time.
#[pyclass(
    name = "Timeline",
    module = "opentimelineio._otio",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PyTimeline;

/// The handle under a `Timeline`.
fn timeline_handle(slf: &PyRef<'_, PyTimeline>) -> Handle {
    slf.as_super().as_super().0.clone()
}

#[pymethods]
impl PyTimeline {
    // Upstream's second argument is named `tracks` but takes the children of
    // the timeline's stack, not the stack itself.
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        tracks = None,
        global_start_time = None,
        metadata = None,
    ))]
    fn new(
        name: String,
        tracks: Option<&Bound<'_, PyAny>>,
        global_start_time: Option<PyRationalTime>,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            |base| {
                Node::Timeline(Timeline {
                    base,
                    tracks: None,
                    global_start_time: global_start_time.map(|time| time.0),
                })
            },
            name,
            metadata,
        )?;
        // A timeline always has a stack, even an empty one, because upstream
        // builds one in its constructor and its own tests append to it.
        let stack = empty_stack(&handle)?;
        set_tracks(&handle, stack)?;
        if let Some(tracks) = tracks {
            let stack = handle.sibling(stack)?;
            for track in tracks.try_iter()? {
                let track = track?;
                let id = adopt_into(&stack, &track)?;
                let (shared, parent) = stack.live()?;
                shared.write(|document| core_error(document.append_child(parent, id)))?;
            }
        }
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    #[getter]
    fn tracks(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let handle = timeline_handle(&slf);
        let stack = handle.with(|node| match node {
            Node::Timeline(timeline) => Ok(timeline.tracks),
            _ => Err(PyValueError::new_err("not a timeline")),
        })?;
        match stack {
            None => Ok(py.None()),
            Some(id) => Ok(wrap(py, &handle.sibling(id)?)?.unbind()),
        }
    }

    // Setting this to `None` leaves an empty stack rather than nothing:
    // upstream builds one, and its own test checks that `tl.tracks` is still
    // a `Stack` afterwards.
    #[setter]
    fn set_tracks(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let handle = timeline_handle(&slf);
        let id = if value.is_none() {
            empty_stack(&handle)?
        } else {
            // Upstream's setter is typed to take a `Stack`, so anything else
            // is a `TypeError` there. Checking before adopting matters: the
            // alternative leaves the timeline holding, say, a clip, and every
            // method that walks the tracks then fails or answers about the
            // wrong object.
            if !value.is_instance_of::<PyStack>() {
                return Err(PyTypeError::new_err("a timeline's tracks must be a Stack"));
            }
            adopt_into(&handle, value)?
        };
        set_tracks(&handle, id)
    }

    #[getter]
    fn global_start_time(slf: PyRef<'_, Self>) -> PyResult<Option<PyRationalTime>> {
        timeline_handle(&slf).with(|node| match node {
            Node::Timeline(timeline) => Ok(timeline.global_start_time.map(PyRationalTime)),
            _ => Err(PyValueError::new_err("not a timeline")),
        })
    }

    #[setter]
    fn set_global_start_time(slf: PyRef<'_, Self>, time: Option<PyRationalTime>) -> PyResult<()> {
        timeline_handle(&slf).with_mut(|node| match node {
            Node::Timeline(timeline) => {
                timeline.global_start_time = time.map(|time| time.0);
                Ok(())
            }
            _ => Err(PyValueError::new_err("not a timeline")),
        })
    }

    /// How long the timeline runs.
    fn duration(slf: PyRef<'_, Self>) -> PyResult<PyRationalTime> {
        let (shared, id) = tracks_of(&timeline_handle(&slf))?;
        shared.read(|document| Ok(PyRationalTime(core_error(document.duration(id))?)))
    }

    fn range_of_child(slf: PyRef<'_, Self>, child: &Bound<'_, PyAny>) -> PyResult<PyTimeRange> {
        let handle = timeline_handle(&slf);
        let (shared, stack) = tracks_of(&handle)?;
        let child = adopt_free(&handle, child)?;
        shared.read(|document| {
            Ok(PyTimeRange(core_error(
                document.range_of_child(stack, child),
            )?))
        })
    }

    /// The video tracks, in order.
    fn video_tracks(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        tracks_of_kind(py, &timeline_handle(&slf), otio_core::TRACK_KIND_VIDEO)
    }

    /// The audio tracks, in order.
    fn audio_tracks(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        tracks_of_kind(py, &timeline_handle(&slf), otio_core::TRACK_KIND_AUDIO)
    }

    #[pyo3(signature = (descended_from_type = None, search_range = None, shallow_search = false))]
    fn find_children(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        descended_from_type: Option<&Bound<'_, PyAny>>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        let handle = timeline_handle(&slf);
        let (shared, stack) = tracks_of(&handle)?;
        find_children_below(
            py,
            &Handle { shared, id: stack },
            descended_from_type,
            search_range,
            shallow_search,
        )
    }

    #[pyo3(signature = (search_range = None, shallow_search = false))]
    fn find_clips(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        let handle = timeline_handle(&slf);
        let (shared, stack) = tracks_of(&handle)?;
        find_clips_below(
            py,
            &Handle { shared, id: stack },
            search_range,
            shallow_search,
        )
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = timeline_handle(&slf);
        let tracks = Self::tracks(slf, py)?;
        Ok(format!(
            "Timeline(\"{}\", {})",
            name_str(&handle)?,
            tracks.bind(py).str()?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = timeline_handle(&slf);
        let tracks = Self::tracks(slf, py)?;
        Ok(format!(
            "otio.schema.Timeline(name={}, tracks={})",
            name_repr(py, &handle)?,
            tracks.bind(py).repr()?
        ))
    }
}

/// Adds an empty stack named `tracks` to a timeline's document.
fn empty_stack(handle: &Handle) -> PyResult<NodeId> {
    let (shared, _) = handle.live()?;
    shared.write(|document| {
        Ok(document.insert(Node::Stack(Stack {
            item: ItemData {
                base: Base {
                    name: "tracks".to_string(),
                    metadata: AnyDictionary::new(),
                },
                ..ItemData::new()
            },
            children: Vec::new(),
        })))
    })
}

/// Points a timeline at `stack`.
fn set_tracks(handle: &Handle, stack: NodeId) -> PyResult<()> {
    handle.with_mut(|node| match node {
        Node::Timeline(timeline) => {
            timeline.tracks = Some(stack);
            Ok(())
        }
        _ => Err(PyValueError::new_err("not a timeline")),
    })
}

/// Returns a timeline's stack, refusing a timeline that has none.
fn tracks_of(handle: &Handle) -> PyResult<(Shared, NodeId)> {
    let stack = handle.with(|node| match node {
        Node::Timeline(timeline) => Ok(timeline.tracks),
        _ => Err(PyValueError::new_err("not a timeline")),
    })?;
    let stack = stack.ok_or_else(|| PyValueError::new_err("the timeline has no tracks"))?;
    let (shared, _) = handle.live()?;
    Ok((shared, stack))
}

/// The tracks of one kind on a timeline, in order.
fn tracks_of_kind(py: Python<'_>, handle: &Handle, kind: &str) -> PyResult<Py<PyAny>> {
    let (shared, stack) = tracks_of(handle)?;
    let found = shared.read(|document| {
        core_error(document.find_children(
            stack,
            None,
            true,
            &|node: &Node| matches!(node, Node::Track(track) if track.kind == kind),
        ))
    })?;
    wrappers(py, &shared, &found)
}

/// An ordered group of any objects, with no timing of its own.
///
/// Upstream's bin: a way to keep several timelines, clips or references in
/// one file. It is not a composition, so its children have no range in it,
/// and it is what the FCP 7 XML and AAF readers return when a file holds more
/// than one thing.
#[pyclass(
    name = "SerializableCollection",
    module = "opentimelineio._otio",
    extends = PySerializableObjectWithMetadata,
    subclass
)]
pub struct PySerializableCollection;

/// The handle under a `SerializableCollection`.
fn collection_handle(slf: &PyRef<'_, PySerializableCollection>) -> Handle {
    slf.as_super().as_super().0.clone()
}

#[pymethods]
impl PySerializableCollection {
    #[new]
    #[pyo3(signature = (name = String::new(), children = None, metadata = None))]
    fn new(
        name: String,
        children: Option<&Bound<'_, PyAny>>,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            |base| {
                Node::SerializableCollection(SerializableCollection {
                    base,
                    children: Vec::new(),
                })
            },
            name,
            metadata,
        )?;
        if let Some(children) = children {
            for child in children.try_iter()? {
                let child = child?;
                let id = adopt_into(&handle, &child)?;
                let (shared, parent) = handle.live()?;
                shared.write(|document| core_error(document.append_child(parent, id)))?;
            }
        }
        Ok(PyClassInitializer::from(PySerializableObject(handle))
            .add_subclass(PySerializableObjectWithMetadata)
            .add_subclass(Self))
    }

    fn __len__(slf: PyRef<'_, Self>) -> PyResult<usize> {
        Ok(children_of(&collection_handle(&slf))?.len())
    }

    fn __internal_getitem__(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        index: isize,
    ) -> PyResult<Py<PyAny>> {
        let handle = collection_handle(&slf);
        let children = children_of(&handle)?;
        let at = child_index(index, children.len(), READ_PAST_END)?;
        Ok(wrap(py, &handle.sibling(children[at])?)?.unbind())
    }

    fn __internal_setitem__(
        slf: PyRef<'_, Self>,
        index: isize,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let handle = collection_handle(&slf);
        let at = child_index(index, children_of(&handle)?.len(), WRITE_PAST_END)?;
        let id = adopt_into(&handle, value)?;
        let (shared, parent) = handle.live()?;
        let at = i64::try_from(at).map_err(|_| PyIndexError::new_err("index is too large"))?;
        shared.write(|document| {
            core_error(document.remove_child(parent, at))?;
            core_error(document.insert_child(parent, at, id))
        })
    }

    fn __internal_delitem__(slf: PyRef<'_, Self>, index: isize) -> PyResult<()> {
        let handle = collection_handle(&slf);
        let at = child_index(index, children_of(&handle)?.len(), WRITE_PAST_END)?;
        let (shared, parent) = handle.live()?;
        let at = i64::try_from(at).map_err(|_| PyIndexError::new_err("index is too large"))?;
        shared.write(|document| core_error(document.remove_child(parent, at)).map(|_| ()))
    }

    #[pyo3(name = "__internal_insert")]
    fn internal_insert(
        slf: PyRef<'_, Self>,
        index: isize,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let handle = collection_handle(&slf);
        let at = clamped_index(index, children_of(&handle)?.len())?;
        let id = adopt_into(&handle, value)?;
        let (shared, parent) = handle.live()?;
        let at = i64::try_from(at).map_err(|_| PyIndexError::new_err("index is too large"))?;
        shared.write(|document| core_error(document.insert_child(parent, at, id)))
    }

    fn __iter__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let list = children_list(py, &collection_handle(&slf))?;
        PyIterator::from_object(list.bind(py))?.into_py_any(py)
    }

    #[pyo3(signature = (descended_from_type = None, search_range = None, shallow_search = false))]
    fn find_children(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        descended_from_type: Option<&Bound<'_, PyAny>>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        find_children_below(
            py,
            &collection_handle(&slf),
            descended_from_type,
            search_range,
            shallow_search,
        )
    }

    #[pyo3(signature = (search_range = None, shallow_search = false))]
    fn find_clips(
        slf: PyRef<'_, Self>,
        py: Python<'_>,
        search_range: Option<PyTimeRange>,
        shallow_search: bool,
    ) -> PyResult<Py<PyAny>> {
        find_clips_below(py, &collection_handle(&slf), search_range, shallow_search)
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = collection_handle(&slf);
        let children = children_list(py, &handle)?;
        Ok(format!(
            "SerializableCollection({}, {}, {})",
            name_str(&handle)?,
            children.bind(py).str()?,
            metadata_repr(&handle, py)?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = collection_handle(&slf);
        let children = children_list(py, &handle)?;
        Ok(format!(
            "otio.schema.SerializableCollection(name={}, children={}, metadata={})",
            name_repr(py, &handle)?,
            children.bind(py).repr()?,
            metadata_repr(&handle, py)?
        ))
    }
}

/// A dissolve or wipe between two neighbouring items.
#[pyclass(
    name = "Transition",
    module = "opentimelineio._otio",
    extends = PyComposable,
    subclass
)]
pub struct PyTransition;

/// The handle under a `Transition`.
fn transition_handle(slf: &PyRef<'_, PyTransition>) -> Handle {
    slf.as_super().as_super().as_super().0.clone()
}

#[pymethods]
impl PyTransition {
    #[new]
    #[pyo3(signature = (
        name = String::new(),
        transition_type = String::new(),
        in_offset = PyRationalTime(RationalTime::new(0.0, 1.0)),
        out_offset = PyRationalTime(RationalTime::new(0.0, 1.0)),
        metadata = None,
        enabled = true,
    ))]
    fn new(
        name: String,
        transition_type: String,
        in_offset: PyRationalTime,
        out_offset: PyRationalTime,
        metadata: Option<&Bound<'_, PyAny>>,
        enabled: bool,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = alone_with(
            move |base| {
                Node::Transition(Transition {
                    base,
                    parent: None,
                    in_offset: in_offset.0,
                    out_offset: out_offset.0,
                    transition_type: transition_type.clone(),
                    enabled,
                })
            },
            name,
            metadata,
        )?;
        Ok(composable_initializer(handle).add_subclass(Self))
    }

    #[getter]
    fn transition_type(slf: PyRef<'_, Self>) -> PyResult<String> {
        with_transition(&transition_handle(&slf), |transition| {
            Ok(transition.transition_type.clone())
        })
    }

    #[setter]
    fn set_transition_type(slf: PyRef<'_, Self>, kind: String) -> PyResult<()> {
        with_transition_mut(&transition_handle(&slf), |transition| {
            transition.transition_type = kind;
            Ok(())
        })
    }

    #[getter]
    fn in_offset(slf: PyRef<'_, Self>) -> PyResult<PyRationalTime> {
        with_transition(&transition_handle(&slf), |transition| {
            Ok(PyRationalTime(transition.in_offset))
        })
    }

    #[setter]
    fn set_in_offset(slf: PyRef<'_, Self>, offset: PyRationalTime) -> PyResult<()> {
        with_transition_mut(&transition_handle(&slf), |transition| {
            transition.in_offset = offset.0;
            Ok(())
        })
    }

    #[getter]
    fn out_offset(slf: PyRef<'_, Self>) -> PyResult<PyRationalTime> {
        with_transition(&transition_handle(&slf), |transition| {
            Ok(PyRationalTime(transition.out_offset))
        })
    }

    #[setter]
    fn set_out_offset(slf: PyRef<'_, Self>, offset: PyRationalTime) -> PyResult<()> {
        with_transition_mut(&transition_handle(&slf), |transition| {
            transition.out_offset = offset.0;
            Ok(())
        })
    }

    #[getter]
    fn enabled(slf: PyRef<'_, Self>) -> PyResult<bool> {
        with_transition(
            &transition_handle(&slf),
            |transition| Ok(transition.enabled),
        )
    }

    #[setter]
    fn set_enabled(slf: PyRef<'_, Self>, enabled: bool) -> PyResult<()> {
        with_transition_mut(&transition_handle(&slf), |transition| {
            transition.enabled = enabled;
            Ok(())
        })
    }

    /// How long the transition lasts, which is its two offsets together.
    fn duration(slf: PyRef<'_, Self>) -> PyResult<PyRationalTime> {
        let (shared, id) = transition_handle(&slf).live()?;
        shared.read(|document| Ok(PyRationalTime(core_error(document.duration(id))?)))
    }

    fn range_in_parent(slf: PyRef<'_, Self>) -> PyResult<PyTimeRange> {
        let (shared, id) = transition_handle(&slf).live()?;
        shared.read(|document| Ok(PyTimeRange(core_error(document.range_in_parent(id))?)))
    }

    fn trimmed_range_in_parent(slf: PyRef<'_, Self>) -> PyResult<Option<PyTimeRange>> {
        let (shared, id) = transition_handle(&slf).live()?;
        shared
            .read(|document| Ok(core_error(document.trimmed_range_in_parent(id))?.map(PyTimeRange)))
    }

    fn __str__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = transition_handle(&slf);
        let [kind, in_offset, out_offset, metadata, enabled] =
            transition_fields(py, &handle, false)?;
        Ok(format!(
            "Transition(\"{}\", \"{kind}\", {in_offset}, {out_offset}, {metadata}, {enabled})",
            name_str(&handle)?
        ))
    }

    fn __repr__(slf: PyRef<'_, Self>, py: Python<'_>) -> PyResult<String> {
        let handle = transition_handle(&slf);
        let [kind, in_offset, out_offset, metadata, enabled] =
            transition_fields(py, &handle, true)?;
        Ok(format!(
            "otio.schema.Transition(name={}, transition_type={kind}, in_offset={in_offset}, \
             out_offset={out_offset}, metadata={metadata}, enabled={enabled})",
            name_repr(py, &handle)?
        ))
    }
}

/// Runs `f` on a transition's own fields.
fn with_transition<T>(handle: &Handle, f: impl FnOnce(&Transition) -> PyResult<T>) -> PyResult<T> {
    handle.with(|node| match node {
        Node::Transition(transition) => f(transition),
        _ => Err(PyValueError::new_err("not a transition")),
    })
}

/// Runs `f` on a transition's own fields, for writing.
fn with_transition_mut<T>(
    handle: &Handle,
    f: impl FnOnce(&mut Transition) -> PyResult<T>,
) -> PyResult<T> {
    handle.with_mut(|node| match node {
        Node::Transition(transition) => f(transition),
        _ => Err(PyValueError::new_err("not a transition")),
    })
}

/// The five fields upstream prints for a transition, past its name.
fn transition_fields(py: Python<'_>, handle: &Handle, quoted: bool) -> PyResult<[String; 5]> {
    let transition = with_transition(handle, |transition| Ok(transition.clone()))?;
    let render = |time: RationalTime| -> PyResult<String> {
        let time = PyRationalTime(time).into_py_any(py)?;
        Ok(if quoted {
            time.bind(py).repr()?.to_string()
        } else {
            time.bind(py).str()?.to_string()
        })
    };
    Ok([
        if quoted {
            py_repr(py, &transition.transition_type)?
        } else {
            transition.transition_type.clone()
        },
        render(transition.in_offset)?,
        render(transition.out_offset)?,
        metadata_repr(handle, py)?,
        if transition.enabled { "True" } else { "False" }.to_string(),
    ])
}

/// Returns a composition's children, or an empty list if it holds none.
fn children_of(handle: &Handle) -> PyResult<Vec<NodeId>> {
    Ok(handle
        .with(|node| Ok(node.children().map(<[NodeId]>::to_vec)))?
        .unwrap_or_default())
}

/// Returns a composition's children, wrapped, in an ordinary list.
fn children_list(py: Python<'_>, handle: &Handle) -> PyResult<Py<PyAny>> {
    let (shared, _) = handle.live()?;
    wrappers(py, &shared, &children_of(handle)?)
}

/// Wraps a run of nodes from one document into an ordinary list.
fn wrappers(py: Python<'_>, shared: &Shared, ids: &[NodeId]) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for id in ids.iter().copied() {
        list.append(wrap(
            py,
            &Handle {
                shared: shared.clone(),
                id,
            },
        )?)?;
    }
    list.into_py_any(py)
}

/// Turns a Python index into one a composition holds, refusing one past the
/// end as indexing a list does.
///
/// The `IndexError`'s message is upstream's: none at all when reading, as
/// its binding raises a bare `pybind11::index_error`, and `illegal index`
/// when writing or deleting, where the C++ call reports `ILLEGAL_INDEX`.
fn child_index(index: isize, len: usize, message: &'static str) -> PyResult<usize> {
    let length = isize::try_from(len).map_err(|_| PyIndexError::new_err("list is too long"))?;
    let resolved = if index < 0 { index + length } else { index };
    usize::try_from(resolved)
        .ok()
        .filter(|resolved| *resolved < len)
        .ok_or_else(|| PyIndexError::new_err(message))
}

/// What reading a child past the end raises: `IndexError` with no message.
const READ_PAST_END: &str = "";
/// What writing or deleting a child past the end raises.
const WRITE_PAST_END: &str = "illegal index";

/// Returns a parent and a child of the same document, for a call that needs
/// both.
///
/// `unrelated` is the error for a child from another document, which cannot
/// be among the parent's children or below it; the parent is the object it
/// names, as upstream names it.
fn pair(
    parent: &Handle,
    child: &Bound<'_, PyAny>,
    unrelated: fn(NodeId) -> Error,
) -> PyResult<(Shared, NodeId, NodeId)> {
    let (shared, parent_id) = parent.live()?;
    let (home, child) = handle_of(child)?.live()?;
    // An id only means something in the document it was read from. Two
    // documents built separately hand out the same ids from the start, so the
    // first child of one track and the first child of another almost always
    // share one, and pairing this parent with a raw id from elsewhere would
    // quietly answer about whichever object happened to sit there. Upstream
    // compares the objects themselves and finds no match, so it raises.
    if !shared.is(&home)? {
        let parent = Handle {
            shared: shared.clone(),
            id: parent_id,
        };
        return Err(exception(unrelated(parent_id), Some(parent)));
    }
    Ok((shared, parent_id, child))
}

/// The error for an object looked up among a composition's children when it
/// is not one of them.
fn not_a_child_of(parent: NodeId) -> Error {
    Error::NotAChildOf {
        parent: String::new(),
        object: Some(parent),
    }
}

/// The error for an object looked up below a composition when it is not
/// there.
fn not_descended_from(parent: NodeId) -> Error {
    Error::NotDescendedFrom {
        parent: String::new(),
        object: Some(parent),
    }
}

/// Resolves a child that may not be in the same document yet.
///
/// A timeline's `range_of_child` is given an object the caller is holding,
/// which has always come from the timeline in practice; moving it in if it
/// has not is what upstream's reference counting does for free.
fn adopt_free(home: &Handle, value: &Bound<'_, PyAny>) -> PyResult<NodeId> {
    adopt_into(home, value)
}

/// Every descendant of `handle` matching `descended_from_type`, in document
/// order.
fn find_children_below(
    py: Python<'_>,
    handle: &Handle,
    descended_from_type: Option<&Bound<'_, PyAny>>,
    search_range: Option<PyTimeRange>,
    shallow_search: bool,
) -> PyResult<Py<PyAny>> {
    let (shared, id) = handle.live()?;
    let found = shared.read(|document| {
        core_error(document.find_children(
            id,
            search_range.map(|range| range.0),
            shallow_search,
            &|_: &Node| true,
        ))
    })?;

    // Upstream filters by Python type rather than by schema, so a subclass
    // defined in Python matches its base class here too. That can only be
    // decided once each object has its wrapper.
    let list = PyList::empty(py);
    for id in found {
        let wrapper = wrap(
            py,
            &Handle {
                shared: shared.clone(),
                id,
            },
        )?;
        match descended_from_type {
            Some(kind) if !kind.is_none() && !wrapper.is_instance(kind)? => continue,
            _ => list.append(wrapper)?,
        }
    }
    list.into_py_any(py)
}

/// Every clip below `handle`, in document order.
fn find_clips_below(
    py: Python<'_>,
    handle: &Handle,
    search_range: Option<PyTimeRange>,
    shallow_search: bool,
) -> PyResult<Py<PyAny>> {
    let (shared, id) = handle.live()?;
    let found = shared.read(|document| {
        core_error(document.find_children(
            id,
            search_range.map(|range| range.0),
            shallow_search,
            &|node: &Node| matches!(node, Node::Clip(_)),
        ))
    })?;
    wrappers(py, &shared, &found)
}

/// Renders a composition the way upstream's `__str__` does.
fn composition_str(py: Python<'_>, schema: &str, handle: &Handle) -> PyResult<String> {
    let children = children_list(py, handle)?;
    let range = handle.with(|node| {
        Ok(node
            .item()
            .and_then(|item| item.source_range)
            .map(PyTimeRange))
    })?;
    Ok(format!(
        "{schema}({}, {}, {}, {})",
        name_str(handle)?,
        children.bind(py).str()?,
        optional_str(py, range)?,
        metadata_repr(handle, py)?
    ))
}

/// Renders a composition the way upstream's `__repr__` does.
fn composition_repr(py: Python<'_>, schema: &str, handle: &Handle) -> PyResult<String> {
    let children = children_list(py, handle)?;
    let range = handle.with(|node| {
        Ok(node
            .item()
            .and_then(|item| item.source_range)
            .map(PyTimeRange))
    })?;
    let color =
        handle.with(|node| Ok(node.item().and_then(|item| item.color.clone()).map(PyColor)))?;
    Ok(format!(
        "{schema}(name={}, children={}, source_range={}, color={}, metadata={})",
        name_repr(py, handle)?,
        children.bind(py).repr()?,
        optional_repr(py, range)?,
        optional_repr(py, color)?,
        metadata_repr(handle, py)?
    ))
}

/// Returns the handle inside any wrapped object.
///
/// Fails with a `TypeError` if the value is not one; callers use that to
/// tell an OTIO object from an ordinary Python value.
pub fn handle_of(value: &Bound<'_, PyAny>) -> PyResult<Handle> {
    Ok(value
        .extract::<PyRef<'_, PySerializableObject>>()?
        .0
        .clone())
}

/// Builds an object in a document of its own, then fills in its metadata.
///
/// The two steps are in that order because metadata may itself hold OTIO
/// objects, and those have to be moved into this object's document — which
/// does not exist until the object does.
fn alone_with(
    build: impl FnOnce(Base) -> Node,
    name: String,
    metadata: Option<&Bound<'_, PyAny>>,
) -> PyResult<Handle> {
    let handle = Handle::alone(build(Base {
        name,
        metadata: AnyDictionary::new(),
    }));
    if let Some(metadata) = metadata {
        let entries = dictionary_from(&handle.shared, metadata)?;
        handle.with_base_mut(|base| {
            base.metadata = entries;
            Ok(())
        })?;
    }
    Ok(handle)
}

/// Reads a Python mapping into a metadata dictionary.
fn dictionary_from(home: &Shared, value: &Bound<'_, PyAny>) -> PyResult<AnyDictionary> {
    match python_to_any(home, value)? {
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

/// Turns a Python insertion index into one this list can take.
///
/// `list.insert` clamps rather than refusing, so `insert(-99, x)` puts `x`
/// first and `insert(99, x)` puts it last; these sequences do the same.
fn clamped_index(index: isize, len: usize) -> PyResult<usize> {
    let length = isize::try_from(len).map_err(|_| PyIndexError::new_err("list is too long"))?;
    Ok(if index < 0 {
        usize::try_from(index + length).unwrap_or(0)
    } else {
        usize::try_from(index).unwrap_or(len).min(len)
    })
}

/// Renders a string the way Python's `repr()` would, quotes and all.
///
/// Python quotes with apostrophes and Rust with double quotes, and upstream's
/// tests compare the text exactly, so the formatting is left to Python.
fn py_repr(py: Python<'_>, value: &str) -> PyResult<String> {
    Ok(PyString::new(py, value).repr()?.to_string())
}

/// Renders an object's name the way Python's `repr()` would, quotes and all.
///
/// `__str__` and `__repr__` differ here and nowhere else in these classes:
/// upstream builds one from `str(self.name)` and the other from
/// `repr(self.name)`, and its own test compares both.
fn name_repr(py: Python<'_>, handle: &Handle) -> PyResult<String> {
    py_repr(py, &name_str(handle)?)
}

/// Renders an object's metadata the way Python's `str()` would.
fn metadata_repr(handle: &Handle, py: Python<'_>) -> PyResult<String> {
    PyMetadata::of(handle.clone()).__repr__(py)
}

/// Serializes one object, for comparing two of them.
fn write_one(handle: &Handle) -> PyResult<String> {
    let (shared, id) = handle.live()?;
    shared.read(|document| {
        core_error(otio_core::to_string_pretty_from(
            document,
            id,
            otio_core::DEFAULT_INDENT,
        ))
    })
}

/// Registers the object model on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PySerializableObject>()?;
    module.add_class::<PySerializableObjectWithMetadata>()?;
    module.add_class::<PyComposable>()?;
    module.add_class::<PyItem>()?;
    module.add_class::<PyGap>()?;
    module.add_class::<PyMarker>()?;
    module.add_class::<PyEffect>()?;
    module.add_class::<PyTimeEffect>()?;
    module.add_class::<PyLinearTimeWarp>()?;
    module.add_class::<PyFreezeFrame>()?;
    module.add_class::<PyMediaReference>()?;
    module.add_class::<PyMissingReference>()?;
    module.add_class::<PyExternalReference>()?;
    module.add_class::<PyGeneratorReference>()?;
    module.add_class::<PyImageSequenceReference>()?;
    module.add_class::<PyMissingFramePolicy>()?;
    module.add_class::<PyClip>()?;
    module.add_class::<PyComposition>()?;
    module.add_class::<PyTrack>()?;
    module.add_class::<PyStack>()?;
    module.add_class::<PyTimeline>()?;
    module.add_class::<PySerializableCollection>()?;
    module.add_class::<PyTransition>()?;
    module.add_class::<NeighborPolicy>()?;
    module.add_class::<PyNodeList>()?;
    module.add_class::<PyMetadata>()?;
    Ok(())
}

/// Builds the Python wrapper for a node, reusing the one it already has.
pub fn wrap<'py>(py: Python<'py>, handle: &Handle) -> PyResult<Bound<'py, PyAny>> {
    let shared = handle.shared.clone();
    let id = handle.id;
    shared.clone().wrapper_for(py, id, || {
        let handle = Handle { shared, id };
        let (schema, dynamic) = handle.with(|node| {
            Ok(match node {
                // A schema nobody registered, and one registered at run
                // time, are told apart by variant rather than by name: their
                // names are anybody's.
                Node::Unknown(_) => ("UnknownSchema".to_string(), false),
                Node::Dynamic(dynamic) => (dynamic.schema_name.clone(), true),
                _ => (node.schema_name().to_string(), false),
            })
        })?;
        if dynamic {
            if let Some(instance) = crate::registry::wrap_dynamic(py, &handle, &schema)? {
                return Ok(instance);
            }
        }
        let with_base = dynamic && handle.with(|node| Ok(node.base().is_some()))?;
        let object = PySerializableObject(handle);
        if dynamic {
            // A run-time schema with no class registered here, or one of
            // upstream's root classes carrying dynamic fields.
            return if with_base {
                Py::new(
                    py,
                    PyClassInitializer::from(object).add_subclass(PySerializableObjectWithMetadata),
                )?
                .into_bound_py_any(py)
            } else {
                Py::new(py, object)?.into_bound_py_any(py)
            };
        }
        if schema == "UnknownSchema" {
            return Py::new(
                py,
                PyClassInitializer::from(object).add_subclass(crate::registry::PyUnknownSchema),
            )?
            .into_bound_py_any(py);
        }
        let with_metadata = || {
            PyClassInitializer::from(PySerializableObject(object.0.clone()))
                .add_subclass(PySerializableObjectWithMetadata)
        };
        match schema.as_str() {
            "Composable" => Py::new(py, composable_initializer(object.0))?.into_bound_py_any(py),
            "Item" => Py::new(py, composable_initializer(object.0).add_subclass(PyItem))?
                .into_bound_py_any(py),
            "Gap" => Py::new(
                py,
                composable_initializer(object.0)
                    .add_subclass(PyItem)
                    .add_subclass(PyGap),
            )?
            .into_bound_py_any(py),
            "Marker" => Py::new(py, with_metadata().add_subclass(PyMarker))?.into_bound_py_any(py),
            "Effect" => Py::new(py, with_metadata().add_subclass(PyEffect))?.into_bound_py_any(py),
            "TimeEffect" => Py::new(
                py,
                with_metadata()
                    .add_subclass(PyEffect)
                    .add_subclass(PyTimeEffect),
            )?
            .into_bound_py_any(py),
            "LinearTimeWarp" => Py::new(
                py,
                with_metadata()
                    .add_subclass(PyEffect)
                    .add_subclass(PyTimeEffect)
                    .add_subclass(PyLinearTimeWarp),
            )?
            .into_bound_py_any(py),
            "FreezeFrame" => Py::new(
                py,
                with_metadata()
                    .add_subclass(PyEffect)
                    .add_subclass(PyTimeEffect)
                    .add_subclass(PyLinearTimeWarp)
                    .add_subclass(PyFreezeFrame),
            )?
            .into_bound_py_any(py),
            "MediaReference" => Py::new(py, media_initializer(object.0))?.into_bound_py_any(py),
            "MissingReference" => Py::new(
                py,
                media_initializer(object.0).add_subclass(PyMissingReference),
            )?
            .into_bound_py_any(py),
            "ExternalReference" => Py::new(
                py,
                media_initializer(object.0).add_subclass(PyExternalReference),
            )?
            .into_bound_py_any(py),
            "GeneratorReference" => Py::new(
                py,
                media_initializer(object.0).add_subclass(PyGeneratorReference),
            )?
            .into_bound_py_any(py),
            "ImageSequenceReference" => Py::new(
                py,
                media_initializer(object.0).add_subclass(PyImageSequenceReference),
            )?
            .into_bound_py_any(py),
            "Clip" => Py::new(
                py,
                composable_initializer(object.0)
                    .add_subclass(PyItem)
                    .add_subclass(PyClip),
            )?
            .into_bound_py_any(py),
            "Composition" => Py::new(py, composition_initializer(object.0))?.into_bound_py_any(py),
            "Track" => Py::new(py, composition_initializer(object.0).add_subclass(PyTrack))?
                .into_bound_py_any(py),
            "Stack" => Py::new(py, composition_initializer(object.0).add_subclass(PyStack))?
                .into_bound_py_any(py),
            "Timeline" => {
                Py::new(py, with_metadata().add_subclass(PyTimeline))?.into_bound_py_any(py)
            }
            "Transition" => Py::new(
                py,
                composable_initializer(object.0).add_subclass(PyTransition),
            )?
            .into_bound_py_any(py),
            "SerializableCollection" => {
                Py::new(py, with_metadata().add_subclass(PySerializableCollection))?
                    .into_bound_py_any(py)
            }
            "SerializableObjectWithMetadata" => Py::new(py, with_metadata())?.into_bound_py_any(py),
            _ => Py::new(py, object)?.into_bound_py_any(py),
        }
    })
}

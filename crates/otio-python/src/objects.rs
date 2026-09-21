//! The object model: the classes `opentimelineio.core` and
//! `opentimelineio.schema` are built from.
//!
//! Each class here wraps a node in a document; see [`crate::arena`] for why,
//! and for the rules every method follows about borrowing.

use opentime::{RationalTime, TimeRange};

use otio_core::schema::{Base, Composable, EffectData, Gap, ItemData, Marker, Node};
use otio_core::{Any, AnyDictionary, Error, NodeId};

use pyo3::exceptions::{
    PyIndexError, PyKeyError, PyNotImplementedError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyIterator, PyList, PyString, PyTuple};
use pyo3::{IntoPyObjectExt, Py, PyAny};

use crate::arena::Shared;
use crate::opentime::{PyRationalTime, PyTimeRange};
use crate::values::{PyColor, any_to_python, python_to_any};

/// Turns an `otio-core` failure into a Python exception.
///
/// Upstream raises a handful of dedicated exception types from
/// `opentimelineio.exceptions`; until those exist here, everything arrives as
/// `ValueError`, which is what its binding layer falls back to.
pub fn core_error<T>(result: Result<T, Error>) -> PyResult<T> {
    result.map_err(|error: Error| match error {
        // Upstream's base classes leave some questions to their subclasses
        // and report NOT_IMPLEMENTED for them, which its bindings raise as
        // `NotImplementedError`. Its own tests check for that exact type.
        Error::NotImplemented { .. } | Error::NoLayout => {
            PyNotImplementedError::new_err(error.to_string())
        }
        other => PyValueError::new_err(other.to_string()),
    })
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

    /// Records this wrapper as the one for its node.
    ///
    /// Python calls `__init__` after `__new__`, and `__new__` is where every
    /// constructor here builds the object; this is the first point at which
    /// the Python object exists to be remembered. Every class in this module
    /// inherits it, so every constructor registers.
    ///
    /// The arguments are ignored: each subclass's `__new__` has already read
    /// them.
    #[pyo3(signature = (*_args, **_kwargs))]
    fn __init__(
        slf: &Bound<'_, Self>,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let handle = slf.borrow().0.clone();
        let (shared, id) = handle.live()?;
        shared.remember(id, slf.as_any())
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
        handle_of(other).map_or(Ok(false), |other| self.0.same(&other))
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
        let handle = alone_with(Node::SerializableObjectWithMetadata, name, metadata)?;
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
    module = "opentimelineio.core",
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
        metadata = None,
    ))]
    fn new(
        name: String,
        source_range: Option<PyTimeRange>,
        effects: Option<&Bound<'_, PyAny>>,
        markers: Option<&Bound<'_, PyAny>>,
        enabled: bool,
        metadata: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let handle = new_item(
            Node::Item,
            name,
            source_range,
            effects,
            markers,
            enabled,
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
    module = "opentimelineio.schema",
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
            (None, range) => range.map(|range| range.0),
        };

        let handle = new_item(
            |item| Node::Gap(Gap { item }),
            name,
            range.map(PyTimeRange),
            effects,
            markers,
            enabled,
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
    module = "opentimelineio.schema",
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
    #[pyo3(signature = (
        name = String::new(),
        marked_range = None,
        color = None,
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
    module = "opentimelineio.schema",
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

/// A constant-rate speed change.
#[pyclass(
    name = "LinearTimeWarp",
    module = "opentimelineio.schema",
    extends = PyEffect,
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
        Ok(effect_initializer(handle).add_subclass(Self))
    }

    #[getter]
    fn time_scalar(slf: PyRef<'_, Self>) -> PyResult<f64> {
        time_scalar_of(&metadata_handle(&slf.into_super()))
    }

    #[setter]
    fn set_time_scalar(slf: PyRef<'_, Self>, value: f64) -> PyResult<()> {
        set_time_scalar(&metadata_handle(&slf.into_super()), value)
    }
}

/// A hold on a single frame.
#[pyclass(
    name = "FreezeFrame",
    module = "opentimelineio.schema",
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
fn new_item(
    build: impl FnOnce(ItemData) -> Node,
    name: String,
    source_range: Option<PyTimeRange>,
    effects: Option<&Bound<'_, PyAny>>,
    markers: Option<&Bound<'_, PyAny>>,
    enabled: bool,
    metadata: Option<&Bound<'_, PyAny>>,
) -> PyResult<Handle> {
    let handle = alone_with(
        |base| {
            build(ItemData {
                base,
                source_range: source_range.map(|range| range.0),
                enabled,
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
        list.__repr__(py)
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
        wrap(
            py,
            &Handle {
                shared: self.handle.shared.clone(),
                id,
            },
        )
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

    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<Py<PyAny>> {
        let at = self.at(index)?;
        let id = self.ids()?[at];
        Ok(self.wrapper(py, id)?.unbind())
    }

    fn __setitem__(&self, index: isize, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let at = self.at(index)?;
        let id = self.adopt(value)?;
        self.with_list(|list| {
            list[at] = id;
            Ok(())
        })
    }

    fn __delitem__(&self, index: isize) -> PyResult<()> {
        let at = self.at(index)?;
        self.with_list(|list| {
            list.remove(at);
            Ok(())
        })
    }

    /// Inserts `value` before `index`, as `MutableSequence` requires.
    ///
    /// Everything else a list can do — `append`, `extend`, `remove`, `pop` —
    /// is written once in the standard library in terms of this and the four
    /// methods above, and the Python layer borrows it.
    fn insert(&self, index: isize, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let len = self.ids()?.len();
        let length = isize::try_from(len).map_err(|_| PyIndexError::new_err("list is too long"))?;
        let at = if index < 0 {
            usize::try_from(index + length).unwrap_or(0)
        } else {
            usize::try_from(index).unwrap_or(len).min(len)
        };
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

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(self.to_list(py)?.bind(py).repr()?.to_string())
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        self.__repr__(py)
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
        // The value is copied out before it is turned into a Python object,
        // because a metadata value may itself be an object, and building its
        // wrapper reads the document again. See [`crate::arena`]: a borrow
        // lasts one call and no longer.
        let value = self.0.with(|node| {
            node.base()
                .and_then(|base| base.metadata.get(key))
                .cloned()
                .ok_or_else(|| PyKeyError::new_err(key.to_string()))
        })?;
        any_to_python(py, &self.0.shared, &value)
    }

    fn __setitem__(&self, key: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let value = python_to_any(&self.0.shared, value)?;
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
        let entries = self.0.with(|node| {
            Ok(node
                .base()
                .map(|base| base.metadata.clone())
                .unwrap_or_default())
        })?;
        let dict = PyDict::new(py);
        for (key, value) in &entries {
            dict.set_item(key, any_to_python(py, &self.0.shared, value)?)?;
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
    module.add_class::<PyItem>()?;
    module.add_class::<PyGap>()?;
    module.add_class::<PyMarker>()?;
    module.add_class::<PyEffect>()?;
    module.add_class::<PyLinearTimeWarp>()?;
    module.add_class::<PyFreezeFrame>()?;
    module.add_class::<PyNodeList>()?;
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
    if let Ok(handle) = handle_of(value) {
        let (shared, id) = handle.live()?;
        return shared
            .read(|document| core_error(otio_core::to_string_pretty_from(document, id, indent)));
    }

    // Upstream's writer takes anything, not only objects: its own tests
    // serialize an item's marker list and a bare boolean in order to compare
    // them. A value with no objects in it needs no document at all.
    let home = crate::values::home_of(value).unwrap_or_default();
    let any = python_to_any(&home, value)?;
    home.read(|document| core_error(otio_core::to_string_any_pretty(document, &any, indent)))
}

/// Builds the Python wrapper for a node, reusing the one it already has.
pub fn wrap<'py>(py: Python<'py>, handle: &Handle) -> PyResult<Bound<'py, PyAny>> {
    let shared = handle.shared.clone();
    let id = handle.id;
    shared.clone().wrapper_for(py, id, || {
        let handle = Handle { shared, id };
        let schema = handle.with(|node| Ok(node.schema_name().to_string()))?;
        let object = PySerializableObject(handle);
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
            "Effect" | "TimeEffect" => {
                Py::new(py, with_metadata().add_subclass(PyEffect))?.into_bound_py_any(py)
            }
            "LinearTimeWarp" => Py::new(
                py,
                with_metadata()
                    .add_subclass(PyEffect)
                    .add_subclass(PyLinearTimeWarp),
            )?
            .into_bound_py_any(py),
            "FreezeFrame" => Py::new(
                py,
                with_metadata()
                    .add_subclass(PyEffect)
                    .add_subclass(PyLinearTimeWarp)
                    .add_subclass(PyFreezeFrame),
            )?
            .into_bound_py_any(py),
            "SerializableObjectWithMetadata" => Py::new(py, with_metadata())?.into_bound_py_any(py),
            _ => Py::new(py, object)?.into_bound_py_any(py),
        }
    })
}

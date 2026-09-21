//! Metadata values, and the small geometry types that can appear in them.
//!
//! Upstream stores metadata as an `AnyDictionary` whose values are anything
//! the serializer knows how to write, including times, colours and geometry.
//! This module carries those across the boundary in both directions.

use otio_core::{Any, AnyDictionary, Box2d, Color, V2d};

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use pyo3::{IntoPyObjectExt, Py, PyAny};

use crate::opentime::{PyRationalTime, PyTimeRange, PyTimeTransform};

/// A two-dimensional point.
#[pyclass(name = "V2d", module = "opentimelineio.schema", frozen, from_py_object)]
#[derive(Clone, Copy)]
pub struct PyV2d(pub V2d);

#[pymethods]
impl PyV2d {
    #[new]
    #[pyo3(signature = (x = 0.0, y = 0.0))]
    const fn new(x: f64, y: f64) -> Self {
        Self(V2d::new(x, y))
    }

    #[getter]
    const fn x(&self) -> f64 {
        self.0.x
    }

    #[getter]
    const fn y(&self) -> f64 {
        self.0.y
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<Self>().is_ok_and(|point| self.0 == point.0)
    }

    fn __repr__(&self) -> String {
        format!("otio.schema.V2d(x={}, y={})", self.0.x, self.0.y)
    }

    fn __str__(&self) -> String {
        format!("V2d({}, {})", self.0.x, self.0.y)
    }
}

/// An axis-aligned rectangle.
#[pyclass(
    name = "Box2d",
    module = "opentimelineio.schema",
    frozen,
    from_py_object
)]
#[derive(Clone, Copy)]
pub struct PyBox2d(pub Box2d);

#[pymethods]
impl PyBox2d {
    #[new]
    #[pyo3(signature = (min = None, max = None))]
    fn new(min: Option<&PyV2d>, max: Option<&PyV2d>) -> Self {
        Self(Box2d {
            min: min.map_or_else(V2d::default, |point| point.0),
            max: max.map_or_else(V2d::default, |point| point.0),
        })
    }

    #[getter]
    const fn min(&self) -> PyV2d {
        PyV2d(self.0.min)
    }

    #[getter]
    const fn max(&self) -> PyV2d {
        PyV2d(self.0.max)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<Self>().is_ok_and(|box2d| self.0 == box2d.0)
    }

    fn __repr__(&self) -> String {
        format!(
            "otio.schema.Box2d(min={}, max={})",
            PyV2d(self.0.min).__repr__(),
            PyV2d(self.0.max).__repr__()
        )
    }

    fn __str__(&self) -> String {
        format!(
            "Box2d({}, {})",
            PyV2d(self.0.min).__str__(),
            PyV2d(self.0.max).__str__()
        )
    }
}

/// A colour, as used to tint a marker or a clip.
#[pyclass(name = "Color", module = "opentimelineio.core", frozen, from_py_object)]
#[derive(Clone)]
pub struct PyColor(pub Color);

#[pymethods]
impl PyColor {
    #[new]
    #[pyo3(signature = (r = 0.0, g = 0.0, b = 0.0, a = 1.0, name = String::new()))]
    fn new(r: f64, g: f64, b: f64, a: f64, name: String) -> Self {
        Self(Color::new(r, g, b, a, name))
    }

    #[getter]
    const fn r(&self) -> f64 {
        self.0.r
    }

    #[getter]
    const fn g(&self) -> f64 {
        self.0.g
    }

    #[getter]
    const fn b(&self) -> f64 {
        self.0.b
    }

    #[getter]
    const fn a(&self) -> f64 {
        self.0.a
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<Self>().is_ok_and(|color| self.0 == color.0)
    }

    fn __repr__(&self) -> String {
        format!(
            "otio.core.Color(r={}, g={}, b={}, a={}, name={:?})",
            self.0.r, self.0.g, self.0.b, self.0.a, self.0.name
        )
    }
}

/// Turns a metadata value into the Python object upstream would hand back.
pub fn any_to_python(py: Python<'_>, value: &Any) -> PyResult<Py<PyAny>> {
    match value {
        Any::Null => Ok(py.None()),
        Any::Bool(value) => value.into_py_any(py),
        Any::Int(value) => value.into_py_any(py),
        Any::UInt(value) => value.into_py_any(py),
        Any::Double(value) => value.into_py_any(py),
        Any::String(value) => value.into_py_any(py),
        Any::RationalTime(time) => PyRationalTime(*time).into_py_any(py),
        Any::TimeRange(range) => PyTimeRange(*range).into_py_any(py),
        Any::TimeTransform(transform) => PyTimeTransform(*transform).into_py_any(py),
        Any::Color(color) => PyColor(color.clone()).into_py_any(py),
        Any::V2d(point) => PyV2d(*point).into_py_any(py),
        Any::Box2d(box2d) => PyBox2d(*box2d).into_py_any(py),
        Any::Vector(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(any_to_python(py, item)?)?;
            }
            list.into_py_any(py)
        }
        Any::Dictionary(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries {
                dict.set_item(key, any_to_python(py, item)?)?;
            }
            dict.into_py_any(py)
        }
        // A metadata value that refers to an object in the document has no
        // Python form until the object model is bound; it is written back out
        // unchanged either way.
        Any::Object(_) => Err(PyTypeError::new_err(
            "metadata holding another OTIO object cannot be read from Python yet",
        )),
        // `Any` is `#[non_exhaustive]`, so a value of a kind added after this
        // was written reaches here. It is still written back out unchanged.
        _ => Err(PyTypeError::new_err(
            "metadata holds a value of a kind these bindings do not know",
        )),
    }
}

/// Turns a Python object into a metadata value.
///
/// The order the cases are tried in matters: `bool` is a subclass of `int` in
/// Python, so it has to be checked first or `True` becomes `1`.
pub fn python_to_any(value: &Bound<'_, PyAny>) -> PyResult<Any> {
    if value.is_none() {
        return Ok(Any::Null);
    }
    if let Ok(flag) = value.cast::<PyBool>() {
        return Ok(Any::Bool(flag.is_true()));
    }
    if let Ok(number) = value.cast::<PyInt>() {
        return number
            .extract::<i64>()
            .map(Any::Int)
            .or_else(|_| number.extract::<u64>().map(Any::UInt));
    }
    if let Ok(number) = value.cast::<PyFloat>() {
        return Ok(Any::Double(number.extract()?));
    }
    if let Ok(text) = value.cast::<PyString>() {
        return Ok(Any::String(text.extract()?));
    }
    if let Ok(time) = value.extract::<PyRationalTime>() {
        return Ok(Any::RationalTime(time.0));
    }
    if let Ok(range) = value.extract::<PyTimeRange>() {
        return Ok(Any::TimeRange(range.0));
    }
    if let Ok(transform) = value.extract::<PyTimeTransform>() {
        return Ok(Any::TimeTransform(transform.0));
    }
    if let Ok(color) = value.extract::<PyColor>() {
        return Ok(Any::Color(color.0));
    }
    if let Ok(point) = value.extract::<PyV2d>() {
        return Ok(Any::V2d(point.0));
    }
    if let Ok(box2d) = value.extract::<PyBox2d>() {
        return Ok(Any::Box2d(box2d.0));
    }
    if let Ok(dict) = value.cast::<PyDict>() {
        let mut entries = AnyDictionary::new();
        for (key, item) in dict {
            entries.insert(key.extract::<String>()?, python_to_any(&item)?);
        }
        return Ok(Any::Dictionary(entries));
    }
    if value.cast::<PyList>().is_ok() || value.cast::<PyTuple>().is_ok() {
        let mut items = Vec::new();
        for item in value.try_iter()? {
            items.push(python_to_any(&item?)?);
        }
        return Ok(Any::Vector(items));
    }
    let name = value
        .get_type()
        .name()
        .map_or_else(|_| "object".to_string(), |name| name.to_string());
    Err(PyTypeError::new_err(format!(
        "cannot store a {name} in metadata"
    )))
}

/// Registers the value types on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyV2d>()?;
    module.add_class::<PyBox2d>()?;
    module.add_class::<PyColor>()?;
    Ok(())
}

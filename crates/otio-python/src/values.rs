//! Metadata values, and the small geometry types that can appear in them.
//!
//! Upstream stores metadata as an `AnyDictionary` whose values are anything
//! the serializer knows how to write, including times, colours and geometry.
//! This module carries those across the boundary in both directions.

use otio_core::{Any, AnyDictionary, Box2d, Color, V2d};

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use pyo3::{IntoPyObjectExt, Py, PyAny};

use crate::arena::Shared;
use crate::opentime::{PyRationalTime, PyTimeRange, PyTimeTransform};

/// A two-dimensional point.
#[pyclass(name = "V2d", module = "opentimelineio._otio", frozen, from_py_object)]
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

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "otio.schema.V2d(x={}, y={})",
            float_repr(py, self.0.x)?,
            float_repr(py, self.0.y)?
        ))
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "V2d({}, {})",
            float_repr(py, self.0.x)?,
            float_repr(py, self.0.y)?
        ))
    }
}

/// Renders a number the way Python's `repr()` would.
///
/// Rust prints `0.0` as `0` and `1e21` as `1000000000000000000000`; Python
/// does neither, and upstream's tests compare the text.
fn float_repr(py: Python<'_>, value: f64) -> PyResult<String> {
    Ok(value.into_pyobject(py)?.repr()?.to_string())
}

/// An axis-aligned rectangle.
#[pyclass(
    name = "Box2d",
    module = "opentimelineio._otio",
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

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "otio.schema.Box2d(min={}, max={})",
            PyV2d(self.0.min).__repr__(py)?,
            PyV2d(self.0.max).__repr__(py)?
        ))
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Box2d({}, {})",
            PyV2d(self.0.min).__str__(py)?,
            PyV2d(self.0.max).__str__(py)?
        ))
    }
}

/// A colour, as used to tint a marker or a clip.
#[pyclass(
    name = "Color",
    module = "opentimelineio._otio",
    frozen,
    from_py_object
)]
#[derive(Clone)]
pub struct PyColor(pub Color);

#[pymethods]
impl PyColor {
    // Upstream's defaults are opaque white, not black: a colour built with no
    // arguments is `Color(1, 1, 1, 1)`.
    #[new]
    #[pyo3(signature = (r = 1.0, g = 1.0, b = 1.0, a = 1.0, name = String::new()))]
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

    // Upstream compares the eight-bit form of each colour and ignores the
    // name, so the named `Color.RED` equals an unnamed red read from a file.
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<Self>()
            .is_ok_and(|color| self.0.looks_like(&color.0))
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
        !self.__eq__(other)
    }

    fn __hash__(&self) -> u64 {
        u64::from(self.0.to_agbr_integer())
    }

    /// The colour as `#rrggbbaa`.
    fn to_hex(&self) -> String {
        self.0.to_hex()
    }

    /// The colour's four components at `base` bits each.
    #[pyo3(signature = (base = 8))]
    fn to_rgba_int_list(&self, base: i32) -> [i64; 4] {
        self.0.to_rgba_int_list(base)
    }

    /// The colour packed into one 32-bit integer.
    fn to_agbr_integer(&self) -> u32 {
        self.0.to_agbr_integer()
    }

    /// The colour's four components as they are stored.
    fn to_rgba_float_list(&self) -> [f64; 4] {
        self.0.to_rgba_float_list()
    }

    /// Reads a colour from `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`.
    #[staticmethod]
    fn from_hex(color: &str) -> PyResult<Self> {
        Color::from_hex(color)
            .map(Self)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Reads a colour from three or four integers at `bit_depth` bits each.
    #[staticmethod]
    fn from_int_list(color: Vec<i64>, bit_depth: i32) -> PyResult<Self> {
        Color::from_int_list(&color, bit_depth)
            .map(Self)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Reads a colour from three or four components.
    #[staticmethod]
    fn from_float_list(color: Vec<f64>) -> PyResult<Self> {
        Color::from_float_list(&color)
            .map(Self)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    /// Reads a colour from one packed 32-bit integer.
    #[staticmethod]
    fn from_agbr_int(agbr: u32) -> Self {
        Self(Color::from_agbr_int(agbr))
    }

    fn __repr__(&self) -> String {
        format!(
            "otio.core.Color(r={}, g={}, b={}, a={}, name={:?})",
            self.0.r, self.0.g, self.0.b, self.0.a, self.0.name
        )
    }

    // The named colours upstream exposes as read-only static properties. A
    // `#[classattr]` is evaluated once when the module is built, which comes
    // to the same thing for a frozen class like this one.
    #[classattr]
    #[allow(non_snake_case)]
    fn PINK() -> Self {
        Self(Color::pink())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn RED() -> Self {
        Self(Color::red())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn ORANGE() -> Self {
        Self(Color::orange())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn YELLOW() -> Self {
        Self(Color::yellow())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn GREEN() -> Self {
        Self(Color::green())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn CYAN() -> Self {
        Self(Color::cyan())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn BLUE() -> Self {
        Self(Color::blue())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn PURPLE() -> Self {
        Self(Color::purple())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn MAGENTA() -> Self {
        Self(Color::magenta())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn BLACK() -> Self {
        Self(Color::black())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn WHITE() -> Self {
        Self(Color::white())
    }

    #[classattr]
    #[allow(non_snake_case)]
    fn TRANSPARENT() -> Self {
        Self(Color::transparent())
    }
}

/// Turns a metadata value into the Python object upstream would hand back.
///
/// `home` is the document the value's object handles refer to. Metadata may
/// hold whole OTIO objects, and a handle means nothing without the arena it
/// came from.
pub fn any_to_python(py: Python<'_>, home: &Shared, value: &Any) -> PyResult<Py<PyAny>> {
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
                list.append(any_to_python(py, home, item)?)?;
            }
            list.into_py_any(py)
        }
        Any::Dictionary(entries) => {
            let dict = PyDict::new(py);
            for (key, item) in entries {
                dict.set_item(key, any_to_python(py, home, item)?)?;
            }
            dict.into_py_any(py)
        }
        Any::Object(id) => Ok(crate::objects::wrap(
            py,
            &crate::objects::Handle {
                shared: home.clone(),
                id: *id,
            },
        )?
        .unbind()),
        // `Any` is `#[non_exhaustive]`, so a value of a kind added after this
        // was written reaches here. It is still written back out unchanged.
        _ => Err(PyTypeError::new_err(
            "metadata holds a value of a kind these bindings do not know",
        )),
    }
}

/// Turns a Python object into a metadata value.
///
/// `home` is the document the value is going into. An OTIO object given as a
/// value is moved there, because a handle only means something in one arena;
/// see [`crate::arena`].
///
/// The order the cases are tried in matters: `bool` is a subclass of `int` in
/// Python, so it has to be checked first or `True` becomes `1`.
pub fn python_to_any(home: &Shared, value: &Bound<'_, PyAny>) -> PyResult<Any> {
    if let Ok(handle) = crate::objects::handle_of(value) {
        home.absorb(&handle.shared)?;
        let (_, id) = handle.live()?;
        return Ok(Any::Object(id));
    }
    if value.is_none() {
        return Ok(Any::Null);
    }
    if let Ok(flag) = value.cast::<PyBool>() {
        return Ok(Any::Bool(flag.is_true()));
    }
    if let Ok(number) = value.cast::<PyInt>() {
        // Python's integers have no limit and OTIO's do: upstream stores a
        // signed 64-bit value and refuses anything that will not fit, rather
        // than writing a number that cannot be read back. Its own test checks
        // that `2 ** 63` raises `ValueError`.
        return number.extract::<i64>().map(Any::Int).map_err(|_| {
            PyValueError::new_err("an integer in metadata must fit in 64 signed bits")
        });
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
            entries.insert(key.extract::<String>()?, python_to_any(home, &item)?);
        }
        return Ok(Any::Dictionary(entries));
    }
    if value.cast::<PyList>().is_ok()
        || value.cast::<PyTuple>().is_ok()
        || value
            .extract::<PyRef<'_, crate::objects::PyNodeList>>()
            .is_ok()
    {
        let mut items = Vec::new();
        for item in value.try_iter()? {
            items.push(python_to_any(home, &item?)?);
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

/// Returns the document a value's OTIO objects already live in, if any.
///
/// Serializing must not move objects between documents, so the writer takes
/// the home from what it is given rather than making a new one.
pub fn home_of(value: &Bound<'_, PyAny>) -> Option<Shared> {
    if let Ok(handle) = crate::objects::handle_of(value) {
        return Some(handle.shared);
    }
    if let Ok(list) = value.extract::<PyRef<'_, crate::objects::PyNodeList>>() {
        return Some(list.home());
    }
    if let Ok(dict) = value.cast::<PyDict>() {
        return dict.values().iter().find_map(|item| home_of(&item));
    }
    if value.cast::<PyList>().is_ok() || value.cast::<PyTuple>().is_ok() {
        return value
            .try_iter()
            .ok()?
            .find_map(|item| item.ok().and_then(|item| home_of(&item)));
    }
    None
}

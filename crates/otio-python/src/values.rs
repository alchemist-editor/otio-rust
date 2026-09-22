//! Metadata values, and the small geometry types that can appear in them.
//!
//! Upstream stores metadata as an `AnyDictionary` whose values are anything
//! the serializer knows how to write, including times, colours and geometry.
//! This module carries those across the boundary in both directions.

use otio_core::{Any, AnyDictionary, Box2d, Color, V2d};

use pyo3::exceptions::{PyIndexError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use pyo3::{IntoPyObjectExt, Py, PyAny};

use crate::arena::Shared;
use crate::opentime::{PyRationalTime, PyTimeRange, PyTimeTransform};

/// A two-dimensional point.
///
/// Upstream binds Imath's `V2d` directly, so this carries Imath's surface:
/// its camel-case method names, its `^` for the dot product and `%` for the
/// cross product, and components that can be assigned. It is mutable, as
/// Imath's is: `normalize()` changes the vector in place.
#[pyclass(name = "V2d", module = "opentimelineio._otio", from_py_object)]
#[derive(Clone, Copy)]
pub struct PyV2d(pub V2d);

/// Reads the other operand of a comparison, as upstream's `_type_checked`
/// does: anything that is not the right type is a `TypeError`, not `False`.
fn type_checked<T: for<'a, 'py> FromPyObject<'a, 'py> + Clone>(
    rhs: &Bound<'_, PyAny>,
    class: &str,
    op: &str,
) -> PyResult<T> {
    rhs.extract::<T>().map_err(|_| {
        let rhs_type = rhs
            .get_type()
            .name()
            .map_or_else(|_| "object".to_string(), |name| name.to_string());
        PyTypeError::new_err(format!(
            "Unsupported operand type(s) for {class}: {op} and {rhs_type}"
        ))
    })
}

#[pymethods]
impl PyV2d {
    /// `V2d()`, `V2d(a)` for `(a, a)`, or `V2d(x, y)`, as Imath's three
    /// constructors.
    #[new]
    #[pyo3(signature = (x = None, y = None))]
    fn new(x: Option<f64>, y: Option<f64>) -> PyResult<Self> {
        match (x, y) {
            (None, None) => Ok(Self(V2d::new(0.0, 0.0))),
            (Some(a), None) => Ok(Self(V2d::new(a, a))),
            (Some(x), Some(y)) => Ok(Self(V2d::new(x, y))),
            (None, Some(_)) => Err(PyTypeError::new_err(
                "V2d() takes no arguments, one number, or two",
            )),
        }
    }

    #[getter]
    const fn x(&self) -> f64 {
        self.0.x
    }

    #[setter]
    const fn set_x(&mut self, x: f64) {
        self.0.x = x;
    }

    #[getter]
    const fn y(&self) -> f64 {
        self.0.y
    }

    #[setter]
    const fn set_y(&mut self, y: f64) {
        self.0.y = y;
    }

    // Imath does not check the index at all; reading past the end here is
    // an `IndexError`, which is also what lets `list(v)` stop.
    fn __getitem__(&self, index: usize) -> PyResult<f64> {
        match index {
            0 => Ok(self.0.x),
            1 => Ok(self.0.y),
            _ => Err(PyIndexError::new_err("V2d index out of range")),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 == type_checked::<Self>(other, "V2d", "==")?.0)
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 != type_checked::<Self>(other, "V2d", "!=")?.0)
    }

    fn __xor__(&self, other: &Bound<'_, PyAny>) -> PyResult<f64> {
        Ok(self.0.dot(type_checked::<Self>(other, "V2d", "^")?.0))
    }

    fn __mod__(&self, other: &Bound<'_, PyAny>) -> PyResult<f64> {
        Ok(self.0.cross(type_checked::<Self>(other, "V2d", "%")?.0))
    }

    // There are deliberately no in-place operators. Upstream's take their
    // left operand by value and return a new vector, so `v += w` rebinds `v`
    // and leaves any other name for the old vector alone; without
    // `__iadd__`, Python falls back to `__add__` and does exactly that.
    fn __add__(&self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    fn __sub__(&self, other: Self) -> Self {
        Self(self.0 - other.0)
    }

    fn __mul__(&self, other: Self) -> Self {
        Self(self.0 * other.0)
    }

    fn __truediv__(&self, other: Self) -> Self {
        Self(self.0 / other.0)
    }

    #[pyo3(name = "equalWithAbsError")]
    fn equal_with_abs_error(&self, v2: Self, e: f64) -> bool {
        self.0.equal_with_abs_error(v2.0, e)
    }

    #[pyo3(name = "equalWithRelError")]
    fn equal_with_rel_error(&self, v2: Self, e: f64) -> bool {
        self.0.equal_with_rel_error(v2.0, e)
    }

    fn dot(&self, v2: Self) -> f64 {
        self.0.dot(v2.0)
    }

    fn cross(&self, v2: Self) -> f64 {
        self.0.cross(v2.0)
    }

    fn length(&self) -> f64 {
        self.0.length()
    }

    fn length2(&self) -> f64 {
        self.0.length2()
    }

    /// Scales this vector to length one in place, and returns a copy.
    fn normalize(&mut self) -> Self {
        self.0 = self.0.normalized();
        *self
    }

    /// As `normalize`, but a null vector is a `ValueError`.
    #[pyo3(name = "normalizeExc")]
    fn normalize_exc(&mut self) -> PyResult<Self> {
        self.0 = Self::checked(self.0)?;
        Ok(*self)
    }

    /// As `normalize`, with no check for a null vector at all.
    #[pyo3(name = "normalizeNonNull")]
    fn normalize_non_null(&mut self) -> Self {
        self.0 = self.0.normalized_unchecked();
        *self
    }

    fn normalized(&self) -> Self {
        Self(self.0.normalized())
    }

    #[pyo3(name = "normalizedExc")]
    fn normalized_exc(&self) -> PyResult<Self> {
        Self::checked(self.0).map(Self)
    }

    #[pyo3(name = "normalizedNonNull")]
    fn normalized_non_null(&self) -> Self {
        Self(self.0.normalized_unchecked())
    }

    #[staticmethod]
    #[pyo3(name = "baseTypeLowest")]
    const fn base_type_lowest() -> f64 {
        f64::MIN
    }

    #[staticmethod]
    #[pyo3(name = "baseTypeMax")]
    const fn base_type_max() -> f64 {
        f64::MAX
    }

    #[staticmethod]
    #[pyo3(name = "baseTypeSmallest")]
    const fn base_type_smallest() -> f64 {
        f64::MIN_POSITIVE
    }

    #[staticmethod]
    #[pyo3(name = "baseTypeEpsilon")]
    const fn base_type_epsilon() -> f64 {
        f64::EPSILON
    }

    #[staticmethod]
    const fn dimensions() -> u32 {
        2
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

impl PyV2d {
    /// Normalizes, raising what pybind11 raises for Imath's
    /// `std::domain_error`.
    fn checked(v: V2d) -> PyResult<V2d> {
        v.normalized_checked()
            .ok_or_else(|| PyValueError::new_err("Cannot normalize null vector."))
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
///
/// Imath's `Box2d`, as upstream binds it: mutable, with `extendBy` growing
/// the box in place.
#[pyclass(name = "Box2d", module = "opentimelineio._otio", from_py_object)]
#[derive(Clone, Copy)]
pub struct PyBox2d(pub Box2d);

#[pymethods]
impl PyBox2d {
    /// `Box2d()` for a box at the origin, `Box2d(point)` for a box holding
    /// just that point, or `Box2d(min, max)`.
    #[new]
    #[pyo3(signature = (min = None, max = None))]
    fn new(min: Option<PyV2d>, max: Option<PyV2d>) -> PyResult<Self> {
        match (min, max) {
            (None, None) => Ok(Self(Box2d::new(V2d::default(), V2d::default()))),
            (Some(point), None) => Ok(Self(Box2d::new(point.0, point.0))),
            (Some(min), Some(max)) => Ok(Self(Box2d::new(min.0, max.0))),
            (None, Some(_)) => Err(PyTypeError::new_err(
                "Box2d() takes no arguments, one V2d, or two",
            )),
        }
    }

    #[getter]
    const fn min(&self) -> PyV2d {
        PyV2d(self.0.min)
    }

    #[setter]
    const fn set_min(&mut self, min: PyV2d) {
        self.0.min = min.0;
    }

    #[getter]
    const fn max(&self) -> PyV2d {
        PyV2d(self.0.max)
    }

    #[setter]
    const fn set_max(&mut self, max: PyV2d) {
        self.0.max = max.0;
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 == type_checked::<Self>(other, "Box2d", "==")?.0)
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 != type_checked::<Self>(other, "Box2d", "!=")?.0)
    }

    fn center(&self) -> PyV2d {
        PyV2d(self.0.center())
    }

    /// Grows the box in place to hold a point or another box.
    #[pyo3(name = "extendBy")]
    fn extend_by(&mut self, other: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(point) = other.extract::<PyV2d>() {
            self.0 = self.0.extended_by_point(point.0);
        } else {
            self.0 = self.0.extended_by(Self::box_or_point(other, "extendBy")?);
        }
        Ok(())
    }

    /// Whether a point or another box meets this one, edges included.
    fn intersects(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        if let Ok(point) = other.extract::<PyV2d>() {
            return Ok(self.0.contains_point(point.0));
        }
        Ok(self.0.intersects(Self::box_or_point(other, "intersects")?))
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

impl PyBox2d {
    /// Reads the box argument of one of the two-overload methods.
    fn box_or_point(other: &Bound<'_, PyAny>, method: &str) -> PyResult<Box2d> {
        other.extract::<Self>().map(|b| b.0).map_err(|_| {
            PyTypeError::new_err(format!(
                "{method}(): incompatible function arguments; expected a V2d or a Box2d"
            ))
        })
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

    // Upstream's order, name first, with each field as Python's `repr()`
    // renders it.
    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "otio.core.Color(name={}, r={}, g={}, b={}, a={})",
            self.0.name.as_str().into_pyobject(py)?.repr()?,
            float_repr(py, self.0.r)?,
            float_repr(py, self.0.g)?,
            float_repr(py, self.0.b)?,
            float_repr(py, self.0.a)?
        ))
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
/// This is upstream's `_value_to_any`, written here rather than in Python:
/// any mapping becomes a dictionary and any sequence but a string a vector,
/// so an `AnyDictionary`, an `AnyVector` or an item's markers are copied in
/// as readily as a `dict` or a `list`, and a container that holds itself is
/// refused rather than followed for ever.
///
/// The order the cases are tried in matters: `bool` is a subclass of `int` in
/// Python, so it has to be checked first or `True` becomes `1`.
pub fn python_to_any(home: &Shared, value: &Bound<'_, PyAny>) -> PyResult<Any> {
    convert(home, value, &mut Vec::new())
}

/// The value types upstream's error messages list, in its words.
const SUPPORTED_VALUE_TYPES: &str = "('int', 'float', 'str', 'bool', 'list', 'dictionary', \
     'opentime.RationalTime', 'opentime.TimeRange', 'opentime.TimeTransform', \
     'opentimelineio.core.Color', 'opentimelineio.core.SerializableObject')";

/// [`python_to_any`], given the containers being converted on the way down
/// to `value`, by identity.
fn convert(home: &Shared, value: &Bound<'_, PyAny>, within: &mut Vec<usize>) -> PyResult<Any> {
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
        // than writing a number that cannot be read back.
        return number.extract::<i64>().map(Any::Int).map_err(|_| {
            PyValueError::new_err(format!(
                "A value of {number} is outside of the range of integers that \
                 OpenTimelineIO supports, [{}, {}], which is the range of C++ int64_t.",
                i64::MIN,
                i64::MAX
            ))
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

    let py = value.py();
    let abc = py.import("collections.abc")?;
    let mapping = value.cast::<PyDict>().is_ok() || value.is_instance(&abc.getattr("Mapping")?)?;
    let sequence = !mapping
        && (value.cast::<PyList>().is_ok()
            || value.cast::<PyTuple>().is_ok()
            || value.is_instance(&abc.getattr("Sequence")?)?);
    if mapping {
        let mut entries = AnyDictionary::new();
        for pair in value.call_method0("items")?.try_iter()? {
            let (key, item): (Bound<'_, PyAny>, Bound<'_, PyAny>) = pair?.extract()?;
            let Ok(key) = key.cast::<PyString>() else {
                return Err(PyValueError::new_err(format!(
                    "key '{key}' is not a string"
                )));
            };
            entries.insert(key.to_string(), convert_within(home, &item, within)?);
        }
        return Ok(Any::Dictionary(entries));
    }
    if sequence {
        let mut items = Vec::new();
        for item in value.try_iter()? {
            items.push(convert_within(home, &item?, within)?);
        }
        return Ok(Any::Vector(items));
    }
    Err(PyTypeError::new_err(format!(
        "A value of type '{}' is incompatible with OpenTimelineIO. OpenTimelineIO only \
         supports the following value types in AnyDictionary containers (like the \
         .metadata dictionary): {SUPPORTED_VALUE_TYPES}.",
        value.get_type().str()?
    )))
}

/// Converts one entry of a container, refusing it if it is a container
/// already being converted further up, as upstream does.
fn convert_within(
    home: &Shared,
    item: &Bound<'_, PyAny>,
    within: &mut Vec<usize>,
) -> PyResult<Any> {
    let identity = item.as_ptr() as usize;
    if within.contains(&identity) {
        return Err(PyValueError::new_err(
            "circular reference converting dictionary to C++ datatype",
        ));
    }
    within.push(identity);
    let converted = convert(home, item, within);
    within.pop();
    converted
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
    if let Some(home) = crate::containers::home_of(value) {
        return Some(home);
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

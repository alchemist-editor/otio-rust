//! The `opentimelineio._opentime` half of the bindings.
//!
//! These classes are a
//! deliberate imitation: every class, method, default argument and error type
//! here matches upstream OpenTimelineIO's pybind11 bindings in
//! `src/py-opentimelineio/opentime-bindings`, so that upstream's own
//! `tests/test_opentime.py` runs against it unchanged. Where this file looks
//! odd, upstream is usually the reason, and the comment says so.
//!
//! The pure-Python half of the package — `opentimelineio/__init__.py` and
//! `opentimelineio/opentime.py` — lives beside this crate in `python/` and is
//! also carried over from upstream.

use opentime::cfmt::format_g;
use opentime::{DropFrame, RationalTime, TimeRange, TimeTransform};

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::{IntoPyObjectExt, Py, PyAny};

/// Renders a float the way upstream's `%g` does.
fn g(value: f64) -> String {
    format_g(value, 6)
}

/// Turns an `opentime` failure into the `ValueError` upstream raises.
///
/// Upstream's binding layer converts every `ErrorStatus` into
/// `py::value_error(error_status.details)`, so a bad timecode and a bad rate
/// both arrive in Python as `ValueError`, distinguishable only by message.
fn value_error<T>(result: opentime::Result<T>) -> PyResult<T> {
    result.map_err(|error| PyValueError::new_err(error.to_string()))
}

/// Reads a `RationalTime` from an argument, or raises the `TypeError`
/// upstream raises.
///
/// Upstream type-checks the right-hand side of every comparison and arithmetic
/// operator by hand rather than returning `NotImplemented`, so `t < -1` is a
/// `TypeError` and not a silent `False`. Its own tests pin that.
fn time_arg(value: &Bound<'_, PyAny>, op: &str) -> PyResult<RationalTime> {
    value
        .extract::<PyRationalTime>()
        .map(|time| time.0)
        .map_err(|_| {
            let name = value
                .get_type()
                .name()
                .map_or_else(|_| "object".to_string(), |name| name.to_string());
            PyTypeError::new_err(format!(
                "unsupported operand type(s) for {op}: RationalTime and {name}"
            ))
        })
}

/// Maps Python's tri-state `drop_frame` argument onto [`DropFrame`].
///
/// `None` means "infer from the rate", which is why the argument is an
/// `Option<bool>` and not a `bool`.
const fn drop_frame(value: Option<bool>) -> DropFrame {
    match value {
        None => DropFrame::InferFromRate,
        Some(true) => DropFrame::ForceYes,
        Some(false) => DropFrame::ForceNo,
    }
}

/// The RationalTime class represents a measure of time of :math:`rt.value/rt.rate` seconds.
/// It can be rescaled into another :class:`~RationalTime`'s rate.
#[pyclass(
    name = "RationalTime",
    module = "opentimelineio._opentime",
    frozen,
    from_py_object
)]
#[derive(Clone, Copy)]
pub struct PyRationalTime(pub RationalTime);

// PyO3 takes the receiver by reference whatever the type is, so the `to_*`
// methods here cannot follow Rust's convention of taking a `Copy` self by
// value. Their names are upstream's and are what Python calls.
#[allow(clippy::wrong_self_convention)]
#[pymethods]
impl PyRationalTime {
    #[new]
    #[pyo3(signature = (value = 0.0, rate = 1.0))]
    const fn new(value: f64, rate: f64) -> Self {
        Self(RationalTime::new(value, rate))
    }

    #[getter]
    const fn value(&self) -> f64 {
        self.0.value()
    }

    #[getter]
    const fn rate(&self) -> f64 {
        self.0.rate()
    }

    fn is_invalid_time(&self) -> bool {
        self.0.is_invalid_time()
    }

    fn is_valid_time(&self) -> bool {
        self.0.is_valid_time()
    }

    /// Restates this time at another rate.
    ///
    /// Upstream overloads this on `double` and on `RationalTime`; PyO3 has no
    /// overloading, so the argument is inspected here instead.
    fn rescaled_to(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(time) = other.extract::<Self>() {
            return Ok(Self(self.0.rescaled_to_time(time.0)));
        }
        Ok(Self(self.0.rescaled_to(other.extract::<f64>()?)))
    }

    fn value_rescaled_to(&self, other: &Bound<'_, PyAny>) -> PyResult<f64> {
        if let Ok(time) = other.extract::<Self>() {
            return Ok(self.0.value_rescaled_to_time(time.0));
        }
        Ok(self.0.value_rescaled_to(other.extract::<f64>()?))
    }

    #[pyo3(signature = (other, delta = 0.0))]
    fn almost_equal(&self, other: &Self, delta: f64) -> bool {
        self.0.almost_equal(other.0, delta)
    }

    fn strictly_equal(&self, other: &Self) -> bool {
        self.0.strictly_equal(other.0)
    }

    fn floor(&self) -> Self {
        Self(self.0.floor())
    }

    fn ceil(&self) -> Self {
        Self(self.0.ceil())
    }

    fn round(&self) -> Self {
        Self(self.0.round())
    }

    // A time is a value, so copying one is returning it. Upstream says the
    // same thing the same way.
    const fn __copy__(&self) -> Self {
        *self
    }

    #[pyo3(signature = (copier = None))]
    const fn __deepcopy__(&self, copier: Option<&Bound<'_, PyAny>>) -> Self {
        let _ = copier;
        *self
    }

    #[staticmethod]
    fn duration_from_start_end_time(start_time: &Self, end_time_exclusive: &Self) -> Self {
        Self(RationalTime::duration_from_start_end_time(
            start_time.0,
            end_time_exclusive.0,
        ))
    }

    #[staticmethod]
    fn duration_from_start_end_time_inclusive(
        start_time: &Self,
        end_time_inclusive: &Self,
    ) -> Self {
        Self(RationalTime::duration_from_start_end_time_inclusive(
            start_time.0,
            end_time_inclusive.0,
        ))
    }

    /// Deprecated upstream in favour of `is_smpte_timecode_rate`.
    #[staticmethod]
    fn is_valid_timecode_rate(rate: f64) -> bool {
        RationalTime::is_smpte_timecode_rate(rate)
    }

    #[staticmethod]
    fn is_smpte_timecode_rate(rate: f64) -> bool {
        RationalTime::is_smpte_timecode_rate(rate)
    }

    /// Deprecated upstream in favour of `nearest_smpte_timecode_rate`.
    #[staticmethod]
    fn nearest_valid_timecode_rate(rate: f64) -> f64 {
        RationalTime::nearest_smpte_timecode_rate(rate)
    }

    #[staticmethod]
    fn nearest_smpte_timecode_rate(rate: f64) -> f64 {
        RationalTime::nearest_smpte_timecode_rate(rate)
    }

    #[staticmethod]
    fn from_frames(frame: f64, rate: f64) -> Self {
        Self(RationalTime::from_frames(frame, rate))
    }

    #[staticmethod]
    #[pyo3(signature = (seconds, rate = None))]
    fn from_seconds(seconds: f64, rate: Option<f64>) -> Self {
        Self(rate.map_or_else(
            || RationalTime::from_seconds(seconds),
            |rate| RationalTime::from_seconds_at_rate(seconds, rate),
        ))
    }

    #[staticmethod]
    fn from_timecode(timecode: &str, rate: f64) -> PyResult<Self> {
        value_error(RationalTime::from_timecode(timecode, rate)).map(Self)
    }

    #[staticmethod]
    fn from_time_string(time_string: &str, rate: f64) -> PyResult<Self> {
        value_error(RationalTime::from_time_string(time_string, rate)).map(Self)
    }

    #[pyo3(signature = (rate = None))]
    fn to_frames(&self, rate: Option<f64>) -> i32 {
        rate.map_or_else(|| self.0.to_frames(), |rate| self.0.to_frames_at_rate(rate))
    }

    fn to_seconds(&self) -> f64 {
        self.0.to_seconds()
    }

    #[pyo3(signature = (rate = None, drop_frame = None))]
    fn to_timecode(&self, rate: Option<f64>, drop_frame: Option<bool>) -> PyResult<String> {
        let rate = rate.unwrap_or_else(|| self.0.rate());
        value_error(self.0.to_timecode_at(rate, self::drop_frame(drop_frame)))
    }

    #[pyo3(signature = (rate = None, drop_frame = None))]
    fn to_nearest_timecode(&self, rate: Option<f64>, drop_frame: Option<bool>) -> PyResult<String> {
        let rate = rate.unwrap_or_else(|| self.0.rate());
        value_error(
            self.0
                .to_nearest_timecode_at(rate, self::drop_frame(drop_frame)),
        )
    }

    fn to_time_string(&self) -> String {
        self.0.to_time_string()
    }

    fn __str__(&self) -> String {
        format!("RationalTime({}, {})", g(self.0.value()), g(self.0.rate()))
    }

    fn __repr__(&self) -> String {
        format!(
            "otio.opentime.RationalTime(value={}, rate={})",
            g(self.0.value()),
            g(self.0.rate())
        )
    }

    fn __neg__(&self) -> Self {
        Self(-self.0)
    }

    fn __lt__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 < time_arg(other, "<")?)
    }

    fn __gt__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 > time_arg(other, ">")?)
    }

    fn __le__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 <= time_arg(other, "<=")?)
    }

    fn __ge__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 >= time_arg(other, ">=")?)
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 == time_arg(other, "==")?)
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(self.0 != time_arg(other, "!=")?)
    }

    fn __add__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(self.0 + time_arg(other, "+")?))
    }

    fn __sub__(&self, other: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(Self(self.0 - time_arg(other, "-")?))
    }

    // There is deliberately no `__iadd__`. A time has value semantics, and
    // upstream carries a comment saying the same thing: mutating in place
    // would reach every other name bound to the same object. Leaving the slot
    // undefined makes Python fall back to `__add__` and rebind, which is the
    // behaviour upstream goes out of its way to get.
}

/// The TimeRange class represents a range in time. It encodes the start time and the duration,
/// meaning that :meth:`end_time_inclusive` (last portion of a sample in the time range) and
/// :meth:`end_time_exclusive` can be computed.
#[pyclass(
    name = "TimeRange",
    module = "opentimelineio._opentime",
    frozen,
    from_py_object
)]
#[derive(Clone, Copy)]
pub struct PyTimeRange(pub TimeRange);

#[pymethods]
impl PyTimeRange {
    /// Builds a range from two times, or from a start, a duration and a rate.
    ///
    /// Upstream offers both forms, so `TimeRange(RationalTime(0, 24),
    /// RationalTime(48, 24))` and `TimeRange(0, 48, 24)` both work and mean
    /// the same thing. The second form is the reason `start_time` is inspected
    /// rather than typed.
    #[new]
    #[pyo3(signature = (start_time = None, duration = None, rate = None))]
    fn new(
        start_time: Option<&Bound<'_, PyAny>>,
        duration: Option<&Bound<'_, PyAny>>,
        rate: Option<f64>,
    ) -> PyResult<Self> {
        if let Some(rate) = rate {
            let start = start_time.map_or(Ok(0.0), |value| value.extract::<f64>())?;
            let length = duration.map_or(Ok(0.0), |value| value.extract::<f64>())?;
            return Ok(Self(TimeRange::from_values(start, length, rate)));
        }

        let start = start_time
            .map(|value| value.extract::<PyRationalTime>())
            .transpose()?;
        let length = duration
            .map(|value| value.extract::<PyRationalTime>())
            .transpose()?;

        // Where only one of the two is given, the other is zero *at that
        // one's rate*, so `TimeRange(RationalTime(5, 24))` is a range in 24
        // and not a start in 24 beside a duration in 1.
        let rate = start
            .or(length)
            .map_or(1.0, |time| RationalTime::rate(time.0));
        Ok(Self(TimeRange::new(
            start.map_or_else(|| RationalTime::new(0.0, rate), |time| time.0),
            length.map_or_else(|| RationalTime::new(0.0, rate), |time| time.0),
        )))
    }

    #[getter]
    const fn start_time(&self) -> PyRationalTime {
        PyRationalTime(self.0.start_time())
    }

    #[getter]
    const fn duration(&self) -> PyRationalTime {
        PyRationalTime(self.0.duration())
    }

    fn is_invalid_range(&self) -> bool {
        self.0.is_invalid_range()
    }

    fn is_valid_range(&self) -> bool {
        self.0.is_valid_range()
    }

    fn end_time_inclusive(&self) -> PyRationalTime {
        PyRationalTime(self.0.end_time_inclusive())
    }

    fn end_time_exclusive(&self) -> PyRationalTime {
        PyRationalTime(self.0.end_time_exclusive())
    }

    fn duration_extended_by(&self, other: &PyRationalTime) -> Self {
        Self(self.0.duration_extended_by(other.0))
    }

    fn extended_by(&self, other: &Self) -> Self {
        Self(self.0.extended_by(other.0))
    }

    /// Clamps a time or a range into this one.
    ///
    /// Upstream overloads on the argument type; PyO3 inspects it instead.
    fn clamped(&self, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = other.py();
        if let Ok(time) = other.extract::<PyRationalTime>() {
            return PyRationalTime(self.0.clamped_time(time.0)).into_py_any(py);
        }
        let range = other.extract::<Self>()?;
        Self(self.0.clamped_range(range.0)).into_py_any(py)
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn contains(&self, other: &Bound<'_, PyAny>, epsilon_s: f64) -> PyResult<bool> {
        if let Ok(time) = other.extract::<PyRationalTime>() {
            return Ok(self.0.contains_time(time.0));
        }
        Ok(self.0.contains_range(other.extract::<Self>()?.0, epsilon_s))
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn overlaps(&self, other: &Bound<'_, PyAny>, epsilon_s: f64) -> PyResult<bool> {
        if let Ok(time) = other.extract::<PyRationalTime>() {
            return Ok(self.0.overlaps_time(time.0));
        }
        Ok(self.0.overlaps_range(other.extract::<Self>()?.0, epsilon_s))
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn before(&self, other: &Bound<'_, PyAny>, epsilon_s: f64) -> PyResult<bool> {
        if let Ok(time) = other.extract::<PyRationalTime>() {
            return Ok(self.0.before_time(time.0, epsilon_s));
        }
        Ok(self.0.before_range(other.extract::<Self>()?.0, epsilon_s))
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn meets(&self, other: &Self, epsilon_s: f64) -> bool {
        self.0.meets(other.0, epsilon_s)
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn begins(&self, other: &Bound<'_, PyAny>, epsilon_s: f64) -> PyResult<bool> {
        if let Ok(time) = other.extract::<PyRationalTime>() {
            return Ok(self.0.begins_time(time.0, epsilon_s));
        }
        Ok(self.0.begins_range(other.extract::<Self>()?.0, epsilon_s))
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn finishes(&self, other: &Bound<'_, PyAny>, epsilon_s: f64) -> PyResult<bool> {
        if let Ok(time) = other.extract::<PyRationalTime>() {
            return Ok(self.0.finishes_time(time.0, epsilon_s));
        }
        Ok(self.0.finishes_range(other.extract::<Self>()?.0, epsilon_s))
    }

    #[pyo3(signature = (other, epsilon_s = opentime::DEFAULT_EPSILON_S))]
    fn intersects(&self, other: &Self, epsilon_s: f64) -> bool {
        self.0.intersects(other.0, epsilon_s)
    }

    #[staticmethod]
    fn range_from_start_end_time(
        start_time: &PyRationalTime,
        end_time_exclusive: &PyRationalTime,
    ) -> Self {
        Self(TimeRange::range_from_start_end_time(
            start_time.0,
            end_time_exclusive.0,
        ))
    }

    #[staticmethod]
    fn range_from_start_end_time_inclusive(
        start_time: &PyRationalTime,
        end_time_inclusive: &PyRationalTime,
    ) -> Self {
        Self(TimeRange::range_from_start_end_time_inclusive(
            start_time.0,
            end_time_inclusive.0,
        ))
    }

    const fn __copy__(&self) -> Self {
        *self
    }

    #[pyo3(signature = (copier = None))]
    const fn __deepcopy__(&self, copier: Option<&Bound<'_, PyAny>>) -> Self {
        let _ = copier;
        *self
    }

    fn __str__(&self) -> String {
        format!(
            "TimeRange({}, {})",
            PyRationalTime(self.0.start_time()).__str__(),
            PyRationalTime(self.0.duration()).__str__()
        )
    }

    fn __repr__(&self) -> String {
        format!(
            "otio.opentime.TimeRange(start_time={}, duration={})",
            PyRationalTime(self.0.start_time()).__repr__(),
            PyRationalTime(self.0.duration()).__repr__()
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other.extract::<Self>().is_ok_and(|range| self.0 == range.0)
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
        !self.__eq__(other)
    }
}

/// 1D transform for :class:`~RationalTime`. Has offset and scale.
#[pyclass(
    name = "TimeTransform",
    module = "opentimelineio._opentime",
    frozen,
    from_py_object
)]
#[derive(Clone, Copy)]
pub struct PyTimeTransform(pub TimeTransform);

#[pymethods]
impl PyTimeTransform {
    #[new]
    #[pyo3(signature = (offset = None, scale = 1.0, rate = -1.0))]
    fn new(offset: Option<&PyRationalTime>, scale: f64, rate: f64) -> Self {
        Self(TimeTransform::new(
            offset.map_or_else(|| RationalTime::new(0.0, 1.0), |time| time.0),
            scale,
            rate,
        ))
    }

    #[getter]
    const fn offset(&self) -> PyRationalTime {
        PyRationalTime(self.0.offset())
    }

    #[getter]
    const fn scale(&self) -> f64 {
        self.0.scale()
    }

    #[getter]
    const fn rate(&self) -> f64 {
        self.0.rate()
    }

    /// Applies this transform to a time, a range or another transform.
    fn applied_to(&self, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = other.py();
        if let Ok(range) = other.extract::<PyTimeRange>() {
            return PyTimeRange(self.0.applied_to_range(range.0)).into_py_any(py);
        }
        if let Ok(transform) = other.extract::<Self>() {
            return Self(self.0.applied_to_transform(transform.0)).into_py_any(py);
        }
        PyRationalTime(self.0.applied_to_time(other.extract::<PyRationalTime>()?.0)).into_py_any(py)
    }

    const fn __copy__(&self) -> Self {
        *self
    }

    #[pyo3(signature = (copier = None))]
    const fn __deepcopy__(&self, copier: Option<&Bound<'_, PyAny>>) -> Self {
        let _ = copier;
        *self
    }

    fn __str__(&self) -> String {
        format!(
            "TimeTransform({}, {}, {})",
            PyRationalTime(self.0.offset()).__str__(),
            g(self.0.scale()),
            g(self.0.rate())
        )
    }

    fn __repr__(&self) -> String {
        format!(
            "otio.opentime.TimeTransform(offset={}, scale={}, rate={})",
            PyRationalTime(self.0.offset()).__repr__(),
            g(self.0.scale()),
            g(self.0.rate())
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<Self>()
            .is_ok_and(|transform| self.0 == transform.0)
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
        !self.__eq__(other)
    }
}

/// Adds `step_time` to itself `final_frame_number` times.
///
/// Upstream keeps this in a `_testing` submodule for one regression test: it
/// accumulates in C++ rather than round-tripping through Python each step, so
/// it catches drift that a Python loop would hide.
#[pyfunction]
fn add_many(step_time: &PyRationalTime, final_frame_number: i32) -> PyRationalTime {
    let mut sum = step_time.0;
    for _ in 1..final_frame_number {
        sum += step_time.0;
    }
    PyRationalTime(sum)
}

/// Registers the time classes on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyRationalTime>()?;
    module.add_class::<PyTimeRange>()?;
    module.add_class::<PyTimeTransform>()?;

    let testing = PyModule::new(module.py(), "_testing")?;
    testing.add_function(wrap_pyfunction!(add_many, &testing)?)?;
    module.add_submodule(&testing)?;
    module.add("_testing", testing)?;
    Ok(())
}

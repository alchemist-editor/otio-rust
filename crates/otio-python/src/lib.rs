//! Python bindings for the otio-rust core.
//!
//! This builds one extension module, `opentimelineio._otio`, and the Python
//! package around it lives in `python/`. Upstream splits its bindings into
//! two extension modules, `_opentime` and `_otio`, because its C++ library is
//! two libraries; there is no such split here, so `opentimelineio/_opentime.py`
//! is a one-line module that re-exports the time classes from this one. Each
//! class still reports the `__module__` upstream gives it, because that shows
//! up in `repr()` and in pickles.
//!
//! The bindings are a deliberate imitation: every class, method, default
//! argument and error type matches upstream's pybind11 bindings, so that
//! upstream's own tests run against them unchanged. Where a binding looks
//! odd, upstream is usually the reason, and the comment says so.

mod adapters;
mod arena;
mod errors;
mod objects;
mod opentime;
mod values;

use pyo3::prelude::*;
use pyo3::{Py, PyAny};

/// Reads an object back from OTIO JSON.
#[pyfunction]
fn deserialize_json_from_string(py: Python<'_>, input: &str) -> PyResult<Py<PyAny>> {
    objects::read_from_string(py, input)
}

/// Writes an object out as OTIO JSON.
///
/// Upstream takes any object, not only a timeline, and its own tests
/// round-trip a bare clip, so this starts wherever it is pointed.
#[pyfunction]
#[pyo3(signature = (input, indent = otio_core::DEFAULT_INDENT))]
fn serialize_json_to_string(input: &Bound<'_, PyAny>, indent: usize) -> PyResult<String> {
    objects::write_to_string(input, indent)
}

/// The `opentimelineio._otio` extension module.
#[pymodule]
fn _otio(module: &Bound<'_, PyModule>) -> PyResult<()> {
    errors::register(module)?;
    opentime::register(module)?;
    values::register(module)?;
    objects::register(module)?;
    adapters::register(module)?;
    module.add_function(wrap_pyfunction!(deserialize_json_from_string, module)?)?;
    module.add_function(wrap_pyfunction!(serialize_json_to_string, module)?)?;
    Ok(())
}

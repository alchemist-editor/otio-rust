//! `opentimelineio._otio.bundle`: upstream's bundle helpers.
//!
//! Upstream binds its C++ `bundle` namespace as this submodule. Only
//! `file_from_url` is here so far, which `opentimelineio.url_utils` calls.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

/// Convert a file:// URL to a filesystem path.
///
/// Handles Windows drive letters in netloc position (file://C:/...), UNC
/// paths (file://host/share/...), percent-encoded characters, and the
/// 'localhost' authority. Returns the input unchanged if it is a bare path
/// with no URL scheme. Returns None if the URL uses a non-file scheme (e.g.
/// http://...).
#[pyfunction]
#[pyo3(signature = (url))]
fn file_from_url(py: Python<'_>, url: &str) -> PyResult<Option<Py<PyAny>>> {
    let Some(path) = otio_core::bundle::file_from_url(url)
        .map_err(|error| PyValueError::new_err(error.to_string()))?
    else {
        return Ok(None);
    };
    // A percent escape can spell bytes that are not UTF-8. pybind11 decodes
    // the C++ string with Python's own UTF-8 codec, so decoding the same way
    // raises the same `UnicodeDecodeError` upstream does.
    Ok(Some(
        PyBytes::new(py, &path)
            .call_method1("decode", ("utf-8",))?
            .unbind(),
    ))
}

/// Adds the `bundle` submodule to `_otio`.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let bundle = PyModule::new(module.py(), "bundle")?;
    bundle.add_function(wrap_pyfunction!(file_from_url, &bundle)?)?;
    module.add_submodule(&bundle)
}

//! Turning on otio-rust's extensions from Python: work built in this
//! repository that is not part of OpenTimelineIO (ADR 0010).
//!
//! These are private functions of `_otio`. The public face is the
//! `opentimelineio.ext` package, which nothing in `opentimelineio` imports, so
//! a program that never asks for an extension never sees one.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

/// Installs every extension these bindings are built with.
fn install() {
    otio_ext_sync::install();
}

/// Turns on the extension `id`, and every extension it depends on first.
#[pyfunction]
fn _ext_enable(id: &str) -> PyResult<()> {
    install();
    otio_ext::enable(id).map_err(|error| match error {
        otio_ext::Error::Register { .. } => PyRuntimeError::new_err(error.to_string()),
        _ => PyValueError::new_err(error.to_string()),
    })
}

/// Whether the extension `id` is on.
#[pyfunction]
fn _ext_is_enabled(id: &str) -> bool {
    otio_ext::is_enabled(id)
}

/// The ids of the extensions that are on, in the order they were enabled.
#[pyfunction]
fn _ext_enabled() -> Vec<&'static str> {
    otio_ext::enabled()
}

/// The ids of every extension these bindings are built with.
#[pyfunction]
fn _ext_available() -> Vec<&'static str> {
    install();
    otio_ext::available()
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(_ext_enable, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_is_enabled, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_enabled, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_available, module)?)?;
    Ok(())
}

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

// pyo3's `#[pyclass(from_py_object)]` expands to a `.clone()` of the class,
// which clippy 1.99 flags on every class that is also `Copy`. The call is in
// the macro's code, not this crate's, so there is nothing here to change.
#![allow(clippy::clone_on_copy)]

mod adapters;
mod algorithms;
mod arena;
mod bundle;
mod containers;
mod edit;
mod enums;
mod errors;
mod objects;
mod opentime;
mod plugins;
mod registry;
mod testing;
mod testing_hooks;
mod values;
mod vectors;

use pyo3::prelude::*;

/// The `opentimelineio._otio` extension module.
#[pymodule]
fn _otio(module: &Bound<'_, PyModule>) -> PyResult<()> {
    errors::register(module)?;
    opentime::register(module)?;
    values::register(module)?;
    objects::register(module)?;
    vectors::register(module)?;
    registry::register(module)?;
    containers::register(module)?;
    testing_hooks::register(module)?;
    adapters::register(module)?;
    algorithms::register(module)?;
    edit::register(module)?;
    bundle::register(module)?;
    plugins::register(module)?;
    testing::register(module)?;
    Ok(())
}

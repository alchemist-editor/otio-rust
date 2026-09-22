//! Upstream's `_testing` submodule: hooks its regression tests call.
//!
//! Upstream's hooks exercise its C++ reference counting from threads that
//! have let go of the GIL. There is no reference counting to exercise here,
//! but the same calls still check something real: that a document can be
//! read from several threads at once while Python runs, and that the object
//! the hook hands back is the one Python sees.

use otio_core::NodeId;

use pyo3::prelude::*;
use pyo3::{Py, PyAny};

use crate::objects::{Handle, handle_of, wrap};

/// Returns the extension module's `_testing` submodule, making it if no
/// other part of the bindings has yet.
///
/// Upstream has one of these on each of its two extension modules; there is
/// one extension module here, so every part adds its hooks to the same one.
pub fn submodule<'py>(module: &Bound<'py, PyModule>) -> PyResult<Bound<'py, PyModule>> {
    if let Ok(existing) = module.getattr("_testing") {
        return Ok(existing.cast_into::<PyModule>()?);
    }
    let testing = PyModule::new(module.py(), "_testing")?;
    module.add_submodule(&testing)?;
    Ok(testing)
}

/// How many times upstream's hooks take hold of the child.
const ROUNDS: usize = 1024 * 10;

/// Reads a collection's first child `ROUNDS` times and counts the reads that
/// found one. Runs without the GIL.
fn bash(handle: &Handle) -> (usize, Option<NodeId>) {
    let mut total = 0;
    let mut first = None;
    for _ in 0..ROUNDS {
        let child = handle
            .with(|node| {
                Ok(node
                    .children()
                    .and_then(|children| children.first().copied()))
            })
            .ok()
            .flatten();
        if child.is_some() {
            total += 1;
            first = child;
        }
    }
    (total, first)
}

/// Upstream's `bash_retainers1`: takes hold of a collection's first child
/// many times over with the GIL released, and returns how often it was
/// there.
#[pyfunction]
fn bash_retainers1(py: Python<'_>, sc: &Bound<'_, PyAny>) -> PyResult<usize> {
    let handle = handle_of(sc)?;
    Ok(py.detach(|| bash(&handle).0))
}

/// Upstream's `bash_retainers2`: as [`bash_retainers1`], twice, calling
/// `materialize_obj` between the two; returns the first child, or `None` if
/// it was never there.
#[pyfunction]
fn bash_retainers2(
    py: Python<'_>,
    sc: &Bound<'_, PyAny>,
    materialize_obj: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let handle = handle_of(sc)?;
    let (before, _) = py.detach(|| bash(&handle));
    materialize_obj.call0()?;
    let (after, first) = py.detach(|| bash(&handle));
    match first {
        Some(child) if before + after > 0 => Ok(wrap(py, &handle.sibling(child)?)?.unbind()),
        _ => Ok(py.None()),
    }
}

/// Upstream's `gil_scoping`: lets go of the GIL and takes it back, nested
/// both ways round.
#[pyfunction]
fn gil_scoping(py: Python<'_>) {
    py.detach(|| ());
    Python::attach(|_| ());
    py.detach(|| Python::attach(|_| ()));
}

/// Upstream's `xyzzy`: a place to put a debugger breakpoint.
#[pyfunction]
fn xyzzy(msg: &str) {
    println!("XYZZY: {msg}");
}

/// Upstream's `takeme`: takes hold of an object and lets go again.
#[pyfunction]
fn takeme(so: &Bound<'_, PyAny>) -> PyResult<()> {
    handle_of(so).map(drop)
}

/// Not upstream's: how many objects the document `value` lives in holds,
/// hidden ones included. The bindings' own tests use it to see an object
/// freed, which a weak reference to its wrapper cannot show.
#[pyfunction]
fn _document_size(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    let shared = handle_of(value)
        .map(|handle| handle.shared)
        .ok()
        .or_else(|| crate::containers::home_of(value))
        .or_else(|| crate::vectors::home_of(value))
        .ok_or_else(|| {
            pyo3::exceptions::PyTypeError::new_err("expected an OTIO object or container")
        })?;
    shared.read(|document| Ok(document.len()))
}

/// Registers these hooks on the extension module's `_testing` submodule.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let testing = submodule(module)?;
    testing.add_function(wrap_pyfunction!(bash_retainers1, &testing)?)?;
    testing.add_function(wrap_pyfunction!(bash_retainers2, &testing)?)?;
    testing.add_function(wrap_pyfunction!(gil_scoping, &testing)?)?;
    testing.add_function(wrap_pyfunction!(xyzzy, &testing)?)?;
    testing.add_function(wrap_pyfunction!(takeme, &testing)?)?;
    testing.add_function(wrap_pyfunction!(_document_size, &testing)?)?;
    Ok(())
}

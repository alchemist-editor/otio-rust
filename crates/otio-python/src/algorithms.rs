//! The algorithms upstream implements in C++: flattening a stack and
//! trimming a track.
//!
//! Upstream's `opentimelineio.algorithms` is mostly Python, and those modules
//! are carried over as Python in `python/opentimelineio/algorithms/`. These
//! two are the exceptions: `flatten_stack` is a C++ function upstream binds
//! directly, and `track_trimmed_to_range` exists in its C++ library as well
//! as in Python. Both run on `otio_core::algorithm` here.

use otio_core::algorithm;
use otio_core::{Error, Node, NodeId};

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::{Py, PyAny};

use crate::arena::Shared;
use crate::objects::{Handle, core_error, handle_of, wrap_root};
use crate::opentime::PyTimeRange;

/// Flattens a stack, or a list of tracks, down to one track.
///
/// Upstream binds two C++ overloads under this one name, one taking a stack
/// (`in_stack`) and one a list of tracks (`tracks`), and picks by the type of
/// the argument. Tracks from different documents are brought into one first,
/// as appending them to a stack would.
#[pyfunction]
#[pyo3(signature = (in_stack = None, tracks = None))]
fn flatten_stack(
    py: Python<'_>,
    in_stack: Option<&Bound<'_, PyAny>>,
    tracks: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let argument = match (in_stack, tracks) {
        (Some(argument), None) | (None, Some(argument)) => argument,
        _ => {
            return Err(PyTypeError::new_err(
                "flatten_stack() takes a Stack or a list of Tracks",
            ));
        }
    };

    if let Ok(handle) = handle_of(argument) {
        if !handle.with(|node| Ok(matches!(node, Node::Stack(_))))? {
            return Err(incompatible());
        }
        let (shared, stack) = handle.live()?;
        let flat =
            shared.write(|document| core_error(algorithm::flatten_stack(document, stack)))?;
        return Ok(wrap_root(py, &Handle { shared, id: flat })?.unbind());
    }

    let mut handles = Vec::new();
    for item in argument.try_iter().map_err(|_| incompatible())? {
        let handle = handle_of(&item?).map_err(|_| incompatible())?;
        if !handle.with(|node| Ok(matches!(node, Node::Track(_))))? {
            return Err(incompatible());
        }
        handles.push(handle);
    }

    let home = match handles.first() {
        Some(first) => first.live()?.0,
        None => Shared::new(),
    };
    let mut ids: Vec<NodeId> = Vec::with_capacity(handles.len());
    for handle in &handles {
        home.absorb(&handle.shared)?;
        ids.push(handle.live()?.1);
    }
    let flat = home.write(|document| core_error(algorithm::flatten_tracks(document, &ids)))?;
    Ok(wrap_root(
        py,
        &Handle {
            shared: home,
            id: flat,
        },
    )?
    .unbind())
}

/// The error pybind11 raises when no overload accepts the arguments.
fn incompatible() -> PyErr {
    PyTypeError::new_err(
        "flatten_stack(): incompatible function arguments. The following argument types are \
         supported:\n    1. (in_stack: opentimelineio._otio.Stack) -> \
         opentimelineio._otio.Track\n    2. (tracks: list[opentimelineio._otio.Track]) -> \
         opentimelineio._otio.Track",
    )
}

/// Cuts a copy of a track down to `trim_range`.
///
/// The Python wrapper in `algorithms/track_algo.py` keeps upstream's
/// signature and docstring; this does the work. Trimming through a
/// transition raises upstream's `CannotTrimTransitionsError`, which is
/// defined in Python, so it is looked up rather than built here.
#[pyfunction]
fn track_trimmed_to_range(
    py: Python<'_>,
    in_track: &Bound<'_, PyAny>,
    trim_range: PyTimeRange,
) -> PyResult<Py<PyAny>> {
    let handle = handle_of(in_track)?;
    if !handle.with(|node| Ok(matches!(node, Node::Track(_))))? {
        return Err(PyTypeError::new_err("in_track must be a Track"));
    }
    let (shared, track) = handle.live()?;
    // The lock is let go before anything else runs: raising the Python
    // exception means importing a module.
    let result = shared.write(|document| {
        Ok(algorithm::track_trimmed_to_range(
            document,
            track,
            trim_range.0,
        ))
    })?;
    let trimmed = match result {
        Err(Error::CannotTrimTransition) => {
            let exceptions = py.import("opentimelineio.exceptions")?;
            return Err(PyErr::from_value(
                exceptions
                    .getattr("CannotTrimTransitionsError")?
                    .call1(("Cannot trim in the middle of a Transition.",))?,
            ));
        }
        result => core_error(result)?,
    };
    Ok(wrap_root(
        py,
        &Handle {
            shared,
            id: trimmed,
        },
    )?
    .unbind())
}

/// Registers the algorithms on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(flatten_stack, module)?)?;
    module.add_function(wrap_pyfunction!(track_trimmed_to_range, module)?)?;
    Ok(())
}

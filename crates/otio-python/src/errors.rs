//! The exception types upstream's `opentimelineio.exceptions` exports.
//!
//! Upstream's own tests catch these by type — `assertRaises(NotAChildError)`
//! rather than `assertRaises(ValueError)` — so a binding that raised
//! `ValueError` for everything would fail them even while doing the right
//! thing. Each is registered here and re-exported by `exceptions.py`, which
//! adds the ones upstream defines in Python.

use pyo3::prelude::*;
use pyo3::{create_exception, exceptions::PyException};

create_exception!(
    _otio,
    OTIOError,
    PyException,
    "The base class every OpenTimelineIO error derives from."
);
create_exception!(
    _otio,
    NotAChildError,
    OTIOError,
    "An object was asked about a composition it does not sit in."
);
create_exception!(
    _otio,
    UnsupportedSchemaError,
    OTIOError,
    "A file carried a schema version this library cannot read."
);
create_exception!(
    _otio,
    CannotComputeAvailableRangeError,
    OTIOError,
    "An item was asked for its media's full span, and nothing below it knows."
);

/// Registers the exception types on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("OTIOError", module.py().get_type::<OTIOError>())?;
    module.add("NotAChildError", module.py().get_type::<NotAChildError>())?;
    module.add(
        "UnsupportedSchemaError",
        module.py().get_type::<UnsupportedSchemaError>(),
    )?;
    module.add(
        "CannotComputeAvailableRangeError",
        module.py().get_type::<CannotComputeAvailableRangeError>(),
    )?;
    Ok(())
}

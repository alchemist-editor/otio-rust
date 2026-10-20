//! Turning on otio-rust's extensions from Python: work built in this
//! repository that is not part of OpenTimelineIO (ADR 0010).
//!
//! These are private functions of `_otio`. The public face is the
//! `opentimelineio.ext` package, which nothing in `opentimelineio` imports, so
//! a program that never asks for an extension never sees one.
//!
//! Timeline sync's calls, `_sync_*`, are `opentimelineio.ext.sync`'s. They
//! do what the C interface's `otio_ext_sync_*` do, on `otio_ext_sync`
//! directly, and take OTIO objects where it takes a document and a handle:
//! the document is the one the object lives in, as for every other function
//! here that takes an object.

use otio_core::NodeId;
use otio_ext_sync::document::{self as sync_document, Transaction};
use otio_ext_sync::{Patch, Snapshot, guid};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::{Py, PyAny};

use crate::arena::Shared;
use crate::objects::{Handle, core_error, handle_of, wrap};

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

/// Refuses every sync call but turning it on until sync is enabled, as the
/// C interface does.
fn sync_enabled() -> PyResult<()> {
    if otio_ext::is_enabled(otio_ext_sync::ID) {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(
            "timeline sync is not enabled: call opentimelineio.ext.sync.enable() first",
        ))
    }
}

/// Turns a sync failure into the exception the C interface's status maps
/// to: text that is not JSON or not patches, and a bad argument, are a
/// `ValueError`; a document that cannot be written or read back is a
/// `RuntimeError`.
fn sync_error(error: &otio_ext_sync::Error) -> PyErr {
    match error {
        otio_ext_sync::Error::Document(_) => PyRuntimeError::new_err(error.to_string()),
        _ => PyValueError::new_err(error.to_string()),
    }
}

fn snapshot_from(json: &str) -> PyResult<Snapshot> {
    Snapshot::from_json(json).map_err(|error| sync_error(&error))
}

fn patches_from(json: &str) -> PyResult<Vec<Patch>> {
    otio_ext_sync::patch::from_json(json).map_err(|error| sync_error(&error))
}

/// The document `object` lives in, and its node there.
fn located(object: &Bound<'_, PyAny>) -> PyResult<(Shared, NodeId)> {
    handle_of(object)?.live()
}

/// A new GUID, a version 4 UUID.
#[pyfunction]
fn _sync_new_guid() -> PyResult<String> {
    sync_enabled()?;
    Ok(guid::new_guid())
}

/// The GUID `node` carries in `metadata["sync"]["guid"]`, or `None`.
#[pyfunction]
fn _sync_guid(node: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    sync_enabled()?;
    let (shared, id) = located(node)?;
    shared.read(|document| {
        core_error(document.try_get(id))?;
        Ok(guid::guid(document, id).map(str::to_owned))
    })
}

/// Gives `node` the GUID `value`, replacing any it had.
#[pyfunction]
fn _sync_set_guid(node: &Bound<'_, PyAny>, value: &str) -> PyResult<()> {
    sync_enabled()?;
    let (shared, id) = located(node)?;
    shared.write(|document| core_error(guid::set_guid(document, id, value)))
}

/// Gives every object sync addresses under `timeline` a GUID it lacks, and
/// returns how many it gave one to.
#[pyfunction]
fn _sync_stamp(timeline: &Bound<'_, PyAny>) -> PyResult<usize> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| Ok(core_error(guid::stamp(document, id))?.len()))
}

/// The object under `timeline` carrying the GUID `value`, or `None`.
#[pyfunction]
fn _sync_find(
    py: Python<'_>,
    timeline: &Bound<'_, PyAny>,
    value: &str,
) -> PyResult<Option<Py<PyAny>>> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    let found = shared.read(|document| {
        core_error(document.try_get(id))?;
        let index = guid::GuidIndex::build(document, id)
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        Ok(index.get(value))
    })?;
    found
        .map(|id| {
            Ok(wrap(
                py,
                &Handle {
                    shared: shared.clone(),
                    id,
                },
            )?
            .unbind())
        })
        .transpose()
}

/// The patches that take one snapshot to another, as a JSON list.
#[pyfunction]
fn _sync_diff(old_snapshot: &str, new_snapshot: &str, pretty: bool) -> PyResult<String> {
    sync_enabled()?;
    let old = snapshot_from(old_snapshot)?;
    let new = snapshot_from(new_snapshot)?;
    let (patches, _) = otio_ext_sync::diff(&old, &new).map_err(|error| sync_error(&error))?;
    Ok(if pretty {
        otio_ext_sync::patch::to_json_pretty(&patches)
    } else {
        otio_ext_sync::patch::to_json(&patches)
    })
}

/// What a diff of the same snapshots does, as a JSON object.
#[pyfunction]
fn _sync_diff_stats(old_snapshot: &str, new_snapshot: &str) -> PyResult<String> {
    sync_enabled()?;
    let old = snapshot_from(old_snapshot)?;
    let new = snapshot_from(new_snapshot)?;
    let (_, stats) = otio_ext_sync::diff(&old, &new).map_err(|error| sync_error(&error))?;
    Ok(stats.to_json())
}

/// The timeline JSON `timeline` with the patches `patches` applied, as
/// compact JSON.
#[pyfunction]
fn _sync_apply(timeline: &str, patches: &str) -> PyResult<String> {
    sync_enabled()?;
    let patches = patches_from(patches)?;
    otio_ext_sync::apply(timeline, &patches).map_err(|error| sync_error(&error))
}

/// Applies `patches` to `timeline` in place.
///
/// What the patches remove leaves the document, so the wrappers held for it
/// are forgotten; see [`Shared::forget_removed`].
#[pyfunction]
fn _sync_apply_to(py: Python<'_>, timeline: &Bound<'_, PyAny>, patches: &str) -> PyResult<()> {
    sync_enabled()?;
    let patches = patches_from(patches)?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        sync_document::apply_to(document, id, &patches).map_err(|error| sync_error(&error))
    })?;
    shared.forget_removed(py)
}

/// Starts a transaction on `timeline`, returning the snapshot that is the
/// transaction.
#[pyfunction]
fn _sync_begin(
    timeline: &Bound<'_, PyAny>,
    sequence_id: &str,
    generation: i64,
) -> PyResult<String> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        let transaction = Transaction::begin(document, id, sequence_id, generation)
            .map_err(|error| sync_error(&error))?;
        Ok(transaction.before().to_json())
    })
}

/// Ends the transaction `before`, returning its patches as a compact JSON
/// list.
#[pyfunction]
fn _sync_commit(timeline: &Bound<'_, PyAny>, before: &str) -> PyResult<String> {
    sync_enabled()?;
    let before = snapshot_from(before)?;
    let (shared, id) = located(timeline)?;
    let patches = shared.write(|document| {
        Transaction::resume(id, before)
            .commit(document)
            .map_err(|error| sync_error(&error))
    })?;
    Ok(otio_ext_sync::patch::to_json(&patches))
}

/// Ends the transaction `before`, putting the timeline back as it was.
#[pyfunction]
fn _sync_rollback(py: Python<'_>, timeline: &Bound<'_, PyAny>, before: &str) -> PyResult<()> {
    sync_enabled()?;
    let before = snapshot_from(before)?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        Transaction::resume(id, before)
            .rollback(document)
            .map_err(|error| sync_error(&error))
    })?;
    shared.forget_removed(py)
}

/// Starts the journal of the document `timeline` is in.
#[pyfunction]
fn _sync_start_journal(timeline: &Bound<'_, PyAny>) -> PyResult<()> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        core_error(document.try_get(id))?;
        if !document.is_journalling() {
            document.start_journal();
        }
        Ok(())
    })
}

/// Stops the journal of the document `timeline` is in.
#[pyfunction]
fn _sync_stop_journal(timeline: &Bound<'_, PyAny>) -> PyResult<()> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        core_error(document.try_get(id))?;
        document.stop_journal();
        Ok(())
    })
}

/// Commits the journal of the document `timeline` is in, running the
/// `document_changed` hook, and returns whether anything changed.
#[pyfunction]
fn _sync_commit_journal(timeline: &Bound<'_, PyAny>) -> PyResult<bool> {
    sync_enabled()?;
    let (shared, id) = located(timeline)?;
    shared.write(|document| {
        core_error(document.try_get(id))?;
        let changes = otio_ext_sync::journal::commit(document)
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        Ok(!changes.is_empty())
    })
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(_ext_enable, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_is_enabled, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_enabled, module)?)?;
    module.add_function(wrap_pyfunction!(_ext_available, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_new_guid, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_guid, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_set_guid, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_stamp, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_find, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_diff, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_diff_stats, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_apply, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_apply_to, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_begin, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_commit, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_rollback, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_start_journal, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_stop_journal, module)?)?;
    module.add_function(wrap_pyfunction!(_sync_commit_journal, module)?)?;
    Ok(())
}

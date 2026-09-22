//! `opentimelineio._otio.bundle`: `.otioz` and `.otiod` file bundles.
//!
//! Upstream implements bundles in C++ (`bundle.h`) and binds them as a
//! submodule of its extension, which the `otioz` and `otiod` adapter modules
//! call. This is the same submodule, with the same classes, functions,
//! defaults and argument names, over the `otio-bundle` crate.
//!
//! Upstream reports a bundle that cannot be read or written as
//! `FILE_OPEN_FAILED` or `FILE_WRITE_FAILED`, which its bindings raise as
//! `OSError`; so does this, with upstream's message as its text.

use std::path::{Path, PathBuf};

use otio_bundle::{Error, MediaReferencePolicy, ReadOptions, WriteOptions};
use otio_core::{Document, NodeId};

use pyo3::exceptions::{PyOSError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyString};
use pyo3::{Py, PyAny};

use crate::arena::Shared;
use crate::objects::{Handle, core_error, handle_of, wrap};

/// The bundle media reference policy.
#[pyclass(
    name = "MediaReferencePolicy",
    module = "opentimelineio._otio.bundle",
    from_py_object
)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PyMediaReferencePolicy {
    /// Return an error if there are any non-file media references.
    #[pyo3(name = "error_if_not_file")]
    ErrorIfNotFile,
    /// Replace non-file media references with missing references.
    #[pyo3(name = "missing_if_not_file")]
    MissingIfNotFile,
    /// Replace all media references with missing references.
    #[pyo3(name = "all_missing")]
    AllMissing,
}

crate::enums::pybind11_enum!(PyMediaReferencePolicy "MediaReferencePolicy" [
    ErrorIfNotFile = "error_if_not_file",
    MissingIfNotFile = "missing_if_not_file",
    AllMissing = "all_missing",
] {});

impl From<PyMediaReferencePolicy> for MediaReferencePolicy {
    fn from(policy: PyMediaReferencePolicy) -> Self {
        match policy {
            PyMediaReferencePolicy::ErrorIfNotFile => Self::ErrorIfNotFile,
            PyMediaReferencePolicy::MissingIfNotFile => Self::MissingIfNotFile,
            PyMediaReferencePolicy::AllMissing => Self::AllMissing,
        }
    }
}

/// Options for writing bundles.
#[pyclass(name = "WriteOptions", module = "opentimelineio._otio.bundle")]
pub struct PyWriteOptions {
    /// Base directory for resolving relative media reference paths. If a
    /// media reference URL resolves to a relative path, it is resolved
    /// against this directory before being added to the bundle.
    #[pyo3(get, set)]
    relative_media_base_dir: Option<String>,
    /// The media reference policy.
    #[pyo3(get, set)]
    policy: PyMediaReferencePolicy,
    /// Number of spaces for JSON indentation.
    #[pyo3(get, set)]
    indent: i64,
}

#[pymethods]
impl PyWriteOptions {
    #[new]
    fn new() -> Self {
        Self {
            relative_media_base_dir: None,
            policy: PyMediaReferencePolicy::ErrorIfNotFile,
            indent: otio_core::DEFAULT_INDENT as i64,
        }
    }
}

impl PyWriteOptions {
    fn options(&self) -> WriteOptions {
        WriteOptions {
            relative_media_base_dir: self.relative_media_base_dir.as_ref().map(PathBuf::from),
            policy: self.policy.into(),
            // Upstream writes compact JSON for a negative indent; the Rust
            // writer's narrowest form is no indentation at all.
            indent: usize::try_from(self.indent).unwrap_or(0),
        }
    }
}

/// Options for reading bundles.
#[pyclass(name = "ReadOptions", module = "opentimelineio._otio.bundle")]
#[derive(Default)]
pub struct PyReadOptions {
    /// Extract the contents of the otioz bundle to this directory, which
    /// must not already exist.
    #[pyo3(get, set)]
    extract_path: Option<String>,
    /// Convert the media reference paths to absolute paths. If this is set
    /// to true for otioz files, an extract_path must also be set.
    #[pyo3(get, set)]
    absolute_media_reference_paths: bool,
}

#[pymethods]
impl PyReadOptions {
    #[new]
    fn new() -> Self {
        Self::default()
    }
}

impl PyReadOptions {
    fn options(&self) -> ReadOptions {
        ReadOptions {
            extract_path: self.extract_path.as_ref().map(PathBuf::from),
            absolute_media_reference_paths: self.absolute_media_reference_paths,
        }
    }
}

fn bundle_error(error: Error) -> PyErr {
    match error {
        Error::FileOpen(details) | Error::FileWrite(details) => PyOSError::new_err(details),
        Error::NotATimeline(_) => PyTypeError::new_err(error.to_string()),
        Error::Core(error) => core_error::<()>(Err(error)).unwrap_err(),
        error => PyValueError::new_err(error.to_string()),
    }
}

/// Runs `f` over the document the timeline `value` lives in.
fn with_timeline<T>(
    value: &Bound<'_, PyAny>,
    f: impl FnOnce(&Document, NodeId) -> Result<T, Error>,
) -> PyResult<T> {
    let handle = handle_of(value)
        .map_err(|_| PyTypeError::new_err("a bundle is written from a Timeline"))?;
    let (shared, id) = handle.live()?;
    shared
        .read(|document| Ok(f(document, id)))?
        .map_err(bundle_error)
}

/// Hands a document a bundle held to Python, as its root object.
fn into_python(py: Python<'_>, document: Document) -> PyResult<Py<PyAny>> {
    let root = document
        .root()
        .ok_or_else(|| PyValueError::new_err("the bundle holds no object"))?;
    let shared = Shared::new();
    shared.write(|slot| {
        *slot = document;
        Ok(())
    })?;
    Ok(wrap(py, &Handle { shared, id: root })?.unbind())
}

fn write_options(options: Option<PyRef<'_, PyWriteOptions>>) -> WriteOptions {
    options.map_or_else(WriteOptions::default, |options| options.options())
}

fn read_options(options: Option<PyRef<'_, PyReadOptions>>) -> ReadOptions {
    options.map_or_else(ReadOptions::default, |options| options.options())
}

/// Convert a file:// URL to a filesystem path.
///
/// Handles Windows drive letters in netloc position (file://C:/...), UNC
/// paths (file://host/share/...), percent-encoded characters, and the
/// 'localhost' authority. Returns the input unchanged if it is a bare path
/// with no URL scheme. Returns None if the URL uses a non-file scheme (e.g.
/// http://...).
///
/// This follows upstream's `std::stoi`-based percent decoding exactly, as
/// `otio_core::bundle::file_from_url` does: an escape `std::stoi` cannot
/// read raises `ValueError("stoi")`, and one that spells bytes that are not
/// UTF-8 raises the `UnicodeDecodeError` pybind11 would.
#[pyfunction]
#[pyo3(signature = (url))]
fn file_from_url(py: Python<'_>, url: &Bound<'_, PyAny>) -> PyResult<Option<Py<PyAny>>> {
    let url = url_argument(url)?;
    let Some(path) = otio_core::bundle::file_from_url(&url)
        .map_err(|error| PyValueError::new_err(error.to_string()))?
    else {
        return Ok(None);
    };
    // pybind11 turns the `std::string` upstream returns into a `str` with
    // `PyUnicode_DecodeUTF8`, as `bytes.decode` does, so bytes that are not
    // UTF-8 raise the same `UnicodeDecodeError`, message and all.
    Ok(Some(
        PyBytes::new(py, &path)
            .call_method1("decode", ("utf-8",))?
            .unbind(),
    ))
}

/// The bytes of `file_from_url`'s argument, taken as pybind11 takes a
/// `std::string`.
///
/// pybind11 takes a `str` as its UTF-8 and a `bytes` or `bytearray` as it
/// is, so a path that is not UTF-8 can be passed as bytes. Anything else,
/// including a `str` with a lone surrogate (which is how `os.fsdecode`
/// spells such a path), cannot be converted, and pybind11 raises its
/// `TypeError` for arguments that match no overload.
fn url_argument(url: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(text) = url.cast::<PyString>() {
        if let Ok(text) = text.to_str() {
            return Ok(text.as_bytes().to_vec());
        }
    } else if let Ok(bytes) = url.cast::<PyBytes>() {
        return Ok(bytes.as_bytes().to_vec());
    } else if let Ok(bytes) = url.cast::<PyByteArray>() {
        return Ok(bytes.to_vec());
    }
    Err(PyTypeError::new_err(format!(
        "file_from_url(): incompatible function arguments. The following argument types \
         are supported:\n    1. (url: str) -> str | None\n\nInvoked with: {}",
        url.repr()?
    )))
}

/// Calculate the total uncompressed size of the files that would be written
/// to a bundle, without actually writing it. This is useful for estimating
/// the disk space required.
#[pyfunction]
#[pyo3(signature = (timeline, options = None))]
fn dry_run(
    timeline: &Bound<'_, PyAny>,
    options: Option<PyRef<'_, PyWriteOptions>>,
) -> PyResult<u64> {
    let options = write_options(options);
    with_timeline(timeline, |document, id| {
        otio_bundle::dry_run(document, id, &options)
    })
}

/// Write a timeline and it's referenced media to an .otioz bundle.
#[pyfunction]
#[pyo3(signature = (timeline, path, options = None))]
fn write_otioz(
    timeline: &Bound<'_, PyAny>,
    path: &str,
    options: Option<PyRef<'_, PyWriteOptions>>,
) -> PyResult<bool> {
    let options = write_options(options);
    with_timeline(timeline, |document, id| {
        otio_bundle::write_otioz(document, id, Path::new(path), &options)
    })?;
    Ok(true)
}

/// Read a timeline from an .otioz bundle.
#[pyfunction]
#[pyo3(signature = (path, options = None))]
fn read_otioz(
    py: Python<'_>,
    path: &str,
    options: Option<PyRef<'_, PyReadOptions>>,
) -> PyResult<Py<PyAny>> {
    let document =
        otio_bundle::read_otioz(Path::new(path), &read_options(options)).map_err(bundle_error)?;
    into_python(py, document)
}

/// Write a timeline and it's referenced media to an .otiod bundle.
#[pyfunction]
#[pyo3(signature = (timeline, path, options = None))]
fn write_otiod(
    timeline: &Bound<'_, PyAny>,
    path: &str,
    options: Option<PyRef<'_, PyWriteOptions>>,
) -> PyResult<bool> {
    let options = write_options(options);
    with_timeline(timeline, |document, id| {
        otio_bundle::write_otiod(document, id, Path::new(path), &options)
    })?;
    Ok(true)
}

/// Read a timeline from an .otiod bundle.
#[pyfunction]
#[pyo3(signature = (path, options = None))]
fn read_otiod(
    py: Python<'_>,
    path: &str,
    options: Option<PyRef<'_, PyReadOptions>>,
) -> PyResult<Py<PyAny>> {
    let document =
        otio_bundle::read_otiod(Path::new(path), &read_options(options)).map_err(bundle_error)?;
    into_python(py, document)
}

/// Adds the `bundle` submodule to the extension module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let bundle = PyModule::new(module.py(), "bundle")?;
    bundle.add_class::<PyMediaReferencePolicy>()?;
    bundle.add_class::<PyWriteOptions>()?;
    bundle.add_class::<PyReadOptions>()?;
    bundle.add_function(wrap_pyfunction!(file_from_url, &bundle)?)?;
    bundle.add_function(wrap_pyfunction!(dry_run, &bundle)?)?;
    bundle.add_function(wrap_pyfunction!(write_otioz, &bundle)?)?;
    bundle.add_function(wrap_pyfunction!(read_otioz, &bundle)?)?;
    bundle.add_function(wrap_pyfunction!(write_otiod, &bundle)?)?;
    bundle.add_function(wrap_pyfunction!(read_otiod, &bundle)?)?;
    module.add_submodule(&bundle)?;
    // Upstream's pybind11 lists the submodule in `sys.modules` under its
    // full name, which is what lets pickle find `MediaReferencePolicy` by
    // its `__module__` again.
    module
        .py()
        .import("sys")?
        .getattr("modules")?
        .set_item("opentimelineio._otio.bundle", &bundle)?;
    Ok(())
}

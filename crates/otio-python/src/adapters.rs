//! The file-format adapters, for the Python modules that present them.
//!
//! Upstream's adapters are Python modules found through a plugin manifest,
//! each exposing some of `read_from_string`, `read_from_file`,
//! `write_to_string` and `write_to_file` with keyword arguments of its own.
//! The modules under `python/opentimelineio/adapters/` keep that shape,
//! signatures and all, and each is a thin layer over one pair of functions
//! here, which hand the work to the adapter crate.
//!
//! The functions take each format's options as named arguments rather than as
//! a dictionary, so the Python module is the only place that has to know
//! upstream's spelling of them. Each also takes the exception class to raise
//! when the input does not parse, because upstream's adapters disagree about
//! it: the EDL one raises its own `EDLParseError`, ALE its own
//! `ALEParseError`, and both FCP XML flavours plain `ValueError`. The module
//! defines the class, and this raises it.
//!
//! AAF is the exception to the pairs: it is read from a file and written to
//! one, as upstream's adapter is, and its writer fails in more ways than a
//! parse error, so it maps the `otio-aaf` crate's own error rather than the
//! adapter trait's.

use std::path::PathBuf;

use otio_aaf::Aaf;
use otio_adapter::{Adapter, Error, TextAdapter};
use otio_ale::Ale;
use otio_cmx3600::{Cmx3600, Style};
use otio_core::Document;
use otio_fcp7::Fcp7Xml;
use otio_fcpx::FcpxXml;

use pyo3::exceptions::{PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyType;
use pyo3::{Py, PyAny};

use crate::arena::Shared;
use crate::objects::{Handle, core_error, handle_of, wrap};

/// Turns an adapter's failure into the Python exception upstream raises.
fn adapter_error(py: Python<'_>, error: Error, parse_error: &Bound<'_, PyType>) -> PyErr {
    match error {
        Error::Io(error) => error.into(),
        Error::Core(error) => core_error::<()>(Err(error)).unwrap_err(),
        Error::Time(error) => PyValueError::new_err(error.to_string()),
        Error::Encoding(error) => PyValueError::new_err(error.to_string()),
        error @ Error::Parse { .. } => PyErr::from_type(parse_error.clone(), error.to_string()),
        // Upstream's EDL writer raises `NotSupportedError` for a timeline it
        // cannot express, and it is the one writer here that refuses
        // anything; `exceptions.py` defines the class, so it is looked up.
        error => match not_supported(py) {
            Ok(class) => PyErr::from_type(class, error.to_string()),
            Err(lookup) => lookup,
        },
    }
}

/// `opentimelineio.exceptions.NotSupportedError`.
fn not_supported(py: Python<'_>) -> PyResult<Bound<'_, PyType>> {
    Ok(py
        .import("opentimelineio.exceptions")?
        .getattr("NotSupportedError")?
        .cast_into::<PyType>()?)
}

/// Hands a document an adapter read to Python, as its root object.
fn into_python(py: Python<'_>, document: Document) -> PyResult<Py<PyAny>> {
    let root = document
        .root()
        .ok_or_else(|| PyValueError::new_err("the adapter read no object"))?;
    let shared = Shared::new();
    shared.write(|slot| {
        *slot = document;
        Ok(())
    })?;
    Ok(wrap(py, &Handle { shared, id: root })?.unbind())
}

/// Runs a writer over the document `value` lives in, with `value` as its root.
///
/// A writer starts from the document's root, and an object built from Python
/// lives in a document that may hold more than it — the timeline a clip sits
/// in, say — and whose root is not set at all. So the root is pointed at the
/// object for the length of the write and put back afterwards. Nothing here
/// calls back into Python while the document is held.
fn write_from<T, E>(
    value: &Bound<'_, PyAny>,
    write: impl FnOnce(&Document) -> Result<T, E>,
) -> PyResult<Result<T, E>> {
    let handle = handle_of(value).map_err(|_| {
        PyTypeError::new_err(format!(
            "an adapter writes an OpenTimelineIO object, not a {}",
            value
                .get_type()
                .name()
                .map_or_else(|_| "value".into(), |name| name.to_string())
        ))
    })?;
    let (shared, id) = handle.live()?;
    shared.write(|document| {
        let previous = document.root();
        document.set_root(Some(id));
        let written = write(document);
        document.set_root(previous);
        Ok(written)
    })
}

/// Reads a CMX 3600 EDL.
#[pyfunction]
#[pyo3(signature = (input, rate, ignore_timecode_mismatch, parse_error))]
fn read_cmx_3600(
    py: Python<'_>,
    input: &str,
    rate: f64,
    ignore_timecode_mismatch: bool,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<Py<PyAny>> {
    let options = otio_cmx3600::ReadOptions {
        rate,
        ignore_timecode_mismatch,
    };
    let document = Cmx3600::read_from_str(input, &options)
        .map_err(|error| adapter_error(py, error, parse_error))?;
    into_python(py, document)
}

/// Writes a CMX 3600 EDL.
///
/// `style` is upstream's string. An unknown one is refused with
/// `NotSupportedError`, which is what upstream raises for it too.
#[pyfunction]
#[pyo3(signature = (input, rate, style, reelname_len, parse_error))]
fn write_cmx_3600(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    rate: Option<f64>,
    style: &str,
    reelname_len: Option<usize>,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<String> {
    let style: Style = style
        .parse()
        .map_err(|error| adapter_error(py, error, parse_error))?;
    let options = otio_cmx3600::WriteOptions {
        rate,
        style,
        reelname_len,
    };
    write_from(input, |document| {
        Cmx3600::write_to_string(document, &options)
    })?
    .map_err(|error| adapter_error(py, error, parse_error))
}

/// Reads an Avid Log Exchange file.
#[pyfunction]
#[pyo3(signature = (input, fps, name_column, parse_error))]
fn read_ale(
    py: Python<'_>,
    input: &str,
    fps: f64,
    name_column: String,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<Py<PyAny>> {
    let options = otio_ale::ReadOptions { fps, name_column };
    let document = Ale::read_from_str(input, &options)
        .map_err(|error| adapter_error(py, error, parse_error))?;
    into_python(py, document)
}

/// Writes an Avid Log Exchange file.
#[pyfunction]
#[pyo3(signature = (input, columns, fps, video_format, parse_error))]
fn write_ale(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    columns: Option<Vec<String>>,
    fps: Option<f64>,
    video_format: Option<String>,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<String> {
    let options = otio_ale::WriteOptions {
        columns,
        fps,
        video_format,
    };
    write_from(input, |document| Ale::write_to_string(document, &options))?
        .map_err(|error| adapter_error(py, error, parse_error))
}

/// Reads a Final Cut Pro 7 XML file.
#[pyfunction]
fn read_fcp_xml(
    py: Python<'_>,
    input: &str,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<Py<PyAny>> {
    let document = Fcp7Xml::read_from_str(input, &otio_fcp7::ReadOptions::default())
        .map_err(|error| adapter_error(py, error, parse_error))?;
    into_python(py, document)
}

/// Writes a Final Cut Pro 7 XML file.
#[pyfunction]
fn write_fcp_xml(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<String> {
    write_from(input, |document| {
        Fcp7Xml::write_to_string(document, &otio_fcp7::WriteOptions::default())
    })?
    .map_err(|error| adapter_error(py, error, parse_error))
}

/// Reads a Final Cut Pro X XML file.
#[pyfunction]
fn read_fcpx_xml(
    py: Python<'_>,
    input: &str,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<Py<PyAny>> {
    let document = FcpxXml::read_from_str(input, &otio_fcpx::ReadOptions::default())
        .map_err(|error| adapter_error(py, error, parse_error))?;
    into_python(py, document)
}

/// Writes a Final Cut Pro X XML file.
#[pyfunction]
fn write_fcpx_xml(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    parse_error: &Bound<'_, PyType>,
) -> PyResult<String> {
    write_from(input, |document| {
        FcpxXml::write_to_string(document, &otio_fcpx::WriteOptions::default())
    })?
    .map_err(|error| adapter_error(py, error, parse_error))
}

/// The name FCP X gives a video format, from its rate and `ffprobe`'s
/// `widthxheight`.
#[pyfunction]
fn fcpx_format_name(frame_rate: i64, frame_size: &str) -> String {
    otio_fcpx::format_name(frame_rate, frame_size)
}

/// Reads an AAF from a file on disk, running the two optional passes as
/// asked.
///
/// The file is read by seeking around it rather than loaded whole, so a large
/// AAF stays on disk.
#[pyfunction]
fn read_aaf_file(
    py: Python<'_>,
    path: PathBuf,
    parse_error: &Bound<'_, PyType>,
    simplify: bool,
    attach_markers: bool,
) -> PyResult<Py<PyAny>> {
    let options = otio_aaf::ReadOptions::new()
        .with_simplify(simplify)
        .with_attach_markers(attach_markers);
    let document = Aaf::read_from_file(path, &options)
        .map_err(|error| adapter_error(py, error, parse_error))?;
    into_python(py, document)
}

/// Writes an AAF file, as upstream's `write_to_file` does.
///
/// The file is only created once the whole AAF has been built, so a timeline
/// the writer refuses leaves nothing behind.
///
/// `_calls_tsv` is for this package's tests, not for use: the sidecar of one
/// of `otio-aaf`'s written fixtures, whose recorded times and identifiers
/// are replayed so that the file comes out identical to the one upstream
/// wrote. After the write, a writer that asked for other values than the
/// sidecar lists raises `RuntimeError`.
#[pyfunction]
#[pyo3(signature = (
    input,
    path,
    error_class,
    prefer_file_mob_id,
    use_empty_mob_ids,
    embed_essence,
    create_edgecode,
    _calls_tsv = None,
))]
#[allow(clippy::too_many_arguments)]
fn write_aaf_file(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
    path: PathBuf,
    error_class: &Bound<'_, PyType>,
    prefer_file_mob_id: bool,
    use_empty_mob_ids: bool,
    embed_essence: bool,
    create_edgecode: bool,
    _calls_tsv: Option<PathBuf>,
) -> PyResult<()> {
    let mut options = otio_aaf::WriteOptions::new()
        .with_prefer_file_mob_id(prefer_file_mob_id)
        .with_use_empty_mob_ids(use_empty_mob_ids)
        .with_embed_essence(embed_essence)
        .with_create_edgecode(create_edgecode);
    let sidecar = _calls_tsv
        .map(|tsv| {
            let name = tsv.file_name().map_or_else(
                || "the fixture".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
            otio_aaf::replay::Sidecar::read(&name, &tsv).map_err(PyValueError::new_err)
        })
        .transpose()?;
    if let Some(sidecar) = &sidecar {
        options = options.with_replay(sidecar);
    }
    let bytes = write_from(input, |document| {
        otio_aaf::write_to_bytes_with(document, &options)
    })?
    .map_err(|error| aaf_write_error(py, error, error_class, embed_essence))?;
    if let Some(sidecar) = &sidecar {
        sidecar.replay.finish().map_err(PyRuntimeError::new_err)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Turns the AAF writer's failure into the Python exception upstream raises.
///
/// Upstream raises `NotSupportedError` for a timeline it has no AAF for, and
/// its own `AAFAdapterError` for the rest. Embedding essence is the one
/// option this writer does not implement, so asking for it raises
/// `NotImplementedError`, pointing at where the work is tracked.
fn aaf_write_error(
    py: Python<'_>,
    error: otio_aaf::Error,
    error_class: &Bound<'_, PyType>,
    embed_essence: bool,
) -> PyErr {
    match error {
        otio_aaf::Error::Io(error) => error.into(),
        otio_aaf::Error::Otio(error) => core_error::<()>(Err(error)).unwrap_err(),
        error @ otio_aaf::Error::Unsupported(_) if embed_essence => PyNotImplementedError::new_err(
            format!("{error} (tracked in https://github.com/alchemist-editor/otio-rust/issues/66)"),
        ),
        error @ otio_aaf::Error::Unsupported(_) => match not_supported(py) {
            Ok(class) => PyErr::from_type(class, error.to_string()),
            Err(lookup) => lookup,
        },
        error => PyErr::from_type(error_class.clone(), error.to_string()),
    }
}

/// Registers the adapter functions on a module.
pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(read_cmx_3600, module)?)?;
    module.add_function(wrap_pyfunction!(write_cmx_3600, module)?)?;
    module.add_function(wrap_pyfunction!(read_ale, module)?)?;
    module.add_function(wrap_pyfunction!(write_ale, module)?)?;
    module.add_function(wrap_pyfunction!(read_fcp_xml, module)?)?;
    module.add_function(wrap_pyfunction!(write_fcp_xml, module)?)?;
    module.add_function(wrap_pyfunction!(read_fcpx_xml, module)?)?;
    module.add_function(wrap_pyfunction!(write_fcpx_xml, module)?)?;
    module.add_function(wrap_pyfunction!(fcpx_format_name, module)?)?;
    module.add_function(wrap_pyfunction!(read_aaf_file, module)?)?;
    module.add_function(wrap_pyfunction!(write_aaf_file, module)?)?;
    Ok(())
}

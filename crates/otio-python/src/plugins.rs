//! The media linkers and hook scripts registered natively, in
//! [`otio_adapter::plugins`], as Python's plugin system reaches them.
//!
//! Python keeps upstream's plugin system: manifests name Python modules, and
//! upstream's `Adapter`, `media_linker` and `hooks` run them, handing them
//! Python objects and the caller's own dictionaries. What these functions add
//! is the linkers and scripts some other part of the process registered in
//! Rust: `opentimelineio.media_linker` and `opentimelineio.hooks` find them by
//! name, after the ones manifests declare, and run them here.
//!
//! A native plugin is handed its arguments as OTIO values. An argument that
//! has no OTIO value, such as an open file, is left out of what it is handed,
//! since a native plugin could not use it anyway.

use otio_adapter::plugins;
use otio_core::{AnyDictionary, NodeId};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pyo3::{Py, PyAny};

use crate::arena::Shared;
use crate::objects::{Handle, core_error, handle_of, wrap, wrap_root};
use crate::values::python_to_any;

/// The names of the natively registered media linkers.
#[pyfunction]
fn native_media_linker_names() -> Vec<String> {
    plugins::registry().media_linker_names()
}

/// The names of the natively registered hook scripts.
#[pyfunction]
fn native_hook_script_names() -> Vec<String> {
    plugins::registry().hook_script_names()
}

/// The natively declared hooks.
#[pyfunction]
fn native_hook_names() -> Vec<String> {
    plugins::registry().hook_names()
}

/// The natively registered scripts attached to `hook`, in the order they
/// run, or `None` if it is not declared natively.
#[pyfunction]
fn native_scripts_attached_to(hook: &str) -> Option<Vec<String>> {
    plugins::registry()
        .scripts_attached_to(hook)
        .map(<[String]>::to_vec)
}

/// The arguments a native plugin is handed: each entry of `arguments` that
/// has an OTIO value.
fn native_arguments(home: &Shared, arguments: Option<&Bound<'_, PyDict>>) -> AnyDictionary {
    let mut native = AnyDictionary::new();
    for (key, value) in arguments.into_iter().flatten() {
        let (Ok(key), Ok(value)) = (key.extract::<String>(), python_to_any(home, &value)) else {
            continue;
        };
        native.insert(key, value);
    }
    native
}

/// Wraps what a native plugin returned, `id` in the document `like` lives
/// in, reusing the wrapper it has if it has one.
fn wrap_result(py: Python<'_>, like: &Handle, id: NodeId) -> PyResult<Py<PyAny>> {
    let handle = like.sibling(id)?;
    let owned = handle
        .shared
        .read(|document| Ok(document.owner_of(id).is_some()))?;
    if owned || id == like.live()?.1 {
        Ok(wrap(py, &handle)?.unbind())
    } else {
        Ok(wrap_root(py, &handle)?.unbind())
    }
}

/// Runs the native media linker `name` on `clip`, returning the media
/// reference it made, or `None` to leave the clip as it is.
#[pyfunction]
#[pyo3(signature = (name, clip, arguments = None))]
fn run_native_media_linker(
    py: Python<'_>,
    name: &str,
    clip: &Bound<'_, PyAny>,
    arguments: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let linker = plugins::registry()
        .media_linker(name)
        .cloned()
        .ok_or_else(|| PyValueError::new_err(format!("no native media linker named {name}")))?;
    let handle = handle_of(clip)?;
    let (shared, id) = handle.live()?;
    let arguments = native_arguments(&shared, arguments);
    let linked = shared.write(|document| {
        core_error(document.try_get(id))?;
        linker
            .link(document, id, &arguments)
            .map_err(|message| PyRuntimeError::new_err(format!("{name}: {message}")))
    })?;
    match linked {
        Some(reference) => wrap_result(py, &handle, reference),
        None => Ok(py.None()),
    }
}

/// Runs the native hook script `name` on `target`, returning what it
/// returned.
#[pyfunction]
#[pyo3(signature = (name, target, arguments = None))]
fn run_native_hook_script(
    py: Python<'_>,
    name: &str,
    target: &Bound<'_, PyAny>,
    arguments: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let script = plugins::registry()
        .hook_script(name)
        .cloned()
        .ok_or_else(|| PyValueError::new_err(format!("no native hook script named {name}")))?;
    let handle = handle_of(target)?;
    let (shared, id) = handle.live()?;
    let arguments = native_arguments(&shared, arguments);
    let result = shared.write(|document| {
        core_error(document.try_get(id))?;
        let result = script
            .run(document, id, &arguments)
            .map_err(|message| PyRuntimeError::new_err(format!("{name}: {message}")))?;
        core_error(document.try_get(result))?;
        Ok(result)
    })?;
    wrap_result(py, &handle, result)
}

/// For this package's tests: registers, natively, a media linker named
/// `native_example` and a hook script named `native_example`, the latter
/// attached to `native_example_hook`, or forgets all three with `False`.
///
/// The linker gives a clip a `MissingReference` named after it plus
/// `_native`, carrying its arguments as metadata, as upstream's example
/// linker does with `_tweaked`. The script records the names of its
/// arguments in the metadata of what it is handed, under `native_hook`.
#[pyfunction]
#[pyo3(signature = (register = true))]
fn register_native_example_plugins(register: bool) {
    use otio_core::schema::MissingReference;
    use otio_core::{Any, Node};

    let mut registry = plugins::registry();
    if !register {
        registry.remove_media_linker("native_example");
        registry.remove_hook_script("native_example");
        registry.set_scripts_attached_to("native_example_hook", Vec::new());
        return;
    }
    registry.register_media_linker(
        "native_example",
        plugins::MediaLinker::new(|document, clip, arguments| {
            let name = document
                .try_get(clip)
                .map_err(|error| error.to_string())?
                .name()
                .to_owned();
            let mut reference = MissingReference::default();
            reference.media.base.name = format!("{name}_native");
            reference.media.base.metadata = arguments.clone();
            Ok(Some(document.insert(Node::MissingReference(reference))))
        }),
    );
    registry.register_hook_script(
        "native_example",
        plugins::HookScript::new(|document, target, arguments| {
            let keys = arguments.keys().cloned().map(Any::String).collect();
            if let Some(base) = document.get_mut(target).and_then(Node::base_mut) {
                base.metadata
                    .insert("native_hook".to_owned(), Any::Vector(keys));
            }
            Ok(target)
        }),
    );
    registry.set_scripts_attached_to("native_example_hook", vec!["native_example".to_owned()]);
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let testing = crate::testing_hooks::submodule(module)?;
    testing.add_function(wrap_pyfunction!(register_native_example_plugins, &testing)?)?;
    module.add_function(wrap_pyfunction!(native_media_linker_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_hook_script_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_hook_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_scripts_attached_to, module)?)?;
    module.add_function(wrap_pyfunction!(run_native_media_linker, module)?)?;
    module.add_function(wrap_pyfunction!(run_native_hook_script, module)?)?;
    Ok(())
}

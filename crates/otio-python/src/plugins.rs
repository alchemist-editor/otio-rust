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

/// The names of the natively registered effects linkers, less the ones the
/// library builds in.
///
/// The built-in `otio_fx_registry` is listed by the package's own built-in
/// manifest, as upstream's is, and its module runs it here, so listing it a
/// second time as a native linker would be listing it twice; and a test that
/// replaces the manifest would still find it.
#[pyfunction]
fn native_effects_linker_names() -> Vec<String> {
    plugins::registry()
        .effects_linker_names()
        .into_iter()
        .filter(|name| name != plugins::OTIO_FX_REGISTRY)
        .collect()
}

/// The hooks every read and write runs, which the library declares
/// whether or not a manifest does.
#[pyfunction]
fn native_adapter_hook_names() -> Vec<String> {
    plugins::ADAPTER_HOOKS
        .iter()
        .map(|hook| (*hook).to_owned())
        .collect()
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

/// Records as owned every object in `arguments` that the plugin left held
/// in the document, as storing it from Python would, so that its wrapper
/// lives on once the caller lets go of it.
fn mark_kept(py: Python<'_>, home: &Shared, arguments: &AnyDictionary) -> PyResult<()> {
    let mut objects = Vec::new();
    for value in arguments.values() {
        value.visit_objects(&mut |id| objects.push(id));
    }
    let kept: Vec<NodeId> = home.read(|document| {
        Ok(objects
            .into_iter()
            .filter(|id| document.owner_of(*id).is_some())
            .collect())
    })?;
    for id in kept {
        home.mark_owned(py, id, None)?;
    }
    Ok(())
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
        let linked = linker
            .link(document, id, &arguments)
            .map_err(|message| PyRuntimeError::new_err(format!("{name}: {message}")))?;
        // As `plugins::link_media` does, refuse anything but a media
        // reference, which the adapter would otherwise put on the clip.
        if let Some(reference) = linked {
            let node = core_error(document.try_get(reference))?;
            if node.media().is_none() {
                return Err(PyRuntimeError::new_err(format!(
                    "{name}: the media linker returned a {}, not a media reference",
                    node.schema_name()
                )));
            }
        }
        Ok(linked)
    });
    // A linker that fails may still have stored an argument.
    mark_kept(py, &shared, &arguments)?;
    let linked = linked?;
    match linked {
        Some(reference) => wrap_result(py, &handle, reference),
        None => Ok(py.None()),
    }
}

/// Runs the native effects linker `name` on `effect`, held by `parent`,
/// returning the effect it made, or `None` to leave the effect as it is.
///
/// This is also how the built-in `otio_fx_registry` runs: its Python module
/// hands its effects here.
#[pyfunction]
#[pyo3(signature = (name, effect, parent = None, arguments = None))]
fn run_native_effects_linker(
    py: Python<'_>,
    name: &str,
    effect: &Bound<'_, PyAny>,
    parent: Option<&Bound<'_, PyAny>>,
    arguments: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let linker = plugins::registry()
        .effects_linker(name)
        .cloned()
        .ok_or_else(|| PyValueError::new_err(format!("no native effects linker named {name}")))?;
    let handle = handle_of(effect)?;
    let (shared, id) = handle.live()?;
    // A parent in another document is no parent the linker could reach.
    let parent = match parent {
        Some(parent) if !parent.is_none() => {
            let (home, parent) = handle_of(parent)?.live()?;
            home.is(&shared)?.then_some(parent)
        }
        _ => None,
    };
    let arguments = native_arguments(&shared, arguments);
    let linked = shared.write(|document| {
        core_error(document.try_get(id))?;
        let linked = linker
            .link(document, id, parent, &arguments)
            .map_err(|message| PyRuntimeError::new_err(format!("{name}: {message}")))?;
        // As `plugins::link_effects` does, refuse anything but an effect.
        if let Some(made) = linked {
            let node = core_error(document.try_get(made))?;
            if node.effect().is_none() {
                return Err(PyRuntimeError::new_err(format!(
                    "{name}: the effects linker returned a {}, not an effect",
                    node.schema_name()
                )));
            }
        }
        Ok(linked)
    });
    mark_kept(py, &shared, &arguments)?;
    match linked? {
        Some(made) => wrap_result(py, &handle, made),
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
    });
    // A script that fails may still have stored an argument.
    mark_kept(py, &shared, &arguments)?;
    let result = result?;
    wrap_result(py, &handle, result)
}

/// For this package's tests: registers, natively, a media linker named
/// `native_example` and a hook script named `native_example`, the latter
/// attached to `native_example_hook`, a media linker named `native_broken`
/// that stores its arguments in the clip's metadata and then wrongly hands
/// back the clip itself, and an effects linker named `native_example`; or
/// forgets them all with `False`.
///
/// The linker gives a clip a `MissingReference` named after it plus
/// `_native`, carrying its arguments as metadata, as upstream's example
/// linker does with `_tweaked`. The script records the names of its
/// arguments in the metadata of what it is handed, under `native_hook`. The
/// effects linker gives an effect the id `native.` plus its effect name, and
/// records its parent's schema and its arguments' names in its parameters;
/// it leaves an effect named `keep` alone.
#[pyfunction]
#[pyo3(signature = (register = true))]
fn register_native_example_plugins(register: bool) {
    use otio_core::schema::MissingReference;
    use otio_core::{Any, Node};

    let mut registry = plugins::registry();
    if !register {
        registry.remove_media_linker("native_example");
        registry.remove_media_linker("native_broken");
        registry.remove_hook_script("native_example");
        registry.remove_effects_linker("native_example");
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
    registry.register_media_linker(
        "native_broken",
        plugins::MediaLinker::new(|document, clip, arguments| {
            if let Some(base) = document.get_mut(clip).and_then(Node::base_mut) {
                base.metadata.extend(arguments.clone());
            }
            Ok(Some(clip))
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
    registry.register_effects_linker(
        "native_example",
        plugins::EffectsLinker::new(|document, effect, parent, arguments| {
            let parent = match parent {
                Some(parent) => document
                    .try_get(parent)
                    .map_err(|error| error.to_string())?
                    .schema_name()
                    .to_string(),
                None => String::new(),
            };
            let data = document
                .get_mut(effect)
                .and_then(Node::effect_mut)
                .ok_or_else(|| "not an effect".to_owned())?;
            if data.effect_name == "keep" {
                return Ok(None);
            }
            data.effect_id = format!("native.{}", data.effect_name);
            data.parameters
                .insert("parent".to_owned(), Any::String(parent));
            let keys = arguments.keys().cloned().map(Any::String).collect();
            data.parameters
                .insert("arguments".to_owned(), Any::Vector(keys));
            Ok(Some(effect))
        }),
    );
}

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let testing = crate::testing_hooks::submodule(module)?;
    testing.add_function(wrap_pyfunction!(register_native_example_plugins, &testing)?)?;
    module.add_function(wrap_pyfunction!(native_media_linker_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_effects_linker_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_adapter_hook_names, module)?)?;
    module.add_function(wrap_pyfunction!(run_native_effects_linker, module)?)?;
    module.add_function(wrap_pyfunction!(native_hook_script_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_hook_names, module)?)?;
    module.add_function(wrap_pyfunction!(native_scripts_attached_to, module)?)?;
    module.add_function(wrap_pyfunction!(run_native_media_linker, module)?)?;
    module.add_function(wrap_pyfunction!(run_native_hook_script, module)?)?;
    Ok(())
}

//! Media linkers and hook scripts, for C.
//!
//! Upstream OpenTimelineIO has two plugin points. A *media linker* is handed
//! each clip a read produced and returns the media reference the clip should
//! point at instead. A *hook script* is handed what a read produced, or what
//! is about to be written, at a named point, and returns what to go on with.
//! Both are functions here, registered under a name with
//! [`otio_register_media_linker`] and [`otio_register_hook_script`].
//!
//! ```text
//!  read ──► post_adapter_read ──► media linker, per clip ──► post_media_linker
//!  pre_adapter_write ──► write ──► post_adapter_write
//! ```
//!
//! Every read and write runs that sequence, as upstream's adapters do:
//! [`otio_read_from_bytes`](crate::otio_read_from_bytes) and the rest run the
//! hooks, and link media with the linker their options name, or the one the
//! `OTIO_DEFAULT_MEDIA_LINKER` environment variable names if they name none.
//! A script runs at a hook only once it is attached there with
//! [`otio_attach_hook_script`].
//!
//! # The callback
//!
//! A linker and a script are both an [`OtioPluginFn`]. It is handed:
//!
//! - the document it works in, which it may edit but must not free or keep
//!   beyond the call;
//! - the object it works on: the clip to link, or what the hook runs on;
//! - an object whose metadata holds the arguments, as upstream's
//!   `argument_map`, which it reads with the `otio_metadata_` calls and must
//!   not keep beyond the call either;
//! - where to write its result, and room for a message saying why it failed.
//!
//! It writes the object to go on with to `out_result`: for a hook script,
//! the object it was handed or another in the same document; for a linker, a
//! media reference in the same document for the clip to use in place of its
//! active one, or `otio_node_none()` to leave the clip as it is. It returns
//! `OTIO_STATUS_OK`, or any other status to stop the read or write, having
//! written a NUL-terminated sentence saying why to `message` if it can. The
//! call that ran it then fails with `OTIO_STATUS_PLUGIN_ERROR` and that
//! sentence.
//!
//! The `context` given at registration is handed back on every call, and
//! handed to the `release` function, if one was given, once the library has
//! no more use for it: when the name is registered again, or unregistered.
//!
//! The registry is process-wide, and a read on one thread may call a plugin
//! registered on another, so a plugin must be safe to call from any thread
//! that reads or writes. Nothing is called while the registry is locked, so a
//! plugin may register, unregister or attach others.

use std::ffi::{c_char, c_void};
use std::sync::Arc;

use otio_adapter::plugins::{self, HookScript, LinkerChoice, MediaLinker, PluginArguments};
use otio_core::schema::SerializableCollection;
use otio_core::{Any, AnyDictionary, Document, Node, NodeId};

use crate::buffer::OtioBuffer;
use crate::handle::{OtioDocument, OtioNode, document_mut, optional_text, text, write_out};
use crate::node::node;
use crate::status::{Fault, OtioStatus, Outcome, guard};

/// A media linker or hook script.
///
/// See the module documentation, under "The callback", for what each
/// parameter holds and what the function must do.
pub type OtioPluginFn = Option<
    unsafe extern "C" fn(
        context: *mut c_void,
        document: *mut OtioDocument,
        target: OtioNode,
        arguments: OtioNode,
        out_result: *mut OtioNode,
        message: *mut c_char,
        message_capacity: usize,
    ) -> OtioStatus,
>;

/// Releases the context a plugin was registered with, once the library has
/// no more use for it.
pub type OtioPluginReleaseFn = Option<unsafe extern "C" fn(context: *mut c_void)>;

/// How much room a plugin is given for the sentence saying why it failed,
/// its terminating NUL included.
const MESSAGE_CAPACITY: usize = 1024;

/// A function a caller registered, the context it is called with, and how
/// that context is released.
struct Callback {
    function: unsafe extern "C" fn(
        *mut c_void,
        *mut OtioDocument,
        OtioNode,
        OtioNode,
        *mut OtioNode,
        *mut c_char,
        usize,
    ) -> OtioStatus,
    context: *mut c_void,
    release: OtioPluginReleaseFn,
}

// SAFETY: the registry is process-wide, so a callback may be called, and
// dropped, on any thread. The module documentation makes that the caller's
// promise about the function and its context.
unsafe impl Send for Callback {}
// SAFETY: as above; the callback holds nothing it mutates itself.
unsafe impl Sync for Callback {}

impl Drop for Callback {
    fn drop(&mut self) {
        if let Some(release) = self.release {
            // SAFETY: the caller handed this context over with this release
            // function, to be called once, here.
            unsafe { release(self.context) };
        }
    }
}

impl Callback {
    /// Calls the function on `target`, with `arguments` put where it can read
    /// them, and returns what it wrote to `out_result`.
    fn call(
        &self,
        document: &mut Document,
        target: NodeId,
        arguments: &AnyDictionary,
    ) -> Result<Option<NodeId>, String> {
        // The arguments are handed over as the metadata of an object of their
        // own, in the same document, since the objects among them live there.
        let mut holder = SerializableCollection::default();
        holder.base.metadata = arguments.clone();
        let holder = document.insert(Node::SerializableCollection(holder));
        let mut result = OtioNode::NONE;
        let mut message = [0 as c_char; MESSAGE_CAPACITY];
        // `OtioDocument` is a transparent wrapper, so a document is one.
        let pointer = std::ptr::from_mut(document).cast::<OtioDocument>();
        // SAFETY: the caller's promise about its function, which is handed a
        // live document it borrows for the call, and room for its answers.
        let status = unsafe {
            (self.function)(
                self.context,
                pointer,
                OtioNode::from_id(target),
                OtioNode::from_id(holder),
                &raw mut result,
                message.as_mut_ptr(),
                MESSAGE_CAPACITY,
            )
        };
        // Only the holder goes: what its metadata names belongs to whatever
        // else holds it.
        document.remove(holder);
        if status != OtioStatus::Ok {
            // The last byte is never read, so a message that filled the room
            // without a NUL still ends.
            let written = message[..MESSAGE_CAPACITY - 1]
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| *byte as u8)
                .collect::<Vec<u8>>();
            let written = String::from_utf8_lossy(&written).into_owned();
            return Err(if written.is_empty() {
                format!("it reported {}", status.name())
            } else {
                written
            });
        }
        if result == OtioNode::NONE {
            return Ok(None);
        }
        let result = result.to_id();
        if !document.contains(result) {
            return Err("it returned an object that is not in the document".to_owned());
        }
        Ok(Some(result))
    }
}

/// Checks a registration's function and wraps it with its context.
fn callback(
    function: OtioPluginFn,
    context: *mut c_void,
    release: OtioPluginReleaseFn,
) -> Outcome<Arc<Callback>> {
    let function = function.ok_or_else(|| Fault::null("function"))?;
    Ok(Arc::new(Callback {
        function,
        context,
        release,
    }))
}

/// Registers a media linker under `name`, replacing any registered already.
///
/// A read uses it when its options name it, or when the
/// `OTIO_DEFAULT_MEDIA_LINKER` environment variable does and the options
/// name none. `context` is handed to `function` on every call, and to
/// `release`, which may be null, once the linker is replaced or
/// unregistered. A call that fails registers nothing and releases nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_register_media_linker(
    name: *const c_char,
    function: OtioPluginFn,
    context: *mut c_void,
    release: OtioPluginReleaseFn,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let name = unsafe { text(name, "name") }?;
        if name.is_empty() {
            return Err(Fault::invalid("a media linker needs a name"));
        }
        let callback = callback(function, context, release)?;
        let replaced = plugins::registry().register_media_linker(
            name,
            MediaLinker::new(move |document, clip, arguments| {
                callback.call(document, clip, arguments)
            }),
        );
        // Dropped, and so released, once the registry is unlocked.
        drop(replaced);
        Ok(())
    })
}

/// Unregisters the media linker registered under `name`, releasing its
/// context. Returns whether there was one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_unregister_media_linker(name: *const c_char) -> bool {
    let Ok(name) = (unsafe { text(name, "name") }) else {
        return false;
    };
    // Taken out of the registry before it is dropped, so that a release
    // function that registers something does not find the registry locked.
    let removed = plugins::registry().take_media_linker(name);
    removed.is_some()
}

/// Registers a hook script under `name`, replacing any registered already.
///
/// It runs only at the hooks it is attached to, with
/// [`otio_attach_hook_script`]. `context` is handed to `function` on every
/// call, and to `release`, which may be null, once the script is replaced or
/// unregistered. A call that fails registers nothing and releases nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_register_hook_script(
    name: *const c_char,
    function: OtioPluginFn,
    context: *mut c_void,
    release: OtioPluginReleaseFn,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let name = unsafe { text(name, "name") }?;
        if name.is_empty() {
            return Err(Fault::invalid("a hook script needs a name"));
        }
        let callback = callback(function, context, release)?;
        let replaced = plugins::registry().register_hook_script(
            name,
            HookScript::new(move |document, target, arguments| {
                callback
                    .call(document, target, arguments)?
                    .ok_or_else(|| "it returned no object to go on with".to_owned())
            }),
        );
        drop(replaced);
        Ok(())
    })
}

/// Unregisters the hook script registered under `name`, releasing its
/// context. Returns whether there was one.
///
/// A hook it is still attached to fails when it runs, as upstream's does
/// for a script its manifest lists and cannot find; detach it as well.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_unregister_hook_script(name: *const c_char) -> bool {
    let Ok(name) = (unsafe { text(name, "name") }) else {
        return false;
    };
    let removed = plugins::registry().take_hook_script(name);
    removed.is_some()
}

/// Attaches the hook script `script` to the hook `hook`, after any attached
/// already, declaring the hook if it is new.
///
/// The four hooks every read and write runs are `post_adapter_read`,
/// `post_media_linker`, `pre_adapter_write` and `post_adapter_write`. Any
/// other name declares a hook of the caller's own, which
/// [`otio_node_run_hook`] runs. The script need not be registered yet, but
/// must be by the time the hook runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_attach_hook_script(
    hook: *const c_char,
    script: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let hook = unsafe { text(hook, "hook") }?;
        let script = unsafe { text(script, "script") }?;
        if hook.is_empty() || script.is_empty() {
            return Err(Fault::invalid("a hook and a script each need a name"));
        }
        plugins::registry().attach_hook_script(hook, script);
        Ok(())
    })
}

/// Detaches every attachment of the hook script `script` from the hook
/// `hook`. Returns whether it was attached.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_detach_hook_script(
    hook: *const c_char,
    script: *const c_char,
) -> bool {
    let (Ok(hook), Ok(script)) = (unsafe { text(hook, "hook") }, unsafe {
        text(script, "script")
    }) else {
        return false;
    };
    let mut registry = plugins::registry();
    let Some(attached) = registry.scripts_attached_to(hook) else {
        return false;
    };
    let kept: Vec<String> = attached
        .iter()
        .filter(|name| *name != script)
        .cloned()
        .collect();
    let detached = kept.len() != attached.len();
    registry.set_scripts_attached_to(hook, kept);
    detached
}

/// Runs every script attached to the hook `hook` on an object, each on what
/// the one before returned, and writes what the last returned to
/// `out_result`: upstream's `hooks.run`.
///
/// `arguments` is the scripts' argument map as a JSON object, or null for an
/// empty one. With no script attached, the object itself comes back. A hook
/// that was never declared, by an attachment or as one of the four every
/// read and write runs, is `OTIO_STATUS_PLUGIN_ERROR`, as is a script that
/// fails or is not registered.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_run_hook(
    document: *mut OtioDocument,
    node_handle: OtioNode,
    hook: *const c_char,
    arguments: *const c_char,
    out_result: *mut OtioNode,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let document = unsafe { document_mut(document) }?;
        node(document, node_handle)?;
        let hook = unsafe { text(hook, "hook") }?;
        let arguments = unsafe { optional_text(arguments, "arguments") }?;
        let arguments = argument_map(document, arguments, "arguments")?;
        let result = plugins::run_hook(hook, document, node_handle.to_id(), &arguments)?;
        unsafe { write_out(out_result, OtioNode::from_id(result), "out_result") }
    })
}

/// Reads an argument map given as a JSON object, moving any OTIO object in it
/// into `document`, where a plugin can reach it.
pub(crate) fn argument_map(
    document: &mut Document,
    json: Option<&str>,
    what: &str,
) -> Outcome<AnyDictionary> {
    let Some(json) = json.filter(|json| !json.trim().is_empty()) else {
        return Ok(AnyDictionary::new());
    };
    let (parsed, value) = otio_core::from_str_any(json)?;
    let Any::Dictionary(mut map) = value else {
        return Err(Fault::invalid(format!("{what} must be a JSON object")));
    };
    if !parsed.is_empty() {
        let translation = document.absorb(parsed);
        for value in map.values_mut() {
            value.visit_objects_mut(&mut |id| {
                if let Some(moved) = translation.get(id) {
                    *id = *moved;
                }
            });
        }
    }
    Ok(map)
}

/// What a read's options ask of the linker and hooks, before the arguments
/// are read into the document the read produced.
#[derive(Default)]
pub(crate) struct ReadPlugins {
    pub(crate) linker: LinkerChoice,
    pub(crate) linker_arguments: Option<String>,
    pub(crate) hook_arguments: Option<String>,
}

impl ReadPlugins {
    /// Runs `post_adapter_read`, the linker and `post_media_linker` on what a
    /// read produced, as upstream's `Adapter.read_from_file` does.
    pub(crate) fn run(&self, document: &mut Document) -> Outcome<()> {
        if !plugins::anything_to_run(&self.linker) {
            return Ok(());
        }
        let arguments = PluginArguments {
            media_linker: self.linker.clone(),
            media_linker_arguments: argument_map(
                document,
                self.linker_arguments.as_deref(),
                "media_linker_arguments",
            )?,
            hook_arguments: argument_map(
                document,
                self.hook_arguments.as_deref(),
                "hook_arguments",
            )?,
        };
        plugins::after_read(document, &arguments, AnyDictionary::new())?;
        Ok(())
    }
}

/// Runs `pre_adapter_write` on a copy of `source`, hands the copy, rooted at
/// what the hook returned, to `write`, and runs `post_adapter_write` on it,
/// as upstream's `Adapter.write_to_file` does.
///
/// With nothing attached to either hook, `source` is written as it is and
/// nothing is copied.
pub(crate) fn around_write<T>(
    source: &Document,
    hook_arguments: Option<&str>,
    path: Option<&str>,
    write: impl FnOnce(&Document) -> Outcome<T>,
) -> Outcome<T> {
    if !plugins::anything_to_run(&LinkerChoice::DoNotLink) {
        return write(source);
    }
    let Some(root) = source.root() else {
        return write(source);
    };
    let mut copy = source.clone();
    let arguments = argument_map(&mut copy, hook_arguments, "hook_arguments")?;
    let arguments = plugins::write_hook_arguments(
        &PluginArguments {
            hook_arguments: arguments,
            ..PluginArguments::default()
        },
        AnyDictionary::new(),
        path,
    );
    let target = plugins::before_write(&mut copy, root, &arguments)?;
    copy.set_root(Some(target));
    let written = write(&copy)?;
    plugins::after_write(&mut copy, target, &arguments)?;
    Ok(written)
}

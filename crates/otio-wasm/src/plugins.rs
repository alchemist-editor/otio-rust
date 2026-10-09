//! Media linkers and hook scripts written in JavaScript.
//!
//! The C ABI registers a plugin as a function pointer and a context, and a
//! WebAssembly module cannot be handed a JavaScript function as a pointer:
//! nothing outside the module has an address in it. What a module can do is
//! *import* a function from its host. So the host supplies one dispatcher,
//! `otio_js_plugin`, and one releaser, `otio_js_release`, under the import
//! module `otio_js`, and every plugin registered from JavaScript is the same
//! Rust function, `trampoline`, with a different context: an integer the
//! JavaScript side chose, which keys its own table of callbacks.
//!
//! ```text
//!  read ─► registry ─► trampoline(context, …) ─► import otio_js_plugin(context, …)
//!                                                   └─► callbacks.get(context)(clip, arguments)
//!  unregister ─► registry drops it ─► release(context) ─► import otio_js_release(context)
//! ```
//!
//! The handles cross as their two halves rather than as `OtioNode`, so the
//! import's signature is plain integers whatever the target's C ABI does with
//! a struct passed by value.

use std::ffi::{c_char, c_void};

use otio::{
    OtioBuffer, OtioDocument, OtioNode, OtioStatus, otio_register_hook_script,
    otio_register_media_linker,
};

/// What the JavaScript host provides.
#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "otio_js")]
    unsafe extern "C" {
        /// Calls the plugin `context` names: the C callback's parameters,
        /// with each handle split into its index and generation. Answers 0
        /// for success and anything else for a failure it described in
        /// `message`.
        pub fn otio_js_plugin(
            context: usize,
            document: usize,
            target_index: u32,
            target_generation: u32,
            arguments_index: u32,
            arguments_generation: u32,
            out_result: usize,
            message: usize,
            message_capacity: usize,
        ) -> i32;

        /// Forgets the plugin `context` names: the library has no more use
        /// for it.
        pub fn otio_js_release(context: usize);
    }
}

/// Stand-ins for the host, so the crate builds and its tests run natively,
/// where there is no JavaScript to call.
#[cfg(not(target_arch = "wasm32"))]
mod host {
    use std::sync::Mutex;

    /// The contexts released, oldest first, for the tests to look at.
    #[cfg_attr(not(test), allow(dead_code))]
    pub static RELEASED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    /// Fails every call: there is no JavaScript here to run.
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn otio_js_plugin(
        _context: usize,
        _document: usize,
        _target_index: u32,
        _target_generation: u32,
        _arguments_index: u32,
        _arguments_generation: u32,
        _out_result: usize,
        _message: usize,
        _message_capacity: usize,
    ) -> i32 {
        1
    }

    /// Records the release.
    pub unsafe fn otio_js_release(context: usize) {
        if let Ok(mut released) = RELEASED.lock() {
            released.push(context);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
use host::RELEASED;

/// The one function every JavaScript plugin is registered as.
///
/// # Safety
///
/// Called by the library, with the arguments its callback contract promises.
unsafe extern "C" fn trampoline(
    context: *mut c_void,
    document: *mut OtioDocument,
    target: OtioNode,
    arguments: OtioNode,
    out_result: *mut OtioNode,
    message: *mut c_char,
    message_capacity: usize,
) -> OtioStatus {
    // Safety: the host's dispatcher only reads and writes through the
    // addresses it is handed, within the room the library gave.
    let status = unsafe {
        host::otio_js_plugin(
            context.addr(),
            document.addr(),
            target.index,
            target.generation,
            arguments.index,
            arguments.generation,
            out_result.addr(),
            message.addr(),
            message_capacity,
        )
    };
    // Any status the host invents beyond success is a plugin's failure; it
    // is not trusted to name one of the library's own.
    if status == 0 {
        OtioStatus::Ok
    } else {
        OtioStatus::PluginError
    }
}

/// Tells the host the library has let go of a plugin.
///
/// # Safety
///
/// Called by the library, once per registration, with its context.
unsafe extern "C" fn release(context: *mut c_void) {
    // Safety: the host's releaser takes any integer.
    unsafe { host::otio_js_release(context.addr()) }
}

/// Registers a media linker written in JavaScript under `name`, replacing
/// any registered already: `otio_register_media_linker` with the host's
/// dispatcher as the function.
///
/// `context` is the host's key for the callback. It is handed back on every
/// call, and released through the host once the linker is replaced or
/// unregistered. A call that fails registers nothing and releases nothing.
///
/// # Safety
///
/// As `otio_register_media_linker`: `name` is a NUL-terminated string and
/// `out_error` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_wasm_register_media_linker(
    name: *const c_char,
    context: usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    // Safety: the caller's promise, passed on.
    unsafe {
        otio_register_media_linker(
            name,
            Some(trampoline),
            std::ptr::without_provenance_mut(context),
            Some(release),
            out_error,
        )
    }
}

/// Registers a hook script written in JavaScript under `name`, replacing any
/// registered already. As [`otio_wasm_register_media_linker`].
///
/// # Safety
///
/// As `otio_register_hook_script`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_wasm_register_hook_script(
    name: *const c_char,
    context: usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    // Safety: the caller's promise, passed on.
    unsafe {
        otio_register_hook_script(
            name,
            Some(trampoline),
            std::ptr::without_provenance_mut(context),
            Some(release),
            out_error,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{RELEASED, otio_wasm_register_hook_script, otio_wasm_register_media_linker};
    use otio::{OtioStatus, otio_unregister_hook_script, otio_unregister_media_linker};

    fn released(context: usize) -> usize {
        RELEASED
            .lock()
            .map(|all| all.iter().filter(|each| **each == context).count())
            .unwrap_or(0)
    }

    #[test]
    fn a_registration_is_released_through_the_host_once() {
        let name = c"wasm_test_linker";
        let status =
            unsafe { otio_wasm_register_media_linker(name.as_ptr(), 9001, std::ptr::null_mut()) };
        assert_eq!(status, OtioStatus::Ok);
        assert_eq!(released(9001), 0);
        assert!(unsafe { otio_unregister_media_linker(name.as_ptr()) });
        assert_eq!(released(9001), 1);
        assert!(!unsafe { otio_unregister_media_linker(name.as_ptr()) });
        assert_eq!(released(9001), 1);
    }

    #[test]
    fn replacing_a_script_releases_the_one_replaced() {
        let name = c"wasm_test_script";
        for context in [9101, 9102] {
            let status = unsafe {
                otio_wasm_register_hook_script(name.as_ptr(), context, std::ptr::null_mut())
            };
            assert_eq!(status, OtioStatus::Ok);
        }
        assert_eq!(released(9101), 1);
        assert_eq!(released(9102), 0);
        assert!(unsafe { otio_unregister_hook_script(name.as_ptr()) });
        assert_eq!(released(9102), 1);
    }

    #[test]
    fn a_registration_that_fails_releases_nothing() {
        let status =
            unsafe { otio_wasm_register_media_linker(c"".as_ptr(), 9201, std::ptr::null_mut()) };
        assert_ne!(status, OtioStatus::Ok);
        assert_eq!(released(9201), 0);
    }
}

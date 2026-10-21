//! Turning on extensions: work built in this repository that is not part of
//! OpenTimelineIO, such as timeline sync.
//!
//! Every extension compiled into this library is installed before any of
//! these calls answers, so a caller only ever enables one by its id. An
//! extension does nothing until it is enabled; see
//! `docs/adr/0010-extensions.md`.

use std::ffi::c_char;

use crate::buffer::OtioBuffer;
use crate::handle::{text, write_out};
use crate::status::{Fault, OtioStatus, Outcome, guard};

/// Installs every extension this library was built with. Cheap after the
/// first call.
pub(crate) fn install_builtins() {
    #[cfg(feature = "ext-sync")]
    otio_ext_sync::install();
    #[cfg(feature = "ext-ori")]
    otio_ext_ori_annotations::install();
}

/// Enables an extension by id, as `otio_ext_enable` does, for the
/// extensions' own entry points.
pub(crate) fn enable(id: &str) -> Outcome<()> {
    install_builtins();
    otio_ext::enable(id).map_err(|error| {
        let status = match error {
            otio_ext::Error::Register { .. } => OtioStatus::PluginError,
            _ => OtioStatus::InvalidArgument,
        };
        Fault::new(status, error.to_string())
    })
}

/// Turns on the extension `id`, and every extension it depends on first.
///
/// Enabling one that is already on does nothing and succeeds. An id this
/// library was not built with is `OTIO_STATUS_INVALID_ARGUMENT`, naming the
/// extensions it was built with; an extension that fails to register is
/// `OTIO_STATUS_PLUGIN_ERROR`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_enable(
    id: *const c_char,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let id = unsafe { text(id, "id") }?;
        enable(id)
    })
}

/// Writes whether the extension `id` is on to `out_enabled`.
///
/// An id this library was not built with is simply not on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_is_enabled(
    id: *const c_char,
    out_enabled: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let id = unsafe { text(id, "id") }?;
        unsafe { write_out(out_enabled, otio_ext::is_enabled(id), "out_enabled") }
    })
}

//! Timeline sync's C interface, `otio_ext_sync_*`, declared in
//! `include/otio_ext_sync.h`.
//!
//! **Timeline sync is an otio-rust extension, not part of OpenTimelineIO.**
//! This source is compiled into `libotio` as a module of `otio-capi`, behind
//! its `ext-sync` feature, so the extension shares the library's handles,
//! buffers and statuses. Nothing here does anything until sync is enabled.

use crate::buffer::OtioBuffer;
use crate::handle::write_out;
use crate::status::{OtioStatus, guard};

/// Turns timeline sync on. The same as `otio_ext_enable("sync")`.
///
/// Enabling it again does nothing and succeeds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_enable(out_error: *mut OtioBuffer) -> OtioStatus {
    guard(out_error, || crate::ext::enable(otio_ext_sync::ID))
}

/// Writes whether timeline sync is on to `out_enabled`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_ext_sync_is_enabled(
    out_enabled: *mut bool,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || unsafe {
        write_out(
            out_enabled,
            otio_ext::is_enabled(otio_ext_sync::ID),
            "out_enabled",
        )
    })
}

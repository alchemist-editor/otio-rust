//! Enabling extensions through the C ABI.

use std::ffi::CStr;
use std::ptr;

use otio::{
    OtioBuffer, OtioStatus, otio_buffer_free, otio_ext_enable, otio_ext_is_enabled,
    otio_ext_sync_enable, otio_ext_sync_is_enabled,
};

fn message(buffer: OtioBuffer) -> String {
    let text = if buffer.data.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(buffer.data.cast()) }
            .to_string_lossy()
            .into_owned()
    };
    unsafe { otio_buffer_free(buffer) };
    text
}

fn is_enabled(id: &CStr) -> bool {
    let mut enabled = true;
    let status = unsafe { otio_ext_is_enabled(id.as_ptr(), &mut enabled, ptr::null_mut()) };
    assert_eq!(status, OtioStatus::Ok);
    enabled
}

#[test]
fn sync_is_off_until_enabled_then_on_through_either_call() {
    let mut on = true;
    assert_eq!(
        unsafe { otio_ext_sync_is_enabled(&mut on, ptr::null_mut()) },
        OtioStatus::Ok
    );
    assert!(!on);
    assert!(!is_enabled(c"sync"));

    assert_eq!(
        unsafe { otio_ext_enable(c"sync".as_ptr(), ptr::null_mut()) },
        OtioStatus::Ok
    );
    assert_eq!(
        unsafe { otio_ext_sync_enable(ptr::null_mut()) },
        OtioStatus::Ok
    );
    assert!(is_enabled(c"sync"));
    assert_eq!(
        unsafe { otio_ext_sync_is_enabled(&mut on, ptr::null_mut()) },
        OtioStatus::Ok
    );
    assert!(on);
}

#[test]
fn an_unknown_extension_is_an_invalid_argument_naming_what_is_built_in() {
    let mut error = OtioBuffer {
        data: ptr::null_mut(),
        len: 0,
    };
    let status = unsafe { otio_ext_enable(c"no-such-thing".as_ptr(), &mut error) };
    assert_eq!(status, OtioStatus::InvalidArgument);
    let text = message(error);
    assert!(text.contains("'no-such-thing'"), "{text}");
    assert!(text.contains("sync"), "{text}");
    assert!(!is_enabled(c"no-such-thing"));
}

//! The options structs open with their own size, so that a caller built
//! against one header and a library built from another agree on how much of
//! the struct to read.
//!
//! These calls go through the struct as a C caller would hand it over: a
//! pointer to some bytes whose first `size_t` says how many there are.

use std::ffi::CString;
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioFormat, OtioReadOptions, OtioStatus, OtioWriteOptions,
    otio_buffer_free, otio_document_free, otio_document_from_json, otio_read_from_bytes,
    otio_read_options_default, otio_write_options_default, otio_write_to_bytes,
};

const TIMELINE: &str = r#"{
    "OTIO_SCHEMA": "Timeline.1",
    "name": "sized",
    "tracks": {"OTIO_SCHEMA": "Stack.1", "name": "tracks", "children": []}
}"#;

fn empty() -> OtioBuffer {
    OtioBuffer {
        data: ptr::null_mut(),
        len: 0,
    }
}

/// Reads the sample through `options` and returns the status and message.
fn read_with(options: *const OtioReadOptions) -> (OtioStatus, String) {
    let mut document: *mut OtioDocument = ptr::null_mut();
    let mut error = empty();
    let status = unsafe {
        otio_read_from_bytes(
            OtioFormat::OtioJson,
            TIMELINE.as_ptr(),
            TIMELINE.len(),
            options,
            &raw mut document,
            &raw mut error,
        )
    };
    unsafe { otio_document_free(document) };
    (status, take(error))
}

/// Writes the sample through `options` and returns the status and message.
fn write_with(options: *const OtioWriteOptions) -> (OtioStatus, String) {
    let json = CString::new(TIMELINE).expect("the sample has no NUL");
    let mut document: *mut OtioDocument = ptr::null_mut();
    let status =
        unsafe { otio_document_from_json(json.as_ptr(), &raw mut document, ptr::null_mut()) };
    assert_eq!(status, OtioStatus::Ok);
    let mut bytes = empty();
    let mut error = empty();
    let status = unsafe {
        otio_write_to_bytes(
            OtioFormat::OtioJson,
            document,
            options,
            &raw mut bytes,
            &raw mut error,
        )
    };
    unsafe {
        otio_buffer_free(bytes);
        otio_document_free(document);
    }
    (status, take(error))
}

fn take(buffer: OtioBuffer) -> String {
    let text = if buffer.data.is_null() {
        String::new()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(buffer.data.cast::<u8>(), buffer.len) };
        String::from_utf8_lossy(bytes).into_owned()
    };
    unsafe { otio_buffer_free(buffer) };
    text
}

#[test]
fn the_defaults_carry_the_size_of_the_struct() {
    assert_eq!(
        otio_read_options_default().struct_size,
        size_of::<OtioReadOptions>()
    );
    assert_eq!(
        otio_write_options_default().struct_size,
        size_of::<OtioWriteOptions>()
    );
    let read = otio_read_options_default();
    assert_eq!(read_with(&raw const read).0, OtioStatus::Ok);
    let write = otio_write_options_default();
    assert_eq!(write_with(&raw const write).0, OtioStatus::Ok);
}

#[test]
fn a_struct_with_no_size_is_refused_and_says_how_to_fix_it() {
    let read = OtioReadOptions {
        struct_size: 0,
        ..otio_read_options_default()
    };
    let (status, message) = read_with(&raw const read);
    assert_eq!(status, OtioStatus::InvalidArgument);
    assert!(message.contains("otio_read_options_default"), "{message}");

    let write = OtioWriteOptions {
        struct_size: 3,
        ..otio_write_options_default()
    };
    let (status, message) = write_with(&raw const write);
    assert_eq!(status, OtioStatus::InvalidArgument);
    assert!(message.contains("otio_write_options_default"), "{message}");
}

/// A struct from a newer header: this library's fields, then one it has
/// never heard of.
#[repr(C)]
struct Newer<T> {
    known: T,
    unknown: u64,
}

#[test]
fn a_newer_struct_is_read_when_it_asks_for_nothing_new() {
    let mut newer = Newer {
        known: otio_read_options_default(),
        unknown: 0,
    };
    newer.known.struct_size = size_of::<Newer<OtioReadOptions>>();
    assert_eq!(
        read_with((&raw const newer).cast::<OtioReadOptions>()).0,
        OtioStatus::Ok
    );

    // A non-zero field the library does not know is an option it would
    // silently ignore, so it says no instead.
    newer.unknown = 1;
    let (status, message) = read_with((&raw const newer).cast::<OtioReadOptions>());
    assert_eq!(status, OtioStatus::InvalidArgument);
    assert!(message.contains("newer otio.h"), "{message}");
}

#[test]
fn a_newer_write_struct_is_checked_the_same_way() {
    let mut newer = Newer {
        known: otio_write_options_default(),
        unknown: 7,
    };
    newer.known.struct_size = size_of::<Newer<OtioWriteOptions>>();
    assert_eq!(
        write_with((&raw const newer).cast::<OtioWriteOptions>()).0,
        OtioStatus::InvalidArgument
    );
    newer.unknown = 0;
    assert_eq!(
        write_with((&raw const newer).cast::<OtioWriteOptions>()).0,
        OtioStatus::Ok
    );
}

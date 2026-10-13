//! Asking an object, or a metadata value, for something only another kind
//! has reports `OTIO_STATUS_WRONG_KIND`, so a caller can tell a mistake in
//! its own code from a document that cannot answer.

use std::ffi::CString;
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioNode, OtioStatus, otio_buffer_free, otio_clip_new,
    otio_document_free, otio_document_new, otio_metadata_get_double, otio_metadata_get_string,
    otio_metadata_set_string, otio_metadata_set_vector, otio_node_child_count, otio_status_name,
    otio_track_kind,
};

fn empty() -> OtioBuffer {
    OtioBuffer {
        data: ptr::null_mut(),
        len: 0,
    }
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

/// Runs `call` with a fresh error buffer and returns its status and message.
fn outcome(call: impl FnOnce(*mut OtioBuffer) -> OtioStatus) -> (OtioStatus, String) {
    let mut error = empty();
    let status = call(&raw mut error);
    (status, take(error))
}

struct Fixture {
    document: *mut OtioDocument,
    clip: OtioNode,
}

impl Fixture {
    fn new() -> Self {
        let document = otio_document_new();
        let mut clip = OtioNode {
            index: 0,
            generation: 0,
        };
        let name = CString::new("A").unwrap();
        let status =
            unsafe { otio_clip_new(document, name.as_ptr(), &raw mut clip, ptr::null_mut()) };
        assert_eq!(status, OtioStatus::Ok);
        Self { document, clip }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { otio_document_free(self.document) };
    }
}

#[test]
fn a_clip_asked_for_what_a_track_has_says_it_is_the_wrong_kind() {
    let fixture = Fixture::new();

    let mut kind = empty();
    let (status, message) = outcome(|error| unsafe {
        otio_track_kind(fixture.document, fixture.clip, &raw mut kind, error)
    });
    assert_eq!(status, OtioStatus::WrongKind);
    assert!(message.contains("not a track"), "{message}");

    let mut count = 0;
    let (status, _) = outcome(|error| unsafe {
        otio_node_child_count(fixture.document, fixture.clip, &raw mut count, error)
    });
    assert_eq!(status, OtioStatus::WrongKind);
}

#[test]
fn metadata_of_the_wrong_type_says_it_is_the_wrong_kind() {
    let fixture = Fixture::new();
    let reel = CString::new("reel").unwrap();
    let value = CString::new("A001").unwrap();
    let status = unsafe {
        otio_metadata_set_string(
            fixture.document,
            fixture.clip,
            reel.as_ptr(),
            value.as_ptr(),
            ptr::null_mut(),
        )
    };
    assert_eq!(status, OtioStatus::Ok);

    let mut number = 0.0;
    let (status, message) = outcome(|error| unsafe {
        otio_metadata_get_double(
            fixture.document,
            fixture.clip,
            reel.as_ptr(),
            &raw mut number,
            error,
        )
    });
    assert_eq!(status, OtioStatus::WrongKind);
    assert!(message.contains("string"), "{message}");

    // A list stepped into by key.
    let list = CString::new("list").unwrap();
    let status = unsafe {
        otio_metadata_set_vector(
            fixture.document,
            fixture.clip,
            list.as_ptr(),
            1,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, OtioStatus::Ok);
    let by_key = CString::new("list.name").unwrap();
    let mut text = empty();
    let (status, message) = outcome(|error| unsafe {
        otio_metadata_get_string(
            fixture.document,
            fixture.clip,
            by_key.as_ptr(),
            &raw mut text,
            error,
        )
    });
    assert_eq!(status, OtioStatus::WrongKind);
    assert!(message.contains("has no keys"), "{message}");
}

#[test]
fn the_status_has_a_name() {
    let name = unsafe { std::ffi::CStr::from_ptr(otio_status_name(OtioStatus::WrongKind)) };
    assert_eq!(name.to_str(), Ok("OTIO_STATUS_WRONG_KIND"));
    assert_eq!(OtioStatus::WrongKind.name(), "OTIO_STATUS_WRONG_KIND");
}

//! The block of bytes the library hands back for anything of variable length.
//!
//! Every string and every file this library produces comes back as an
//! [`OtioBuffer`] that the caller frees with [`otio_buffer_free`]. Nothing is
//! ever returned as a borrowed pointer into a document, because the next call
//! that edits the document would invalidate it and C would not notice.

use std::ffi::c_char;

/// A block of bytes owned by the library.
///
/// `data` is always NUL-terminated, so a buffer holding text can be used as a
/// C string directly; `len` counts the bytes before the terminator, which is
/// what a caller holding binary data needs. A buffer with a null `data` holds
/// nothing and does not need freeing, though freeing it is harmless.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioBuffer {
    /// The bytes, NUL-terminated. Owned by the library.
    pub data: *mut c_char,
    /// How many bytes there are, not counting the terminator.
    pub len: usize,
}

impl OtioBuffer {
    /// Copies bytes into a freshly allocated buffer.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let mut owned = Vec::with_capacity(bytes.len() + 1);
        owned.extend_from_slice(bytes);
        owned.push(0);
        let len = bytes.len();
        // `into_boxed_slice` makes the capacity equal the length, which is
        // what `otio_buffer_free` relies on to rebuild the allocation.
        let boxed = owned.into_boxed_slice();
        let data = Box::into_raw(boxed).cast::<c_char>();
        Self { data, len }
    }

    /// Copies a string into a freshly allocated buffer.
    pub(crate) fn from_str(value: &str) -> Self {
        Self::from_bytes(value.as_bytes())
    }
}

/// Releases a buffer the library handed out.
///
/// Passing a buffer whose `data` is null does nothing. Passing the same
/// buffer twice, or one this library did not produce, is undefined.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_buffer_free(buffer: OtioBuffer) {
    if buffer.data.is_null() {
        return;
    }
    // The allocation is `len + 1` bytes: the contents and the terminator.
    let slice = std::ptr::slice_from_raw_parts_mut(buffer.data.cast::<u8>(), buffer.len + 1);
    drop(unsafe { Box::from_raw(slice) });
}

#[cfg(test)]
mod tests {
    use super::{OtioBuffer, otio_buffer_free};

    #[test]
    fn a_buffer_is_nul_terminated() {
        let buffer = OtioBuffer::from_str("hello");
        assert_eq!(buffer.len, 5);
        let bytes = unsafe { std::slice::from_raw_parts(buffer.data.cast::<u8>(), 6) };
        assert_eq!(bytes, b"hello\0");
        unsafe { otio_buffer_free(buffer) };
    }

    #[test]
    fn freeing_a_buffer_that_owns_nothing_is_harmless() {
        let empty = OtioBuffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
        unsafe { otio_buffer_free(empty) };
    }
}

//! The otio-rust C ABI, built for WebAssembly, plus the little that JavaScript
//! needs on top of it.
//!
//! # Why there is almost nothing here
//!
//! [`otio-capi`](../otio/index.html) already compiles to
//! `wasm32-unknown-unknown` unchanged. Every one of its entry points takes
//! integers, doubles and `#[repr(C)]` structs, which are exactly the things a
//! WebAssembly function signature can carry, and it pulls in no system
//! libraries, so the module it produces imports nothing at all. Building it
//! for the web is `rustup target add wasm32-unknown-unknown` and nothing else.
//!
//! So this crate re-exports that ABI rather than wrapping it, and adds the one
//! thing a C caller gets from its own libc and a JavaScript caller does not: a
//! way to allocate inside the module's linear memory. JavaScript cannot hand a
//! pointer to anything, because nothing outside the memory has an address. To
//! pass a string in, it has to write the bytes into the memory first, and to
//! do that it has to be given somewhere to write them.
//!
//! # What the TypeScript SDK is
//!
//! The SDK is generated, not written, and the generator lives in this crate as
//! the `otio-ts-gen` binary. It reads the signatures and doc comments out of
//! `otio-capi`'s source, merges them with a hand-written table saying what a C
//! signature cannot say — which parameter is a receiver, which is the result,
//! what may be absent, which three parameters are one list — and emits the
//! TypeScript in `ts/src/generated`. A test regenerates and compares, so the
//! checked-in SDK and the C ABI cannot drift apart without CI saying so.
//!
//! # Two things the wasm build does differently
//!
//! **Files.** `otio_read_from_file` and its neighbours compile, because
//! `std::fs` compiles for wasm, but they fail at runtime with
//! `OTIO_STATUS_IO_ERROR`: there is no filesystem behind them. The SDK does
//! not expose them. It reads and writes bytes, and leaves getting hold of the
//! bytes to the host, which is where that belongs on the web anyway.
//!
//! **Panics.** `wasm32-unknown-unknown` has no unwinding, so the
//! `catch_unwind` at the C ABI's boundary cannot catch anything: a panic traps
//! the instance instead of coming back as `OTIO_STATUS_PANIC`. A trapped
//! instance cannot be used again, so the SDK treats a trap as fatal, marks the
//! module poisoned and asks for a fresh one.

// Re-exported so the linker keeps them: a `cdylib` exports the `no_mangle`
// symbols of the crates linked into it, and naming the module here is what
// makes `otio-capi` one of them.
pub use otio::*;

use std::alloc::{Layout, alloc, dealloc};

/// The alignment every allocation this module hands out is made with.
///
/// Eight bytes covers every type that crosses the boundary: the widest thing
/// in the ABI is a `double`, and `OtioBuffer` is a 32-bit pointer beside a
/// 32-bit length on this target. A single alignment means the JavaScript side
/// has one number to remember rather than one per type.
const ALIGNMENT: usize = 8;

/// A pointer that is aligned and non-null but addresses nothing.
///
/// Rust's allocator will not take a zero-length request, and handing back a
/// null would be indistinguishable from failure, so a request for nothing gets
/// this. It is never dereferenced, because nothing reads zero bytes.
const EMPTY: *mut u8 = std::ptr::without_provenance_mut(ALIGNMENT);

/// Reserves `size` bytes in the module's linear memory.
///
/// Returns the offset to write to, or null if the memory could not grow. The
/// bytes are uninitialized. Release the block with [`otio_wasm_free`], passing
/// the same `size`.
///
/// This is how the JavaScript side passes anything that is not a number:
/// strings, byte arrays, the out-parameters the ABI writes results through,
/// and the arrays a two-pass list call fills.
#[unsafe(no_mangle)]
pub extern "C" fn otio_wasm_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        return EMPTY;
    }
    let Ok(layout) = Layout::from_size_align(size, ALIGNMENT) else {
        return std::ptr::null_mut();
    };
    // Safety: the layout has a non-zero size, which is the allocator's
    // requirement. A failed allocation returns null, which is this function's
    // answer for the same thing.
    unsafe { alloc(layout) }
}

/// Releases a block from [`otio_wasm_alloc`].
///
/// Passing null does nothing.
///
/// # Safety
///
/// `pointer` must have come from [`otio_wasm_alloc`] and must not have been
/// freed already, and `size` must be the size it was allocated with.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_wasm_free(pointer: *mut u8, size: usize) {
    if pointer.is_null() || size == 0 {
        return;
    }
    let Ok(layout) = Layout::from_size_align(size, ALIGNMENT) else {
        return;
    };
    // Safety: the contract above is that the pointer came from
    // `otio_wasm_alloc` with this size, so the layout matches the one it was
    // allocated with.
    unsafe { dealloc(pointer, layout) };
}

/// The alignment [`otio_wasm_alloc`] guarantees.
///
/// Exported so the JavaScript side reads it from the module rather than
/// repeating the constant and going quietly wrong if it ever changes.
#[unsafe(no_mangle)]
pub extern "C" fn otio_wasm_alignment() -> usize {
    ALIGNMENT
}

// The generator. It reads text files and writes text files, neither of which
// the WebAssembly module has any use for, so it is left out of that build
// rather than shipped to the browser as dead weight.
#[cfg(not(target_arch = "wasm32"))]
pub mod sdk;

#[cfg(test)]
mod tests {
    use super::{ALIGNMENT, otio_wasm_alignment, otio_wasm_alloc, otio_wasm_free};

    #[test]
    fn a_block_can_be_written_and_released() {
        let size = 64;
        let block = otio_wasm_alloc(size);
        assert!(!block.is_null());
        assert_eq!(block as usize % ALIGNMENT, 0);
        unsafe { block.write_bytes(0xAB, size) };
        assert_eq!(unsafe { block.read() }, 0xAB);
        unsafe { otio_wasm_free(block, size) };
    }

    #[test]
    fn a_request_for_nothing_is_not_a_failure() {
        let block = otio_wasm_alloc(0);
        assert!(!block.is_null());
        // Releasing it is a no-op rather than an error.
        unsafe { otio_wasm_free(block, 0) };
    }

    #[test]
    fn releasing_nothing_is_harmless() {
        unsafe { otio_wasm_free(std::ptr::null_mut(), 16) };
    }

    #[test]
    fn the_alignment_is_reported() {
        assert_eq!(otio_wasm_alignment(), ALIGNMENT);
    }
}

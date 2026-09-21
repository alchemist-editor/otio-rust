//! Documents, node handles, and the pointer checks every entry point starts
//! with.

use std::ffi::{CStr, c_char};

use otio_core::{Document, NodeId};

use crate::status::{Fault, Outcome};

/// An OTIO document: the arena that owns every object in a timeline.
///
/// A document is created by `otio_document_new` or
/// by reading a file, and released by
/// `otio_document_free`. Releasing it releases
/// every object in it, so handles into it go stale rather than dangling.
///
/// A document is not internally synchronized. Two threads may read one at
/// once; a thread that edits one must be the only thread touching it.
pub struct OtioDocument(pub(crate) Document);

/// A handle to an object in a document.
///
/// This is the `NodeId` of `otio-core` spelled for C: an index into the
/// document's arena and the generation of the slot it was issued for. If the
/// object is removed and the slot reused, the generation no longer matches, so
/// a stale handle fails a lookup instead of reaching the new occupant.
///
/// Handles are plain values. Copying one, storing it, and comparing two with
/// `index` and `generation` all work as they look.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtioNode {
    /// Which slot of the arena the object sits in.
    pub index: u32,
    /// Which occupant of that slot the handle was issued for.
    pub generation: u32,
}

impl OtioNode {
    /// The handle no object ever has, for saying "nothing" in a struct field.
    pub(crate) const NONE: Self = Self {
        index: u32::MAX,
        generation: u32::MAX,
    };

    pub(crate) const fn to_id(self) -> NodeId {
        NodeId::from_raw(self.index, self.generation)
    }

    pub(crate) const fn from_id(id: NodeId) -> Self {
        let (index, generation) = id.to_raw();
        Self { index, generation }
    }
}

/// Returns the handle that names no object.
///
/// Fields that may be absent, such as a timeline's tracks before one is set,
/// report this. Compare against it with
/// `otio_node_is_none`.
#[unsafe(no_mangle)]
pub extern "C" fn otio_node_none() -> OtioNode {
    OtioNode::NONE
}

/// Returns whether a handle names no object.
#[unsafe(no_mangle)]
pub extern "C" fn otio_node_is_none(node: OtioNode) -> bool {
    node == OtioNode::NONE
}

/// Returns whether two handles name the same object.
///
/// This is object identity: two handles are equal only if they were issued for
/// the same occupant of the same slot.
#[unsafe(no_mangle)]
pub extern "C" fn otio_node_equal(left: OtioNode, right: OtioNode) -> bool {
    left == right
}

/// Borrows a document for reading.
pub(crate) unsafe fn document<'a>(pointer: *const OtioDocument) -> Outcome<&'a Document> {
    if pointer.is_null() {
        return Err(Fault::null("document"));
    }
    Ok(&unsafe { &*pointer }.0)
}

/// Borrows a document for writing.
pub(crate) unsafe fn document_mut<'a>(pointer: *mut OtioDocument) -> Outcome<&'a mut Document> {
    if pointer.is_null() {
        return Err(Fault::null("document"));
    }
    Ok(&mut unsafe { &mut *pointer }.0)
}

/// Writes a value through an out-parameter.
pub(crate) unsafe fn write_out<T>(pointer: *mut T, value: T, what: &str) -> Outcome<()> {
    if pointer.is_null() {
        return Err(Fault::null(what));
    }
    unsafe { pointer.write(value) };
    Ok(())
}

/// Reads a C string argument as UTF-8.
pub(crate) unsafe fn text<'a>(pointer: *const c_char, what: &str) -> Outcome<&'a str> {
    if pointer.is_null() {
        return Err(Fault::null(what));
    }
    unsafe { CStr::from_ptr(pointer) }.to_str().map_err(|_| {
        Fault::new(
            crate::OtioStatus::InvalidUtf8,
            format!("{what} is not UTF-8"),
        )
    })
}

/// Reads an optional C string argument: a null pointer means "not given".
pub(crate) unsafe fn optional_text<'a>(
    pointer: *const c_char,
    what: &str,
) -> Outcome<Option<&'a str>> {
    if pointer.is_null() {
        return Ok(None);
    }
    unsafe { text(pointer, what) }.map(Some)
}

/// Turns a handle that may be [`OtioNode::NONE`] into an optional node.
pub(crate) const fn optional_node(node: OtioNode) -> Option<NodeId> {
    if node.index == u32::MAX && node.generation == u32::MAX {
        None
    } else {
        Some(node.to_id())
    }
}

/// Reads a borrowed slice of bytes from a pointer and a length.
pub(crate) unsafe fn bytes<'a>(pointer: *const u8, len: usize, what: &str) -> Outcome<&'a [u8]> {
    if pointer.is_null() {
        if len == 0 {
            return Ok(&[]);
        }
        return Err(Fault::null(what));
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, len) })
}

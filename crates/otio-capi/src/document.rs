//! Creating documents, reading and writing OTIO JSON, and the arena itself.

use std::ffi::c_char;

use otio_core::Document;

use crate::buffer::OtioBuffer;
use crate::handle::{
    OtioDocument, OtioNode, document, document_mut, optional_node, text, write_out,
};
use crate::status::{Fault, OtioStatus, guard, guard_value};

/// Creates an empty document with no root.
///
/// Returns null only if the allocation fails. Release it with
/// [`otio_document_free`].
#[unsafe(no_mangle)]
pub extern "C" fn otio_document_new() -> *mut OtioDocument {
    guard_value(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(OtioDocument(Document::new())))
    })
}

/// Releases a document and every object in it.
///
/// Passing null does nothing. Every handle into the document is stale
/// afterwards; using one is a programming error this library cannot detect,
/// because the arena it would be checked against is gone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_free(document: *mut OtioDocument) {
    if document.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(document) });
}

/// Copies a document, objects and all.
///
/// Handles into the original name the same objects in the copy, because the
/// copy keeps the arena's layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_clone(
    source: *const OtioDocument,
    out_document: *mut *mut OtioDocument,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let copy = Box::into_raw(Box::new(OtioDocument(source.clone())));
        unsafe { write_out(out_document, copy, "out_document") }
    })
}

/// Reads a document from OTIO JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_from_json(
    json: *const c_char,
    out_document: *mut *mut OtioDocument,
) -> OtioStatus {
    guard(|| {
        let json = unsafe { text(json, "json") }?;
        let parsed = otio_core::from_str(json)?;
        let owned = Box::into_raw(Box::new(OtioDocument(parsed)));
        unsafe { write_out(out_document, owned, "out_document") }
    })
}

/// Reads a document from a `.otio` file on disk.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_read_from_file(
    path: *const c_char,
    out_document: *mut *mut OtioDocument,
) -> OtioStatus {
    guard(|| {
        let path = unsafe { text(path, "path") }?;
        let contents = std::fs::read_to_string(path)
            .map_err(|error| Fault::new(OtioStatus::IoError, error.to_string()))?;
        let parsed = otio_core::from_str(&contents)?;
        let owned = Box::into_raw(Box::new(OtioDocument(parsed)));
        unsafe { write_out(out_document, owned, "out_document") }
    })
}

/// Writes a document as OTIO JSON, starting from its root.
///
/// `indent` is how many spaces each level is indented by;
/// [`otio_default_indent`] is what upstream's Python bindings use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_to_json(
    source: *const OtioDocument,
    indent: usize,
    out_json: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let json = otio_core::to_string_pretty(source, indent)?;
        unsafe { write_out(out_json, OtioBuffer::from_str(&json), "out_json") }
    })
}

/// Writes one object of a document as OTIO JSON.
///
/// Upstream's `write_to_string` takes any object, not only a timeline, so this
/// does too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_to_json(
    source: *const OtioDocument,
    node: OtioNode,
    indent: usize,
    out_json: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let json = otio_core::to_string_pretty_from(source, node.to_id(), indent)?;
        unsafe { write_out(out_json, OtioBuffer::from_str(&json), "out_json") }
    })
}

/// Writes a document to a `.otio` file on disk.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_write_to_file(
    source: *const OtioDocument,
    path: *const c_char,
    indent: usize,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let path = unsafe { text(path, "path") }?;
        let json = otio_core::to_string_pretty(source, indent)?;
        std::fs::write(path, json)
            .map_err(|error| Fault::new(OtioStatus::IoError, error.to_string()))
    })
}

/// Returns the indentation upstream's Python bindings write by default.
#[unsafe(no_mangle)]
pub extern "C" fn otio_default_indent() -> usize {
    otio_core::DEFAULT_INDENT
}

/// Returns the document's root object.
///
/// Reports `OTIO_STATUS_NO_VALUE` for a document that has none, which is what
/// a freshly created one is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_root(
    source: *const OtioDocument,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let root = source.root().ok_or_else(|| Fault::no_value("the root"))?;
        unsafe { write_out(out_node, OtioNode::from_id(root), "out_node") }
    })
}

/// Sets the document's root object.
///
/// Passing `otio_node_none` clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_set_root(
    target: *mut OtioDocument,
    node: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let root = optional_node(node);
        if root.is_some_and(|root| !target.contains(root)) {
            return Err(Fault::from(otio_core::Error::StaleHandle));
        }
        target.set_root(root);
        Ok(())
    })
}

/// Returns how many live objects the document holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_node_count(source: *const OtioDocument) -> usize {
    guard_value(0, || {
        unsafe { document(source) }.map_or(0, otio_core::Document::len)
    })
}

/// Returns whether a handle still names a live object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_contains(
    source: *const OtioDocument,
    node: OtioNode,
) -> bool {
    guard_value(false, || {
        unsafe { document(source) }.is_ok_and(|source| source.contains(node.to_id()))
    })
}

/// Removes one object from the document.
///
/// Anything that referred to it still holds a handle, and that handle is now
/// stale: a lookup fails rather than reaching whatever takes the slot next. To
/// remove an object together with everything hanging off it, use
/// [`otio_document_remove_recursive`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_remove(
    target: *mut OtioDocument,
    node: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        target
            .remove(node.to_id())
            .map(|_| ())
            .ok_or_else(|| Fault::from(otio_core::Error::StaleHandle))
    })
}

/// Removes an object and everything it owns: children, markers, effects and
/// media references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_remove_recursive(
    target: *mut OtioDocument,
    node: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        target.remove_recursive(node.to_id())?;
        Ok(())
    })
}

/// Copies an object and everything it owns, into the same document.
///
/// The copy has no parent, whatever the original had.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_deep_clone(
    target: *mut OtioDocument,
    node: OtioNode,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let copy = target.deep_clone(node.to_id())?;
        unsafe { write_out(out_node, OtioNode::from_id(copy), "out_node") }
    })
}

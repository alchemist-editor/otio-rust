//! Creating documents, reading and writing OTIO JSON, and the arena itself.

use std::ffi::c_char;

use otio_core::{Document, NodeId};

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

/// Moves every object out of one document into another.
///
/// This is the call that lets a binding offer the API OpenTimelineIO's own
/// Python and C++ users expect, where a `Clip` is built on its own and put
/// inside a `Track` afterwards. An arena has no way to do that with handles
/// alone: a handle means nothing outside the document it was issued for, so an
/// object built in one document has to be *moved* into the other rather than
/// pointed at. Upstream's reference-counted object model gets this for free
/// and pays for it elsewhere; see `docs/adr/0001-ownership-model.md`.
///
/// `source` is consumed. On success it is released and the caller's pointer is
/// set to null, so there is nothing left to free and no way to free it twice.
/// On failure nothing moves and the pointer is left alone.
///
/// The source's root is not adopted, because the target has its own.
///
/// # The translation
///
/// Every object arrives under a new handle, so every handle the caller still
/// holds into `source` has to be translated. The call fills `out_from` and
/// `out_to` with the old handle and the new one for each object moved, in the
/// same order, and writes how many pairs there are to `out_count`.
///
/// Unlike the other calls that answer with a list, this one cannot be asked
/// twice to size the buffer: the first call would already have consumed the
/// source. Size the arrays with
/// [`otio_document_node_count`] on the source
/// before calling, which is exactly how many objects will move. A capacity
/// smaller than that is `OTIO_STATUS_INVALID_ARGUMENT`, and nothing moves.
///
/// # Example
///
/// ```c
/// size_t moving = otio_document_node_count(clip_document);
/// OtioNode *from = malloc(moving * sizeof(OtioNode));
/// OtioNode *to = malloc(moving * sizeof(OtioNode));
/// size_t moved = 0;
/// otio_document_absorb(timeline_document, &clip_document, from, to, moving, &moved, std::ptr::null_mut());
/// /* `clip_document` is now NULL; `clip` is `to[i]` where `from[i]` is `clip`. */
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_absorb(
    target: *mut OtioDocument,
    source: *mut *mut OtioDocument,
    out_from: *mut OtioNode,
    out_to: *mut OtioNode,
    capacity: usize,
    out_count: *mut usize,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        if source.is_null() {
            return Err(Fault::null("source"));
        }
        let taken = unsafe { *source };
        if taken.is_null() {
            return Err(Fault::null("the document source points at"));
        }
        if std::ptr::eq(taken.cast_const(), target.cast_const()) {
            return Err(Fault::invalid("a document cannot absorb itself"));
        }
        if out_count.is_null() {
            return Err(Fault::null("out_count"));
        }

        // Everything that can fail is checked before the source is consumed,
        // so a failed call leaves both documents exactly as they were.
        let moving = unsafe { &*taken }.0.len();
        if capacity < moving {
            return Err(Fault::invalid(format!(
                "moving {moving} objects needs a capacity of at least {moving}, not {capacity}"
            )));
        }
        if moving > 0 && (out_from.is_null() || out_to.is_null()) {
            return Err(Fault::null("out_from and out_to"));
        }
        let target = unsafe { document_mut(target) }?;

        let owned = unsafe { Box::from_raw(taken) };
        unsafe { *source = std::ptr::null_mut() };

        let mut pairs: Vec<(NodeId, NodeId)> = target.absorb(owned.0).into_iter().collect();
        // A hash map hands its entries back in whatever order it likes, and a
        // caller comparing two runs should not see them differ.
        pairs.sort_by_key(|(old, _)| old.to_raw());

        for (index, (old, new)) in pairs.iter().enumerate() {
            unsafe {
                out_from.add(index).write(OtioNode::from_id(*old));
                out_to.add(index).write(OtioNode::from_id(*new));
            }
        }
        unsafe { write_out(out_count, pairs.len(), "out_count") }
    })
}

/// Copies a document, objects and all.
///
/// Handles into the original name the same objects in the copy, because the
/// copy keeps the arena's layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_document_clone(
    source: *const OtioDocument,
    out_document: *mut *mut OtioDocument,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
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
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let target = unsafe { document_mut(target) }?;
        let copy = target.deep_clone(node.to_id())?;
        unsafe { write_out(out_node, OtioNode::from_id(copy), "out_node") }
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use super::{
        OtioDocument, OtioNode, otio_document_absorb, otio_document_free, otio_document_new,
        otio_document_node_count,
    };
    use crate::composition::otio_composition_append_child;
    use crate::node::{otio_clip_new, otio_node_name, otio_node_parent, otio_track_new};
    use crate::status::OtioStatus;
    use crate::{OtioBuffer, otio_buffer_free};

    /// Builds a document holding one clip, and returns both.
    fn clip_in_its_own_document(name: &str) -> (*mut OtioDocument, OtioNode) {
        let document = otio_document_new();
        let name = CString::new(name).expect("a test name has no NUL in it");
        let mut clip = OtioNode::NONE;
        assert_eq!(
            unsafe { otio_clip_new(document, name.as_ptr(), &raw mut clip, std::ptr::null_mut()) },
            OtioStatus::Ok
        );
        (document, clip)
    }

    /// Reads an object's name.
    fn name_of(document: *const OtioDocument, node: OtioNode) -> String {
        let mut buffer = OtioBuffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            unsafe { otio_node_name(document, node, &raw mut buffer, std::ptr::null_mut()) },
            OtioStatus::Ok
        );
        let text = unsafe { std::slice::from_raw_parts(buffer.data.cast::<u8>(), buffer.len) };
        let text = String::from_utf8(text.to_vec()).expect("a name is UTF-8");
        unsafe { otio_buffer_free(buffer) };
        text
    }

    /// Absorbs one document into another and returns the translation.
    fn absorb(
        target: *mut OtioDocument,
        source: &mut *mut OtioDocument,
    ) -> Vec<(OtioNode, OtioNode)> {
        let moving = unsafe { otio_document_node_count(*source) };
        let mut from = vec![OtioNode::NONE; moving];
        let mut to = vec![OtioNode::NONE; moving];
        let mut count = 0usize;
        assert_eq!(
            unsafe {
                otio_document_absorb(
                    target,
                    source,
                    from.as_mut_ptr(),
                    to.as_mut_ptr(),
                    moving,
                    &raw mut count,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::Ok
        );
        assert_eq!(count, moving);
        from.into_iter().zip(to).collect()
    }

    #[test]
    fn an_object_built_on_its_own_can_be_put_inside_another() {
        let timeline = otio_document_new();
        let track_name = CString::new("V1").expect("a literal has no NUL in it");
        let mut track = OtioNode::NONE;
        assert_eq!(
            unsafe {
                otio_track_new(
                    timeline,
                    track_name.as_ptr(),
                    std::ptr::null(),
                    &raw mut track,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::Ok
        );

        let (mut clip_document, clip) = clip_in_its_own_document("shot_01");
        let translation = absorb(timeline, &mut clip_document);

        // The source is gone, and the caller's pointer says so.
        assert!(clip_document.is_null());

        let (_, moved) = translation
            .iter()
            .copied()
            .find(|(old, _)| *old == clip)
            .expect("the clip is one of the objects that moved");
        assert_ne!(moved, clip, "a moved object gets a handle in its new home");
        assert_eq!(name_of(timeline, moved), "shot_01");

        // And it behaves like any other object in the document it arrived in.
        assert_eq!(
            unsafe { otio_composition_append_child(timeline, track, moved, std::ptr::null_mut()) },
            OtioStatus::Ok
        );
        let mut parent = OtioNode::NONE;
        assert_eq!(
            unsafe { otio_node_parent(timeline, moved, &raw mut parent, std::ptr::null_mut()) },
            OtioStatus::Ok
        );
        assert_eq!(parent, track);

        unsafe { otio_document_free(timeline) };
    }

    #[test]
    fn the_links_between_moved_objects_survive_the_move() {
        // A track with a clip in it, built away from the timeline it will join.
        let scratch = otio_document_new();
        let name = CString::new("V2").expect("a literal has no NUL in it");
        let mut track = OtioNode::NONE;
        assert_eq!(
            unsafe {
                otio_track_new(
                    scratch,
                    name.as_ptr(),
                    std::ptr::null(),
                    &raw mut track,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::Ok
        );
        let clip_name = CString::new("shot_02").expect("a literal has no NUL in it");
        let mut clip = OtioNode::NONE;
        assert_eq!(
            unsafe {
                otio_clip_new(
                    scratch,
                    clip_name.as_ptr(),
                    &raw mut clip,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::Ok
        );
        assert_eq!(
            unsafe { otio_composition_append_child(scratch, track, clip, std::ptr::null_mut()) },
            OtioStatus::Ok
        );

        let timeline = otio_document_new();
        let mut source = scratch;
        let translation = absorb(timeline, &mut source);
        let translated = |old: OtioNode| {
            translation
                .iter()
                .copied()
                .find(|(from, _)| *from == old)
                .expect("every object that moved is in the translation")
                .1
        };

        let mut parent = OtioNode::NONE;
        assert_eq!(
            unsafe {
                otio_node_parent(
                    timeline,
                    translated(clip),
                    &raw mut parent,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::Ok
        );
        assert_eq!(
            parent,
            translated(track),
            "the clip still belongs to its track, under the track's new handle"
        );

        unsafe { otio_document_free(timeline) };
    }

    #[test]
    fn too_small_a_capacity_moves_nothing() {
        let target = otio_document_new();
        let (document, _) = clip_in_its_own_document("shot_03");
        let mut source = document;
        let mut count = 0usize;
        let mut from = [OtioNode::NONE; 1];
        let mut to = [OtioNode::NONE; 1];

        assert_eq!(
            unsafe {
                otio_document_absorb(
                    target,
                    &raw mut source,
                    from.as_mut_ptr(),
                    to.as_mut_ptr(),
                    0,
                    &raw mut count,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::InvalidArgument
        );
        assert_eq!(source, document, "a failed call leaves the source alone");
        assert_eq!(unsafe { otio_document_node_count(source) }, 1);
        assert_eq!(unsafe { otio_document_node_count(target) }, 0);

        unsafe { otio_document_free(source) };
        unsafe { otio_document_free(target) };
    }

    #[test]
    fn a_document_cannot_absorb_itself() {
        let document = otio_document_new();
        let mut source = document;
        let mut count = 0usize;
        assert_eq!(
            unsafe {
                otio_document_absorb(
                    document,
                    &raw mut source,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    &raw mut count,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::InvalidArgument
        );
        assert_eq!(source, document);
        unsafe { otio_document_free(document) };
    }

    #[test]
    fn a_null_source_is_reported_rather_than_dereferenced() {
        let target = otio_document_new();
        let mut count = 0usize;
        let mut empty: *mut OtioDocument = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                otio_document_absorb(
                    target,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    &raw mut count,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::NullPointer
        );
        assert_eq!(
            unsafe {
                otio_document_absorb(
                    target,
                    &raw mut empty,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    &raw mut count,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::NullPointer
        );
        unsafe { otio_document_free(target) };
    }

    #[test]
    fn absorbing_an_empty_document_is_allowed_and_moves_nothing() {
        let target = otio_document_new();
        let empty = otio_document_new();
        let mut source = empty;
        let mut count = 1usize;
        assert_eq!(
            unsafe {
                otio_document_absorb(
                    target,
                    &raw mut source,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    &raw mut count,
                    std::ptr::null_mut(),
                )
            },
            OtioStatus::Ok
        );
        assert_eq!(count, 0);
        assert!(source.is_null());
        unsafe { otio_document_free(target) };
    }
}

//! Copying an object the way upstream's `SerializableObject::clone` does.
//!
//! [`Document::deep_clone`] copies an object held in two places once, so the
//! copy shares what the original shared, and follows a cycle without
//! complaint. Upstream's `clone` does neither: it copies by writing the
//! object out and reading it back, so an object held twice is copied twice,
//! and an object that holds itself is refused as a cycle.
//!
//! Upstream's writer could keep the sharing — it has an `OTIO_REF_ID` field
//! for an object met a second time — but only when built with
//! `OTIO_INSTANCING_SUPPORT`, which nothing in its build defines. Without it
//! the writer forgets an object once it has written it, so a second holder
//! writes it out again, and only an object met while it is still being
//! written counts as a cycle. Run against an upstream build, `clone()`,
//! Python's `clone`, `deepcopy` and `copy.deepcopy`, the edits that copy
//! (`slice`, `insert`, `overwrite`, `fill`) and the algorithms that copy
//! (`flatten_stack`, `track_trimmed_to_range`) all come back with two objects
//! where the original had one held twice: two metadata keys, one effect
//! listed twice, one media reference under two keys, or one object held by
//! two clips of a copied track.
//!
//! So the copy is offered here in two forms. [`Document::clone_object`] is
//! upstream's `clone` exactly, cycles refused; the Python bindings' copies and
//! the algorithms use it. The edits use a form that copies the same way but
//! follows a cycle, pointing a link back at an object still being copied at
//! the copy of that object; see [`crate::edit`] for why.

use std::collections::HashSet;

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};

/// What a copy does on meeting an object it is still in the middle of
/// copying.
#[derive(Clone, Copy)]
enum Cycles {
    /// Fails with [`Error::ObjectCycle`], as upstream's writer does.
    Refuse,
    /// Links to that object's copy, so the copy has the cycle the original
    /// had.
    Keep,
}

impl Document {
    /// Copies an object and everything it owns, as upstream's `clone` does.
    ///
    /// The copy has no parent. An object held in two places — the same
    /// object under two metadata keys, say — becomes two objects in the copy,
    /// so nothing in the copy is held twice. That is what separates this from
    /// [`Document::deep_clone`], which keeps the sharing.
    ///
    /// # Errors
    ///
    /// [`Error::ObjectCycle`] if the object holds itself somewhere below it,
    /// and [`Error::StaleHandle`] if a handle in it is stale. Nothing is left
    /// behind in the document on failure.
    pub fn clone_object(&mut self, id: NodeId) -> Result<NodeId> {
        self.clone_tree(id, Cycles::Refuse)
    }

    /// [`Document::clone_object`], except that a cycle is copied rather than
    /// refused: a link back to an object the copy is inside points at that
    /// object's copy.
    ///
    /// An object held twice is still copied twice, as upstream copies it;
    /// only the cycle, which upstream cannot copy at all, is kept.
    pub(crate) fn clone_object_keeping_cycles(&mut self, id: NodeId) -> Result<NodeId> {
        self.clone_tree(id, Cycles::Keep)
    }

    /// Drops a copy made by [`Document::clone_object`] and everything it
    /// owns, including the objects its metadata holds.
    ///
    /// [`Document::remove_recursive`] leaves what metadata holds alone,
    /// because in general something else may hold it too. In a copy made by
    /// `clone_object` nothing is held twice and nothing outside holds any of
    /// it, so a scratch copy can be dropped whole.
    pub(crate) fn remove_clone(&mut self, id: NodeId) {
        let mut pending = vec![id];
        let mut seen = HashSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = self.remove(id) {
                node.visit_owned(&mut |owned| pending.push(owned));
            }
        }
    }

    /// Copies `id` with `cycles` deciding what a link back into the copy
    /// does.
    fn clone_tree(&mut self, id: NodeId, cycles: Cycles) -> Result<NodeId> {
        let mut path = Vec::new();
        let mut made = Vec::new();
        let result = self.clone_along(id, &mut path, &mut made, cycles);
        if result.is_err() {
            for copy in made {
                self.remove(copy);
            }
        }
        result
    }

    /// Copies `id`, with `path` pairing each object being copied around it
    /// with its copy.
    ///
    /// An object leaves `path` once its copy is finished, so a second holder
    /// met later copies it again, as upstream's writer, which forgets an
    /// object once written, writes it again.
    fn clone_along(
        &mut self,
        id: NodeId,
        path: &mut Vec<(NodeId, NodeId)>,
        made: &mut Vec<NodeId>,
        cycles: Cycles,
    ) -> Result<NodeId> {
        let original = self.try_get(id)?;
        if let Some(&(_, copy)) = path.iter().find(|(on_path, _)| *on_path == id) {
            return match cycles {
                Cycles::Refuse => Err(Error::ObjectCycle {
                    schema: original.schema_name().to_string(),
                }),
                Cycles::Keep => Ok(copy),
            };
        }

        // With the parent cleared, the links left are the ones the object
        // owns: its children, effects, markers, media, tracks and whatever
        // its free-form fields hold.
        let mut node = original.clone();
        node.set_parent(None);
        let mut links = Vec::new();
        node.visit_links_mut(&mut |link| links.push(*link));

        // The copy takes its handle before its links are followed, so that a
        // link leading back to it has something to point at. Until they are
        // rewritten below, its links are still the original's.
        let copy = self.insert(node);
        made.push(copy);

        path.push((id, copy));
        let mut copies = Vec::with_capacity(links.len());
        for link in links {
            match self.clone_along(link, path, made, cycles) {
                Ok(copied) => copies.push(copied),
                Err(error) => {
                    path.pop();
                    return Err(error);
                }
            }
        }
        path.pop();

        let node = self.try_get_mut(copy)?;
        let mut copies_iter = copies.into_iter();
        node.visit_links_mut(&mut |link| {
            if let Some(copied) = copies_iter.next() {
                *link = copied;
            }
        });
        let children = node.children().map(<[NodeId]>::to_vec).unwrap_or_default();
        for child in children {
            if let Some(child) = self.get_mut(child) {
                child.set_parent(Some(copy));
            }
        }
        Ok(copy)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::{Base, Node};
    use crate::{Any, Document, Error};

    fn with_metadata(document: &mut Document) -> crate::NodeId {
        document.insert(Node::SerializableObjectWithMetadata(Base::default()))
    }

    fn metadata(document: &mut Document, id: crate::NodeId) -> &mut crate::AnyDictionary {
        &mut document
            .get_mut(id)
            .and_then(Node::base_mut)
            .expect("an object with metadata")
            .metadata
    }

    #[test]
    fn an_object_held_twice_is_copied_twice() {
        let mut document = Document::new();
        let outer = with_metadata(&mut document);
        let inner = with_metadata(&mut document);
        metadata(&mut document, outer).insert("a".into(), Any::Object(inner));
        metadata(&mut document, outer).insert("b".into(), Any::Object(inner));

        let copy = document.clone_object(outer).expect("no cycle");
        let copied = metadata(&mut document, copy).clone();
        let (Any::Object(a), Any::Object(b)) = (&copied["a"], &copied["b"]) else {
            panic!("both entries hold objects");
        };
        assert_ne!(a, b);
        assert_ne!(*a, inner);
    }

    #[test]
    fn an_object_holding_itself_is_refused_and_leaves_nothing_behind() {
        let mut document = Document::new();
        let outer = with_metadata(&mut document);
        let inner = with_metadata(&mut document);
        metadata(&mut document, outer).insert("child".into(), Any::Object(inner));
        metadata(&mut document, inner).insert("up".into(), Any::Object(outer));
        let before = document.len();

        assert_eq!(
            document.clone_object(outer),
            Err(Error::ObjectCycle {
                schema: "SerializableObjectWithMetadata".to_string()
            })
        );
        assert_eq!(document.len(), before);
    }

    #[test]
    fn keeping_cycles_copies_a_cycle_but_still_copies_a_twice_held_object_twice() {
        let mut document = Document::new();
        let outer = with_metadata(&mut document);
        let inner = with_metadata(&mut document);
        metadata(&mut document, outer).insert("a".into(), Any::Object(inner));
        metadata(&mut document, outer).insert("b".into(), Any::Object(inner));
        metadata(&mut document, inner).insert("up".into(), Any::Object(outer));

        let copy = document
            .clone_object_keeping_cycles(outer)
            .expect("a cycle is followed");
        let copied = metadata(&mut document, copy).clone();
        let (Any::Object(a), Any::Object(b)) = (&copied["a"], &copied["b"]) else {
            panic!("both entries hold objects");
        };
        assert_ne!(a, b);
        assert_ne!(*a, inner);
        // Each copy of `inner` points back at the copy it is inside, not at
        // the original.
        for held in [*a, *b] {
            assert_eq!(metadata(&mut document, held)["up"], Any::Object(copy));
        }
    }

    #[test]
    fn a_dropped_copy_takes_what_its_metadata_holds_with_it() {
        let mut document = Document::new();
        let outer = with_metadata(&mut document);
        let inner = with_metadata(&mut document);
        metadata(&mut document, outer).insert("a".into(), Any::Object(inner));
        metadata(&mut document, outer).insert("b".into(), Any::Object(inner));
        let before = document.len();

        let copy = document.clone_object(outer).expect("no cycle");
        assert_eq!(document.len(), before + 3);
        document.remove_clone(copy);
        assert_eq!(document.len(), before);
    }
}

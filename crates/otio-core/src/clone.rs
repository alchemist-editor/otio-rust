//! Copying an object the way upstream's `SerializableObject::clone` does.
//!
//! [`Document::deep_clone`] copies an object held in two places once, so the
//! copy shares what the original shared, and follows a cycle without
//! complaint. Upstream's `clone` does neither: it copies by writing the
//! object out and reading it back, so an object held twice is copied twice,
//! and an object that holds itself is refused as a cycle. Its Python
//! bindings' `clone`, `deepcopy` and `copy.deepcopy` all go that way, and its
//! tests check both behaviours, so the same copy is offered here.

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};

impl Document {
    /// Copies an object and everything it owns, as upstream's `clone` does.
    ///
    /// The copy has no parent. An object held in two places — the same
    /// object under two metadata keys, say — becomes two objects in the copy.
    ///
    /// # Errors
    ///
    /// [`Error::ObjectCycle`] if the object holds itself somewhere below it,
    /// and [`Error::StaleHandle`] if a handle in it is stale. Nothing is left
    /// behind in the document on failure.
    pub fn clone_object(&mut self, id: NodeId) -> Result<NodeId> {
        let mut path = Vec::new();
        let mut made = Vec::new();
        let result = self.clone_along(id, &mut path, &mut made);
        if result.is_err() {
            for copy in made {
                self.remove(copy);
            }
        }
        result
    }

    /// Copies `id`, with `path` the objects being copied around it.
    fn clone_along(
        &mut self,
        id: NodeId,
        path: &mut Vec<NodeId>,
        made: &mut Vec<NodeId>,
    ) -> Result<NodeId> {
        let mut node = self.try_get(id)?.clone();
        if path.contains(&id) {
            return Err(Error::ObjectCycle {
                schema: node.schema_name().to_string(),
            });
        }

        // With the parent cleared, the links left are the ones the object
        // owns: its children, effects, markers, media, tracks and whatever
        // its free-form fields hold.
        node.set_parent(None);
        let mut links = Vec::new();
        node.visit_links_mut(&mut |link| links.push(*link));

        path.push(id);
        let mut copies = Vec::with_capacity(links.len());
        for link in links {
            match self.clone_along(link, path, made) {
                Ok(copy) => copies.push(copy),
                Err(error) => {
                    path.pop();
                    return Err(error);
                }
            }
        }
        path.pop();

        let mut copies_iter = copies.iter();
        node.visit_links_mut(&mut |link| {
            if let Some(copy) = copies_iter.next() {
                *link = *copy;
            }
        });
        let children = node.children().map(<[NodeId]>::to_vec).unwrap_or_default();
        let copy = self.insert(node);
        made.push(copy);
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
}

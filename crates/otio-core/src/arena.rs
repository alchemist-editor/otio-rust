//! The arena that owns a document's objects, and the handles into it.
//!
//! See `docs/adr/0001-ownership-model.md` for why the object graph is an arena
//! rather than a web of reference-counted pointers. In short: upstream's C++
//! pairs intrusive reference counting with raw parent back-pointers, which is
//! a cycle. Here a parent link is just data, because a [`NodeId`] is not an
//! owning edge.

use crate::error::{Error, Result};
use crate::schema::Node;

/// A handle to an object in a [`Document`].
///
/// Handles are small, `Copy`, and comparable, so they work as identities in
/// algorithms and cross an FFI boundary as a pair of integers.
///
/// A handle carries the generation of the slot it was issued for. If that
/// object is removed and the slot reused, the generation no longer matches and
/// lookups fail rather than silently returning the new occupant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    /// Returns this handle as a pair of integers, for passing across an FFI
    /// boundary.
    #[must_use]
    pub const fn to_raw(self) -> (u32, u32) {
        (self.index, self.generation)
    }

    /// Rebuilds a handle from [`NodeId::to_raw`].
    ///
    /// A handle rebuilt from arbitrary integers is not trusted: it is checked
    /// against the arena on every lookup like any other.
    #[must_use]
    pub const fn from_raw(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

/// One slot in the arena, occupied or free.
#[derive(Debug, Clone)]
struct Slot {
    generation: u32,
    node: Option<Node>,
}

/// An OTIO document: an arena of objects plus the handle of its root.
///
/// Dropping a document drops every object in it, once. There is no cycle to
/// break and no traversal needed.
#[derive(Debug, Clone, Default)]
pub struct Document {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: Option<NodeId>,
}

impl Document {
    /// Creates an empty document with no root.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an object to the arena and returns its handle.
    ///
    /// # Panics
    ///
    /// Panics if the arena already holds `u32::MAX` objects.
    pub fn insert(&mut self, node: Node) -> NodeId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.node = Some(node);
            return NodeId {
                index,
                generation: slot.generation,
            };
        }

        let index = u32::try_from(self.slots.len()).expect("arena holds fewer than u32::MAX nodes");
        self.slots.push(Slot {
            generation: 0,
            node: Some(node),
        });
        NodeId {
            index,
            generation: 0,
        }
    }

    /// Removes an object, returning it if the handle is live.
    ///
    /// Handles to the removed object become stale; they do not dangle.
    pub fn remove(&mut self, id: NodeId) -> Option<Node> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let node = slot.node.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(id.index);
        Some(node)
    }

    /// Borrows an object, if the handle is live.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_ref()
    }

    /// Borrows an object mutably, if the handle is live.
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_mut()
    }

    /// Borrows an object, failing with [`Error::StaleHandle`] if the handle is
    /// not live.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if the object has been removed.
    pub fn try_get(&self, id: NodeId) -> Result<&Node> {
        self.get(id).ok_or(Error::StaleHandle)
    }

    /// Borrows an object mutably, failing if the handle is not live.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StaleHandle`] if the object has been removed.
    pub fn try_get_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        self.get_mut(id).ok_or(Error::StaleHandle)
    }

    /// Returns whether a handle still refers to a live object.
    #[must_use]
    pub fn contains(&self, id: NodeId) -> bool {
        self.get(id).is_some()
    }

    /// Returns the document's root object, if it has one.
    #[must_use]
    pub const fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Sets the document's root object.
    pub const fn set_root(&mut self, root: Option<NodeId>) {
        self.root = root;
    }

    /// Returns the number of live objects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    /// Returns whether the arena holds no live objects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates over every live object and its handle, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            let node = slot.node.as_ref()?;
            let index = u32::try_from(index).ok()?;
            Some((
                NodeId {
                    index,
                    generation: slot.generation,
                },
                node,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Document, NodeId};
    use crate::schema::{Base, Node};

    fn gap() -> Node {
        Node::Gap(crate::schema::Gap::default())
    }

    #[test]
    fn insert_and_get() {
        let mut document = Document::new();
        let id = document.insert(gap());
        assert!(document.contains(id));
        assert_eq!(document.len(), 1);
        assert!(document.get(id).is_some());
    }

    #[test]
    fn removed_handles_go_stale_rather_than_dangling() {
        let mut document = Document::new();
        let first = document.insert(gap());
        document.remove(first).expect("first insert is live");

        // The slot is reused, but the old handle must not reach the new
        // occupant: that is the use-after-free this design exists to prevent.
        let second = document.insert(gap());
        assert_eq!(first.to_raw().0, second.to_raw().0);
        assert_ne!(first, second);
        assert!(document.get(first).is_none());
        assert!(document.get(second).is_some());
    }

    #[test]
    fn handles_from_arbitrary_integers_are_checked() {
        let document = Document::new();
        assert!(document.get(NodeId::from_raw(9999, 0)).is_none());
    }

    #[test]
    fn root_round_trips() {
        let mut document = Document::new();
        assert_eq!(document.root(), None);
        let id = document.insert(Node::Timeline(crate::schema::Timeline {
            base: Base::default(),
            tracks: None,
            global_start_time: None,
        }));
        document.set_root(Some(id));
        assert_eq!(document.root(), Some(id));
    }
}

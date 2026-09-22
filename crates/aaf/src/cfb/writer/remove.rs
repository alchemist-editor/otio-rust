//! Moving and removing entries, as pyaaf2 does to the streams it parks.
//!
//! pyaaf2 writes the stream of an object that is not yet in the file into a
//! storage of its own under `/tmp`, moves the stream to the object's storage
//! when the object joins the file, and removes `/tmp`, and everything left
//! in it, when the file is closed. It does that when an object holding a
//! stream is copied in from another file, which is how the OpenTimelineIO
//! adapter embeds essence it takes from an AAF. The temporary storages come
//! and go, but they take directory slots while they exist, and taking a
//! child out of a storage rebalances the storage's red-black tree, so both
//! leave their mark on the file and both are reproduced here.
//!
//! The removal is pyaaf2's `DirEntry.pop`, a top-down red-black deletion
//! after Julienne Walker's, with pyaaf2's own departures from it kept: it
//! takes the sibling from the side it is about to descend rather than the
//! side it came from, and it finds a node's grandparent's link to it by
//! comparing names, not identities.

use super::{CompoundFileWriter, LOST, Link, MINI_STREAM_CUTOFF, Node, TYPE_ROOT, TYPE_STORAGE};
use crate::cfb::dir_entry::{DirId, ROOT_ID};
use crate::cfb::error::{Error, Result};

impl CompoundFileWriter {
    /// The entry at `/`-separated `path`, if there is one: pyaaf2's `find`.
    #[must_use]
    pub fn find(&self, path: &str) -> Option<DirId> {
        let mut current = ROOT_ID;
        for name in path.split('/').filter(|n| !n.is_empty()) {
            current = self.get(current, name)?;
        }
        Some(current)
    }

    /// Creates every storage on `/`-separated `path` that is not there yet,
    /// from the root down, and returns the last: pyaaf2's `makedirs`.
    ///
    /// # Errors
    ///
    /// Returns an error if a name is too long or an entry on the way is a
    /// stream.
    pub fn makedirs(&mut self, path: &str) -> Result<DirId> {
        let mut current = ROOT_ID;
        for name in path.split('/').filter(|n| !n.is_empty()) {
            current = match self.get(current, name) {
                Some(id) => id,
                None => self.create_storage(current, name, None)?,
            };
        }
        Ok(current)
    }

    /// Moves an entry into storage `parent` under `name`: pyaaf2's `move`.
    ///
    /// The entry is taken out of its storage's tree, which rebalances it, and
    /// inserted into the new one. Whatever the entry holds moves with it.
    ///
    /// # Errors
    ///
    /// Returns an error if `parent` already has an entry of that name, is
    /// not a storage, or the entry is the root.
    pub fn move_entry(&mut self, id: DirId, parent: DirId, name: &str) -> Result<()> {
        if id == ROOT_ID {
            return Err(Error::Unrepresentable {
                what: "the root storage cannot be moved",
            });
        }
        if self.get(parent, name).is_some() {
            let sep = if parent == ROOT_ID { "" } else { "/" };
            return Err(Error::EntryExists {
                path: format!("{}{sep}{name}", self.path(parent)),
            });
        }
        if !matches!(self.node(parent.0)?.kind, TYPE_STORAGE | TYPE_ROOT) {
            return Err(Error::WrongEntryType {
                id: parent.0,
                expected: "storage",
            });
        }
        self.pop(id.0)?;
        self.entries[id.0 as usize].name = name.to_owned();
        self.add_child(parent.0, id.0)?;
        self.children
            .entry(parent.0)
            .or_default()
            .insert(name.to_owned(), id.0);
        Ok(())
    }

    /// Removes a stream, or an empty storage: pyaaf2's `remove`. A stream's
    /// sectors are freed.
    ///
    /// # Errors
    ///
    /// Returns an error for the root, or a storage that is not empty.
    pub fn remove(&mut self, id: DirId) -> Result<()> {
        let node = self.node(id.0)?;
        if node.kind == TYPE_ROOT {
            return Err(Error::Unrepresentable {
                what: "the root storage cannot be removed",
            });
        }
        if node.kind == TYPE_STORAGE && node.child.is_some() {
            return Err(Error::Unrepresentable {
                what: "a storage that is not empty cannot be removed",
            });
        }
        self.pop(id.0)?;
        let node = &self.entries[id.0 as usize];
        if node.kind != TYPE_STORAGE {
            let (sector, mini) = (node.sector, node.byte_size < MINI_STREAM_CUTOFF);
            self.free_fat_chain(sector, mini);
        }
        self.free_dir_entry(id.0);
        Ok(())
    }

    /// Removes a storage and everything in it: pyaaf2's `rmtree`.
    ///
    /// Everything below the storage is freed without being taken out of its
    /// storage's tree, since the whole tree goes; only the storage itself is
    /// taken out of its parent's. pyaaf2 frees the entries in the order its
    /// listings give them. Here they are freed in the order they were made,
    /// which leaves the same file as long as nothing is written after, as
    /// nothing is when pyaaf2 does this, at close.
    ///
    /// # Errors
    ///
    /// Returns an error for the root.
    pub fn rmtree(&mut self, id: DirId) -> Result<()> {
        let mut storages = vec![id.0];
        let mut i = 0;
        while let Some(&storage) = storages.get(i) {
            i += 1;
            let mut children: Vec<u32> = self
                .children
                .get(&storage)
                .map(|names| names.values().copied().collect())
                .unwrap_or_default();
            children.sort_unstable();
            for child in children {
                let node = &self.entries[child as usize];
                if node.kind == TYPE_STORAGE {
                    storages.push(child);
                } else {
                    let (sector, mini) = (node.sector, node.byte_size < MINI_STREAM_CUTOFF);
                    self.free_fat_chain(sector, mini);
                    self.free_dir_entry(child);
                }
            }
        }
        for &storage in storages.iter().skip(1).rev() {
            self.free_dir_entry(storage);
        }
        self.entries[id.0 as usize].child = None;
        self.children.remove(&id.0);
        self.remove(id)
    }

    /// Returns an entry's slot to the free list: pyaaf2's `free_dir_entry`.
    ///
    /// pyaaf2 leaves the slot's bytes as they are until the file is closed,
    /// when it zeroes every free slot; the writer writes each entry only at
    /// the end, so blanking it now comes to the same.
    fn free_dir_entry(&mut self, id: u32) {
        self.dir_freelist.push_back(id);
        self.children.remove(&id);
        self.open.remove(&id);
        self.entries[id as usize] = Node::blank();
    }

    /// Whether the entry a link names sorts before `entry`: pyaaf2's
    /// `DirEntry.__lt__`, with the false root, whose name is empty, first.
    fn link_less(&self, a: Link, entry: u32) -> bool {
        match a {
            Link::Head => !self.entries[entry as usize].name.is_empty(),
            Link::Entry(a) => self.less(a, entry),
        }
    }

    /// Whether `entry` sorts before the entry a link names.
    fn less_link(&self, entry: u32, b: Link) -> bool {
        match b {
            Link::Head => false,
            Link::Entry(b) => self.less(entry, b),
        }
    }

    /// pyaaf2's `DirEntry.__eq__`: the same name, ignoring case.
    fn same_name(&self, a: Option<Link>, b: Link) -> bool {
        let name = |link: Link| match link {
            Link::Head => "",
            Link::Entry(i) => self.entries[i as usize].name.as_str(),
        };
        let Some(a) = a else { return false };
        let (a, b) = (name(a), name(b));
        a.chars().count() == b.chars().count() && a.to_uppercase() == b.to_uppercase()
    }

    fn child_link(&self, node: Link, side: usize) -> Option<Link> {
        self.link(node, side).map(Link::Entry)
    }

    fn set_child_link(&mut self, node: Link, side: usize, value: Option<Link>) -> Result<()> {
        let value = match value {
            None => None,
            Some(Link::Entry(i)) => Some(i),
            Some(Link::Head) => return Err(LOST),
        };
        self.set_link(node, side, value);
        Ok(())
    }

    /// pyaaf2's `jsw_single` on any node.
    fn rotate(&mut self, root: Link, direction: usize) -> Result<Link> {
        match root {
            Link::Entry(i) => Ok(Link::Entry(self.single(i, direction)?)),
            Link::Head => Err(LOST),
        }
    }

    /// pyaaf2's `jsw_double` on any node.
    fn rotate_twice(&mut self, root: Link, direction: usize) -> Result<Link> {
        match root {
            Link::Entry(i) => Ok(Link::Entry(self.double(i, direction)?)),
            Link::Head => Err(LOST),
        }
    }

    /// pyaaf2's `find_entry_parent`: the node whose child `entry` is,
    /// searching down from `root` at most `max_depth` steps.
    fn find_entry_parent(&self, root: Link, entry: u32, max_depth: usize) -> Result<Option<Link>> {
        let mut parent = None;
        let mut node = Some(root);
        let mut count = 0;
        while let Some(n) = node {
            if count >= max_depth {
                break;
            }
            if n == Link::Entry(entry) {
                return Ok(parent);
            }
            let direction = usize::from(!self.less_link(entry, n));
            parent = Some(n);
            node = self.child_link(n, direction);
            count += 1;
        }
        Err(Error::Unrepresentable {
            what: "a directory tree removal went deeper than it can",
        })
    }

    /// Takes `entry` out of its storage's tree: pyaaf2's `DirEntry.pop`.
    fn pop(&mut self, entry: u32) -> Result<()> {
        let storage = self.entries[entry as usize].parent.ok_or(LOST)?;
        let max_entries = self.dir_sector_count * (self.sector_size / 128);
        let mut count = 0;

        self.head = Node::blank();
        self.head.red = true;
        let head = Link::Head;
        let mut node = head;
        self.head.right = self.entries[storage as usize].child;
        let mut grand_parent: Option<Link>;
        let mut parent: Option<Link> = None;
        let mut entry_grand_parent: Option<Link> = None;
        let mut direction = 1usize;
        let mut found = false;

        // This goes on until the entry's predecessor is found, past the
        // entry itself.
        while self.link(node, direction).is_some() && count < max_entries {
            let last = direction;
            grand_parent = parent;
            parent = Some(node);
            node = self.child_link(node, direction).ok_or(LOST)?;
            direction = usize::from(self.link_less(node, entry));

            if node == Link::Entry(entry) {
                // The entry's parent can change as the tree is rebalanced
                // below, so keep the node above it.
                entry_grand_parent = Some(grand_parent.unwrap_or(head));
                found = true;
            }

            // Push the red node down.
            if !self.is_red(Some(node)) && !self.is_red(self.child_link(node, direction)) {
                let p = parent.ok_or(LOST)?;
                if self.is_red(self.child_link(node, 1 - direction)) {
                    let rotated = self.rotate(node, direction)?;
                    self.set_child_link(p, last, Some(rotated))?;
                    parent = self.child_link(p, last);
                } else if let Some(sibling) = self.child_link(p, 1 - direction) {
                    if !self.is_red(self.child_link(sibling, 1 - last))
                        && !self.is_red(self.child_link(sibling, last))
                    {
                        // Colour flip.
                        self.set_red(p, false);
                        self.set_red(sibling, true);
                        self.set_red(node, true);
                    } else {
                        let g = grand_parent.ok_or(LOST)?;
                        let direction2 = if self.same_name(self.child_link(g, 0), p) {
                            0
                        } else if self.same_name(self.child_link(g, 1), p) {
                            1
                        } else {
                            return Err(LOST);
                        };
                        if self.is_red(self.child_link(sibling, last)) {
                            let rotated = self.rotate_twice(p, last)?;
                            self.set_child_link(g, direction2, Some(rotated))?;
                        } else if self.is_red(self.child_link(sibling, 1 - last)) {
                            let rotated = self.rotate(p, last)?;
                            self.set_child_link(g, direction2, Some(rotated))?;
                        }
                        // Ensure correct colouring.
                        self.set_red(node, true);
                        let top = self.child_link(g, direction2).ok_or(LOST)?;
                        self.set_red(top, true);
                        let left = self.child_link(top, 0).ok_or(LOST)?;
                        self.set_red(left, false);
                        let right = self.child_link(top, 1).ok_or(LOST)?;
                        self.set_red(right, false);
                    }
                }
            }
            count += 1;
        }

        if !found {
            return Err(LOST);
        }
        if count >= max_entries {
            return Err(Error::Unrepresentable {
                what: "a storage has more children than the directory holds",
            });
        }

        let entry_parent = self
            .find_entry_parent(entry_grand_parent.ok_or(LOST)?, entry, 4)?
            .ok_or(LOST)?;
        let entry_direction = if self.link(entry_parent, 0) == Some(entry) {
            0
        } else if self.link(entry_parent, 1) == Some(entry) {
            1
        } else {
            return Err(Error::Unrepresentable {
                what: "a directory tree removal lost the entry's parent",
            });
        };

        let e = Link::Entry(entry);
        if node == e {
            let side = usize::from(self.link(e, 0).is_none());
            let replacement = self.link(e, side);
            self.set_link(entry_parent, entry_direction, replacement);
        } else {
            // `node` is the entry's predecessor, which takes its place.
            let p = parent.ok_or(LOST)?;
            let n = match node {
                Link::Entry(n) => n,
                Link::Head => return Err(LOST),
            };
            let side = usize::from(self.link(p, 1) == Some(n));
            let heir = self.link(node, usize::from(self.link(node, 0).is_none()));
            self.set_link(p, side, heir);
            let (left, right, red) = {
                let en = &self.entries[entry as usize];
                (en.left, en.right, en.red)
            };
            self.set_link(node, 0, left);
            self.set_link(node, 1, right);
            self.set_red(node, red);
            self.set_link(entry_parent, entry_direction, Some(n));
        }

        // The tree's root may have changed.
        self.entries[storage as usize].child = self.head.right;
        if let Some(root) = self.head.right {
            self.entries[root as usize].red = false;
        }

        let name = self.entries[entry as usize].name.clone();
        if let Some(names) = self.children.get_mut(&storage) {
            names.remove(&name);
        }
        let node = &mut self.entries[entry as usize];
        node.left = None;
        node.right = None;
        node.parent = None;
        Ok(())
    }
}

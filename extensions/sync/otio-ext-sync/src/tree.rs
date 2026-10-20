//! The structural view of a timeline that diffing and applying work on: the
//! Timeline, its Stack, the Tracks and the items in them, each known by its
//! GUID.
//!
//! Nodes live in an arena and refer to each other by index, so a node taken
//! out of the tree stays where it is and can be put back.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;

use crate::raw;

/// A node of the structural tree.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// The schema name without its version, such as `Clip`.
    pub kind: String,
    /// Its `metadata.sync.guid`.
    pub guid: String,
    /// Where its JSON is in the text the tree was parsed from. Empty for a
    /// node made only as topology.
    pub raw: Range<usize>,
    /// Its members, keyed by name, as JSON text. Only an applying tree reads
    /// them; a diffing tree leaves them empty and reads [`Node::raw`].
    pub fields: BTreeMap<String, String>,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

/// A timeline's structural tree.
#[derive(Debug, Clone, Default)]
pub(crate) struct Arena {
    pub nodes: Vec<Node>,
    pub root: usize,
    pub by_guid: HashMap<String, usize>,
    /// The nodes in the tree, in preorder.
    pub order: Vec<usize>,
}

/// Something that makes a timeline unaddressable; the text is the fallback
/// reason Furnace gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem(pub String);

impl Problem {
    fn guidless(at: &str) -> Self {
        Self(format!("guidless node at {at}"))
    }
}

const NON_OBJECT: &str = "non-object JSON value";

/// What Furnace's `nodeEnvelope` decodes from a node: just the members that
/// shape the tree.
#[derive(Default)]
struct Envelope<'a> {
    schema: String,
    guid: String,
    tracks: Option<&'a str>,
    children: Option<Vec<&'a str>>,
}

/// Whether `key` names the struct field `field` in Go's eyes.
fn names(key: &str, field: &str) -> bool {
    key == field || raw::fold_name(key) == raw::fold_name(field)
}

/// Decodes a node's envelope as Go's `json.Unmarshal` would, or fails where
/// Go would report an error: a member of the wrong type, or a node that is
/// not an object.
fn envelope(text: &str) -> Option<Envelope<'_>> {
    let mut envelope = Envelope::default();
    for (key, value) in raw::members(text)? {
        let first = value.as_bytes()[0];
        if names(&key, "OTIO_SCHEMA") {
            match first {
                b'"' => envelope.schema = raw::decode_string(value),
                b'n' => {}
                _ => return None,
            }
        } else if names(&key, "metadata") {
            if first == b'n' {
                continue;
            }
            // Go decodes a repeated member into the same struct, so a second
            // `metadata` adds to the first rather than replacing it.
            for (key, value) in raw::members(value)? {
                if !names(&key, "sync") {
                    continue;
                }
                if value.as_bytes()[0] == b'n' {
                    continue;
                }
                for (key, value) in raw::members(value)? {
                    if !names(&key, "guid") {
                        continue;
                    }
                    match value.as_bytes()[0] {
                        b'"' => envelope.guid = raw::decode_string(value),
                        b'n' => {}
                        _ => return None,
                    }
                }
            }
        } else if names(&key, "tracks") {
            envelope.tracks = Some(value);
        } else if names(&key, "children") {
            envelope.children = if first == b'n' {
                None
            } else {
                Some(raw::elements(value)?)
            };
        }
    }
    Some(envelope)
}

/// The schema name without a trailing `.N` version.
fn schema_kind(schema: &str) -> &str {
    match schema.rfind('.') {
        Some(dot)
            if dot + 1 < schema.len() && schema[dot + 1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            &schema[..dot]
        }
        _ => schema,
    }
}

fn recognized(kind: &str) -> bool {
    matches!(kind, "Timeline" | "Stack" | "Track" | "Clip" | "Gap")
}

/// Whether a node of this kind has a children array patches can change.
pub(crate) fn has_children(kind: &str) -> bool {
    kind == "Stack" || kind == "Track"
}

/// Whether `key` is an ordinary property of a node of this kind, rather than
/// its schema or the member that holds its children.
pub(crate) fn is_value_property(kind: &str, key: &str) -> bool {
    !(key == "OTIO_SCHEMA"
        || (kind == "Timeline" && key == "tracks")
        || (has_children(kind) && key == "children"))
}

impl Arena {
    /// Parses a timeline into a tree, checking that every GUID is used once.
    ///
    /// With `eager`, each node's members are read into [`Node::fields`] for
    /// applying patches; without, they are left in the text for diffing.
    /// `text` must be valid JSON.
    pub(crate) fn parse(text: &str, eager: bool) -> Result<Self, Problem> {
        let mut arena = Self::default();
        let root = arena.parse_node(text, 0, text, None, eager)?;
        if arena.nodes[root].kind != "Timeline" {
            return Err(Problem(format!(
                "root is not a Timeline (found {})",
                arena.nodes[root].kind
            )));
        }
        arena.root = root;
        arena.reindex_checked()?;
        Ok(arena)
    }

    /// Parses a subtree into this arena, unattached, and returns its root.
    /// `text` must be valid JSON.
    pub(crate) fn parse_detached(&mut self, text: &str) -> Result<usize, Problem> {
        self.parse_node(text, 0, text, None, true)
    }

    /// Parses `text`, a slice of `base` starting at `offset`, as a node.
    fn parse_node(
        &mut self,
        base: &str,
        offset: usize,
        text: &str,
        parent: Option<usize>,
        eager: bool,
    ) -> Result<usize, Problem> {
        let trimmed = raw::trim(text);
        if !trimmed.starts_with('{') {
            return Err(Problem::guidless(NON_OBJECT));
        }
        let envelope = envelope(trimmed).ok_or_else(|| Problem::guidless(NON_OBJECT))?;
        let kind = schema_kind(&envelope.schema).to_owned();
        if !recognized(&kind) {
            let schema = if envelope.schema.is_empty() {
                "missing OTIO_SCHEMA"
            } else {
                &envelope.schema
            };
            return Err(Problem::guidless(schema));
        }
        if envelope.guid.is_empty() {
            return Err(Problem::guidless(&envelope.schema));
        }
        let fields = if eager {
            raw::member_map(trimmed)
                .ok_or_else(|| Problem::guidless(NON_OBJECT))?
                .into_iter()
                .map(|(key, value)| (key, value.to_owned()))
                .collect()
        } else {
            BTreeMap::new()
        };
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind: kind.clone(),
            guid: envelope.guid,
            raw: offset..offset + text.len(),
            fields,
            children: Vec::new(),
            parent,
        });

        let at = |slice: &str| slice.as_ptr() as usize - base.as_ptr() as usize;
        let mut children = Vec::new();
        match kind.as_str() {
            "Timeline" => {
                let tracks = envelope
                    .tracks
                    .filter(|tracks| *tracks != "null")
                    .ok_or_else(|| Problem::guidless("Timeline tracks"))?;
                let child = self.parse_node(base, at(tracks), tracks, Some(id), eager)?;
                if self.nodes[child].kind != "Stack" {
                    return Err(Problem(format!(
                        "Timeline tracks is {}, not Stack",
                        self.nodes[child].kind
                    )));
                }
                children.push(child);
            }
            "Stack" | "Track" => {
                for child_text in envelope.children.unwrap_or_default() {
                    children.push(self.parse_node(
                        base,
                        at(child_text),
                        child_text,
                        Some(id),
                        eager,
                    )?);
                }
            }
            _ => {}
        }
        self.nodes[id].children = children;
        Ok(id)
    }

    /// A copy of this tree's shape, without any JSON.
    pub(crate) fn clone_topology(&self) -> Self {
        let mut copy = Self::default();
        copy.root = copy.copy_subtree(self, self.root);
        copy.reindex();
        copy
    }

    /// Copies the shape of `node` and everything under it from `source` into
    /// this arena, unattached, and returns the copy.
    pub(crate) fn copy_subtree(&mut self, source: &Self, node: usize) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind: source.nodes[node].kind.clone(),
            guid: source.nodes[node].guid.clone(),
            raw: 0..0,
            fields: BTreeMap::new(),
            children: Vec::new(),
            parent: None,
        });
        let children: Vec<usize> = source.nodes[node]
            .children
            .iter()
            .map(|&child| {
                let copy = self.copy_subtree(source, child);
                self.nodes[copy].parent = Some(id);
                copy
            })
            .collect();
        self.nodes[id].children = children;
        id
    }

    fn reindex_checked(&mut self) -> Result<(), Problem> {
        self.reindex();
        if self.order.len() != self.by_guid.len() {
            let mut seen = HashSet::new();
            for &node in &self.order {
                if !seen.insert(self.nodes[node].guid.as_str()) {
                    return Err(Problem(format!("duplicate guid {}", self.nodes[node].guid)));
                }
            }
        }
        Ok(())
    }

    /// Rebuilds the GUID index and the preorder from the root, setting every
    /// reachable node's parent on the way.
    pub(crate) fn reindex(&mut self) {
        self.by_guid.clear();
        self.order.clear();
        let mut stack = vec![self.root];
        while let Some(node) = stack.pop() {
            // A later duplicate replaces an earlier one, as in Go.
            self.by_guid.insert(self.nodes[node].guid.clone(), node);
            self.order.push(node);
            let children = self.nodes[node].children.clone();
            for &child in children.iter().rev() {
                self.nodes[child].parent = Some(node);
                stack.push(child);
            }
        }
    }

    /// Takes a node out of its parent's children.
    pub(crate) fn detach(&mut self, node: usize) {
        let Some(parent) = self.nodes[node].parent else {
            return;
        };
        if let Some(at) = self.index_of_child(parent, node) {
            self.nodes[parent].children.remove(at);
            self.nodes[node].parent = None;
        }
    }

    /// Where `child` is among `parent`'s children.
    pub(crate) fn index_of_child(&self, parent: usize, child: usize) -> Option<usize> {
        self.nodes[parent].children.iter().position(|&c| c == child)
    }

    fn is_ancestor(&self, candidate: usize, node: usize) -> bool {
        let mut at = Some(node);
        while let Some(current) = at {
            if current == candidate {
                return true;
            }
            at = self.nodes[current].parent;
        }
        false
    }

    /// Moves `node` into `dest`'s children at `index`, counted after `node`
    /// has been taken out of its old place.
    pub(crate) fn move_node(&mut self, node: usize, dest: usize, index: i64) -> Result<(), String> {
        if self.nodes[node].parent.is_none() {
            return Err("cannot move root or detached node".into());
        }
        if self.is_ancestor(node, dest) {
            return Err("move would create a cycle".into());
        }
        self.detach(node);
        let at = checked_index(index, self.nodes[dest].children.len())
            .ok_or_else(|| format!("destination index {index} out of range"))?;
        self.nodes[dest].children.insert(at, node);
        self.nodes[node].parent = Some(dest);
        Ok(())
    }

    /// Inserts the unattached subtree `inserted` into `dest`'s children at
    /// `index`. A node already in the tree with a GUID the subtree also holds
    /// is taken out first, unless an ancestor of it is, so the subtree
    /// adopts it.
    pub(crate) fn adopt_and_insert(
        &mut self,
        dest: usize,
        index: i64,
        inserted: usize,
    ) -> Result<(), String> {
        let mut guids = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![inserted];
        while let Some(node) = stack.pop() {
            let guid = self.nodes[node].guid.clone();
            if !seen.insert(guid.clone()) {
                return Err(format!("duplicate guid {guid} in inserted subtree"));
            }
            guids.push(guid);
            stack.extend(self.nodes[node].children.iter().rev());
        }
        if seen.contains(&self.nodes[dest].guid) {
            return Err("inserted subtree contains destination parent".into());
        }
        for guid in &guids {
            let Some(&existing) = self.by_guid.get(guid) else {
                continue;
            };
            let mut ancestor_also_inserted = false;
            let mut at = self.nodes[existing].parent;
            while let Some(parent) = at {
                if seen.contains(&self.nodes[parent].guid) {
                    ancestor_also_inserted = true;
                    break;
                }
                at = self.nodes[parent].parent;
            }
            if !ancestor_also_inserted {
                if self.nodes[existing].parent.is_none() {
                    return Err("cannot adopt the timeline root".into());
                }
                self.detach(existing);
            }
        }
        let at = checked_index(index, self.nodes[dest].children.len())
            .ok_or_else(|| format!("destination index {index} out of range"))?;
        self.nodes[dest].children.insert(at, inserted);
        self.nodes[inserted].parent = Some(dest);
        self.reindex();
        Ok(())
    }

    /// The JSON of `node` and everything under it, as Furnace writes it: the
    /// members of each node in byte order of their names, and everything
    /// compacted.
    pub(crate) fn marshal(&self, node: usize) -> String {
        let mut out = String::new();
        self.marshal_into(node, &mut out);
        out
    }

    fn marshal_into(&self, node: usize, out: &mut String) {
        let node = &self.nodes[node];
        let structural = match node.kind.as_str() {
            "Timeline" if node.children.len() == 1 => Some("tracks"),
            "Stack" | "Track" => Some("children"),
            _ => None,
        };
        let mut keys: Vec<&str> = node.fields.keys().map(String::as_str).collect();
        if let Some(key) = structural.filter(|key| !node.fields.contains_key(*key)) {
            keys.push(key);
            keys.sort_unstable();
        }
        out.push('{');
        for (i, key) in keys.into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            raw::push_string(out, key);
            out.push(':');
            if Some(key) == structural {
                if node.kind == "Timeline" {
                    self.marshal_into(node.children[0], out);
                } else {
                    out.push('[');
                    for (j, &child) in node.children.iter().enumerate() {
                        if j > 0 {
                            out.push(',');
                        }
                        self.marshal_into(child, out);
                    }
                    out.push(']');
                }
            } else {
                raw::compact_into(out, &node.fields[key]);
            }
        }
        out.push('}');
    }
}

/// `index` as a position in a list of `len` items, if it is one: from zero
/// to `len` inclusive.
pub(crate) fn checked_index(index: i64, len: usize) -> Option<usize> {
    usize::try_from(index).ok().filter(|&at| at <= len)
}

/// An object's members as a map of JSON text, or an error if it is not an
/// object; absent metadata reads as empty, as in Furnace.
pub(crate) fn metadata_namespaces(text: Option<&str>) -> Result<BTreeMap<String, &str>, String> {
    match text {
        None | Some("") => Ok(BTreeMap::new()),
        Some(text) => raw::member_map(text).ok_or_else(|| "metadata is not an object".into()),
    }
}

//! Working out the patches between two versions of a timeline.
//!
//! A port of `Diff`, `DiffWithOptions`, `ParseSnapshot` and `DiffTrees` from
//! Furnace's `internal/otiodiff` (svc-furnace, in Go), algorithm for
//! algorithm, so both make the same patches from the same snapshots and
//! Furnace's golden files are this module's tests.
//!
//! # How a diff is made
//!
//! 1. **Removes.** Walking the old tree in preorder, each node whose GUID the
//!    new tree lacks is removed from its parent, and its subtree with it.
//! 2. **Alignment.** Walking the new tree from the top, each Stack and Track
//!    has its children brought into the new order on a working copy of the
//!    old tree's shape:
//!    - a child the working tree lacks is inserted, with its whole subtree,
//!      which adopts any node of that subtree already elsewhere;
//!    - a child under another parent is moved in;
//!    - of the children already there, the longest run already in the new
//!      order stays put, and only the rest move. Each is moved once, straight
//!      to its final index, in an order chosen so no later move disturbs it;
//!      where no such order exists, one is placed anyway and a correction
//!      pass puts right whatever it displaced. A single displaced child
//!      always costs a single move.
//! 3. **Properties.** For each node in both trees, in the new tree's
//!    preorder, each changed member becomes a `set_property` of its own,
//!    in byte order of the member's name, and `metadata` is compared one
//!    namespace at a time so two programs can change different namespaces
//!    of one object. `metadata.sync` is never patched.
//!
//! # Falling back
//!
//! Some changes cannot be expressed as patches, and some are cheaper to send
//! whole. The diff then sends the whole new timeline as one
//! [`Op::ReplaceTimeline`], and [`Stats::fallback_reason`] says why, in
//! Furnace's words:
//!
//! - `generation regression`: the new snapshot's generation is not later;
//! - `sequenceId changed`;
//! - `timeline guid changed`;
//! - `timeline tracks slot changed`: the Stack under the Timeline is not the
//!   same one;
//! - `op count threshold exceeded: …`: more than [`Options::max_ops`]
//!   patches (500), or more than [`Options::max_ops_fraction`] (40%) of the
//!   nodes;
//! - a parse problem, such as `guidless node at Transition.1` or
//!   `duplicate guid …`: a node sync cannot address.

use std::collections::{HashMap, HashSet};

use crate::patch::{
    DEFAULT_MAX_OPS, DEFAULT_MAX_OPS_FRACTION, Error, Op, Options, Patch, Snapshot, Stats,
};
use crate::raw;
use crate::tree::{Arena, Node, has_children, is_value_property, metadata_namespaces};

/// A snapshot parsed once, to diff against others without parsing it again.
///
/// It keeps the snapshot's sequence id, generation and timeline, so
/// [`diff_trees`] checks and falls back exactly as [`diff_with_options`]
/// does. It is never changed by a diff, so one tree can be diffed many
/// times, from many threads, on either side.
#[derive(Debug, Clone)]
pub struct Tree {
    sequence_id: String,
    generation: i64,
    timeline: String,
    parsed: Arena,
}

impl Tree {
    /// Parses a snapshot's timeline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidJson`] if the timeline is not JSON, and
    /// [`Error::Unaddressable`] if it holds something sync cannot address,
    /// with the reason a diff would fall back with.
    pub fn parse(snapshot: &Snapshot) -> Result<Self, Error> {
        if !raw::is_valid(&snapshot.timeline) {
            return Err(Error::InvalidJson("timeline"));
        }
        parse_valid(snapshot).map_err(Error::Unaddressable)
    }

    /// The sequence this is a version of.
    #[must_use]
    pub fn sequence_id(&self) -> &str {
        &self.sequence_id
    }

    /// The snapshot's generation.
    #[must_use]
    pub const fn generation(&self) -> i64 {
        self.generation
    }

    /// The timeline, as the JSON it was parsed from.
    #[must_use]
    pub fn timeline(&self) -> &str {
        &self.timeline
    }

    fn raw(&self, node: &Node) -> &str {
        &self.timeline[node.raw.clone()]
    }
}

/// Parses a snapshot whose timeline is known to be JSON.
fn parse_valid(snapshot: &Snapshot) -> Result<Tree, String> {
    let parsed = Arena::parse(&snapshot.timeline, false).map_err(|problem| problem.0)?;
    Ok(Tree {
        sequence_id: snapshot.sequence_id.clone(),
        generation: snapshot.generation,
        timeline: snapshot.timeline.clone(),
        parsed,
    })
}

/// Works out the patches that turn `old` into `new`, with the default
/// limits.
///
/// # Errors
///
/// Returns [`Error::InvalidJson`] if either timeline is not JSON. Anything
/// else that stops a diff makes it fall back instead: see the
/// [module documentation](self).
pub fn diff(old: &Snapshot, new: &Snapshot) -> Result<(Vec<Patch>, Stats), Error> {
    diff_with_options(old, new, &Options::default())
}

/// Works out the patches that turn `old` into `new`, with the limits in
/// `options`.
///
/// # Errors
///
/// Returns [`Error::InvalidJson`] if either timeline is not JSON. Anything
/// else that stops a diff makes it fall back instead: see the
/// [module documentation](self).
pub fn diff_with_options(
    old: &Snapshot,
    new: &Snapshot,
    options: &Options,
) -> Result<(Vec<Patch>, Stats), Error> {
    if !raw::is_valid(&old.timeline) {
        return Err(Error::InvalidJson("old timeline"));
    }
    if !raw::is_valid(&new.timeline) {
        return Err(Error::InvalidJson("new timeline"));
    }
    if let Some(reason) = snapshot_change(
        &old.sequence_id,
        old.generation,
        &new.sequence_id,
        new.generation,
    ) {
        return Ok(fallback(&new.timeline, Stats::default(), reason));
    }
    let old_tree = match parse_valid(old) {
        Ok(tree) => tree,
        Err(reason) => return Ok(fallback(&new.timeline, Stats::default(), reason)),
    };
    let new_tree = match parse_valid(new) {
        Ok(tree) => tree,
        Err(reason) => return Ok(fallback(&new.timeline, Stats::default(), reason)),
    };
    Ok(diff_parsed(&old_tree, &new_tree, options))
}

/// Works out the patches that turn `old` into `new`, both already parsed.
///
/// It checks the sequence ids and generations the trees kept, and falls back
/// just as [`diff_with_options`] does. Neither tree is changed.
#[must_use]
pub fn diff_trees(old: &Tree, new: &Tree, options: &Options) -> (Vec<Patch>, Stats) {
    if let Some(reason) = snapshot_change(
        &old.sequence_id,
        old.generation,
        &new.sequence_id,
        new.generation,
    ) {
        return fallback(&new.timeline, Stats::default(), reason);
    }
    diff_parsed(old, new, options)
}

fn snapshot_change(
    old_sequence: &str,
    old_generation: i64,
    new_sequence: &str,
    new_generation: i64,
) -> Option<&'static str> {
    if new_generation <= old_generation {
        Some("generation regression")
    } else if old_sequence != new_sequence {
        Some("sequenceId changed")
    } else {
        None
    }
}

fn fallback(timeline: &str, mut stats: Stats, reason: impl Into<String>) -> (Vec<Patch>, Stats) {
    stats.fell_back = true;
    stats.fallback_reason = reason.into();
    let patch = Patch {
        op: Op::ReplaceTimeline,
        timeline: Some(timeline.to_owned()),
        ..Patch::default()
    };
    (vec![patch], stats)
}

fn diff_parsed(old: &Tree, new: &Tree, options: &Options) -> (Vec<Patch>, Stats) {
    let old_tree = &old.parsed;
    let new_tree = &new.parsed;
    let mut stats = Stats {
        node_count: old_tree.by_guid.len().max(new_tree.by_guid.len()),
        ..Stats::default()
    };
    let old_root = &old_tree.nodes[old_tree.root];
    let new_root = &new_tree.nodes[new_tree.root];
    if old_root.guid != new_root.guid {
        return fallback(&new.timeline, stats, "timeline guid changed");
    }
    if old_root.children.len() != 1
        || new_root.children.len() != 1
        || old_tree.nodes[old_root.children[0]].guid != new_tree.nodes[new_root.children[0]].guid
    {
        return fallback(&new.timeline, stats, "timeline tracks slot changed");
    }

    // Removes, in old-tree preorder; a removed node takes its subtree with it.
    let mut patches = Vec::new();
    let mut removed_roots = Vec::new();
    let mut stack = vec![old_tree.root];
    while let Some(node) = stack.pop() {
        let current = &old_tree.nodes[node];
        if !new_tree.by_guid.contains_key(&current.guid) {
            if let Some(parent) = current.parent {
                patches.push(Patch {
                    op: Op::RemoveChild,
                    parent_guid: old_tree.nodes[parent].guid.clone(),
                    guid: current.guid.clone(),
                    ..Patch::default()
                });
                stats.remove_ops += 1;
                removed_roots.push(current.guid.clone());
            }
            continue;
        }
        stack.extend(current.children.iter().rev());
    }

    let mut working = old_tree.clone_topology();
    for guid in &removed_roots {
        if let Some(&node) = working.by_guid.get(guid) {
            working.detach(node);
        }
    }
    if !removed_roots.is_empty() {
        working.reindex();
    }

    let mut aligner = Aligner {
        new,
        working,
        patches,
        stats,
        inserted_roots: HashSet::new(),
    };
    if let Err(message) = aligner.align(new_tree.root) {
        let stats = aligner.stats;
        return fallback(&new.timeline, stats, format!("alignment failed: {message}"));
    }
    let Aligner {
        mut patches,
        mut stats,
        ..
    } = aligner;

    for &node in &new_tree.order {
        let new_node = &new_tree.nodes[node];
        let Some(&old_node) = old_tree.by_guid.get(&new_node.guid) else {
            continue;
        };
        match diff_properties(old, &old_tree.nodes[old_node], new, new_node) {
            Ok(property_patches) => {
                stats.set_property_ops += property_patches.len();
                patches.extend(property_patches);
            }
            Err(message) => {
                return fallback(
                    &new.timeline,
                    stats,
                    format!("property diff failed: {message}"),
                );
            }
        }
    }

    stats.total_ops = patches.len();
    let max_ops = if options.max_ops == 0 {
        DEFAULT_MAX_OPS
    } else {
        options.max_ops
    };
    let fraction = if options.max_ops_fraction <= 0.0 {
        DEFAULT_MAX_OPS_FRACTION
    } else {
        options.max_ops_fraction
    };
    let too_many =
        patches.len() > max_ops || patches.len() as f64 > fraction * stats.node_count as f64;
    if too_many {
        let reason = format!(
            "op count threshold exceeded: {} ops, max {max_ops}, node count {}, fraction {}",
            patches.len(),
            stats.node_count,
            format_g3(fraction)
        );
        return fallback(&new.timeline, stats, reason);
    }
    (patches, stats)
}

/// The alignment pass: brings the working tree's children into the new
/// tree's order, recording each insert and move as a patch.
struct Aligner<'a> {
    new: &'a Tree,
    working: Arena,
    patches: Vec<Patch>,
    stats: Stats,
    /// Nodes inserted whole, whose subtrees need no further alignment.
    inserted_roots: HashSet<String>,
}

impl<'a> Aligner<'a> {
    fn move_patch(&mut self, parent_guid: &str, guid: &str, index: usize) {
        self.patches.push(Patch {
            op: Op::MoveChild,
            parent_guid: parent_guid.to_owned(),
            guid: guid.to_owned(),
            index: Some(index as i64),
            ..Patch::default()
        });
        self.stats.move_ops += 1;
    }

    fn align(&mut self, target_parent: usize) -> Result<(), String> {
        let new: &'a Tree = self.new;
        let new_tree = &new.parsed;
        let target = &new_tree.nodes[target_parent];
        if self.inserted_roots.contains(&target.guid) {
            return Ok(());
        }
        let working_parent = *self.working.by_guid.get(&target.guid).ok_or_else(|| {
            format!(
                "existing target parent {} is absent from working tree",
                raw::quote(&target.guid)
            )
        })?;
        if target.kind == "Timeline" {
            return self.align(target.children[0]);
        }
        if !has_children(&target.kind) {
            return Ok(());
        }
        let target_guid = target.guid.as_str();
        let target_children: Vec<&str> = target
            .children
            .iter()
            .map(|&child| new_tree.nodes[child].guid.as_str())
            .collect();

        let target_pos: HashMap<&str, usize> = target_children
            .iter()
            .enumerate()
            .map(|(i, guid)| (*guid, i))
            .collect();
        let mut current_target = Vec::new();
        let mut same_parent_at_start = Vec::new();
        for &child in &self.working.nodes[working_parent].children {
            let guid = self.working.nodes[child].guid.as_str();
            if target_pos.contains_key(guid) {
                current_target.push(guid.to_owned());
                same_parent_at_start.push(guid.to_owned());
            }
        }
        let keep = longest_increasing(&current_target, &target_pos);

        // First bring genuinely new and cross-parent children into this
        // parent.
        for (target_index, &child) in target.children.iter().enumerate() {
            let guid = target_children[target_index];
            match self.working.by_guid.get(guid).copied() {
                None => {
                    let copy = self.working.copy_subtree(new_tree, child);
                    self.patches.push(Patch {
                        op: Op::InsertChild,
                        parent_guid: target_guid.to_owned(),
                        index: Some(target_index as i64),
                        node: Some(new.raw(&new_tree.nodes[child]).to_owned()),
                        ..Patch::default()
                    });
                    self.stats.insert_ops += 1;
                    self.working
                        .adopt_and_insert(working_parent, target_index as i64, copy)?;
                    self.inserted_roots.insert(guid.to_owned());
                }
                Some(current) if self.working.nodes[current].parent != Some(working_parent) => {
                    self.move_patch(target_guid, guid, target_index);
                    self.working
                        .move_node(current, working_parent, target_index as i64)?;
                }
                Some(_) => {}
            }
        }

        // Then move exactly the same-parent children outside the LIS.
        // Deferring this pass is what keeps an LIS member from being moved
        // merely because a non-LIS member currently sits before it.
        let mut moved: HashSet<String> = same_parent_at_start
            .into_iter()
            .filter(|guid| !keep.contains(guid.as_str()))
            .collect();
        while !moved.is_empty() {
            let (guid, target_index) =
                next_final_index_move(&self.working, working_parent, &target_children, &moved)
                    .or_else(|| {
                        // Some permutations cannot reach the n - LIS lower
                        // bound when every intermediate move must already use
                        // its final index. Place one remaining node
                        // deterministically and let the correction pass
                        // repair any displacement.
                        target_children
                            .iter()
                            .enumerate()
                            .find(|(_, guid)| moved.contains(**guid))
                            .map(|(i, guid)| ((*guid).to_owned(), i))
                    })
                    .unwrap_or_default();
            let current = self
                .working
                .by_guid
                .get(&guid)
                .copied()
                .filter(|&node| self.working.nodes[node].parent == Some(working_parent))
                .ok_or_else(|| {
                    format!("reorder child {} left its target parent", raw::quote(&guid))
                })?;
            if self.working.index_of_child(working_parent, current) != Some(target_index) {
                self.move_patch(target_guid, &guid, target_index);
                self.working
                    .move_node(current, working_parent, target_index as i64)?;
            }
            moved.remove(&guid);
        }

        // Children moving out remain until their destination parent is
        // visited. If one occupies a target prefix slot, correct that slot
        // now; this pass is empty for pure reorders and so cannot inflate
        // their LIS bound.
        let mut current_index = self.child_indices(working_parent);
        for (target_index, guid) in target_children.iter().enumerate() {
            let current = self
                .working
                .by_guid
                .get(*guid)
                .copied()
                .filter(|&node| self.working.nodes[node].parent == Some(working_parent))
                .ok_or_else(|| {
                    format!(
                        "target child {} is absent after alignment",
                        raw::quote(guid)
                    )
                })?;
            if current_index.get(*guid) == Some(&target_index) {
                continue;
            }
            self.move_patch(target_guid, guid, target_index);
            self.working
                .move_node(current, working_parent, target_index as i64)?;
            current_index = self.child_indices(working_parent);
        }

        for (&child, guid) in target.children.iter().zip(&target_children) {
            if self.inserted_roots.contains(*guid) {
                continue;
            }
            self.align(child)?;
        }
        Ok(())
    }

    fn child_indices(&self, parent: usize) -> HashMap<String, usize> {
        self.working.nodes[parent]
            .children
            .iter()
            .enumerate()
            .map(|(i, &child)| (self.working.nodes[child].guid.clone(), i))
            .collect()
    }
}

/// Chooses a remaining out-of-order child that can be moved once, straight
/// to its final index, without any later remaining move crossing it.
///
/// Each round is linear in the number of siblings; the costly case is bounded
/// in normal use by the operation threshold, while finding the LIS and the
/// common single-edit path stay O(n log n).
fn next_final_index_move(
    working: &Arena,
    parent: usize,
    target: &[&str],
    remaining: &HashSet<String>,
) -> Option<(String, usize)> {
    let children = &working.nodes[parent].children;
    let mut current_index = HashMap::with_capacity(remaining.len());
    let mut prefix = vec![0usize; children.len() + 1];
    for (i, &child) in children.iter().enumerate() {
        prefix[i + 1] = prefix[i];
        let guid = working.nodes[child].guid.as_str();
        if remaining.contains(guid) {
            current_index.insert(guid, i);
            prefix[i + 1] += 1;
        }
    }
    let mut target_rank = 0;
    for (target_index, guid) in target.iter().enumerate() {
        if !remaining.contains(*guid) {
            continue;
        }
        let at = *current_index.get(guid)?;
        // How many remaining children would sit before this one at its final
        // index, once it has been taken out of its current place.
        let before = if at < target_index {
            prefix.get(target_index + 1)? - 1
        } else {
            *prefix.get(target_index)?
        };
        if before == target_rank {
            return Some(((*guid).to_owned(), target_index));
        }
        target_rank += 1;
    }
    None
}

/// The children, in their current order, that form a longest run already in
/// target order, by patience sorting.
fn longest_increasing<'a>(
    current: &'a [String],
    target_pos: &HashMap<&str, usize>,
) -> HashSet<&'a str> {
    let mut keep = HashSet::with_capacity(current.len());
    if current.is_empty() {
        return keep;
    }
    let mut tails: Vec<usize> = Vec::with_capacity(current.len());
    let mut tail_indices: Vec<usize> = Vec::with_capacity(current.len());
    let mut previous: Vec<Option<usize>> = vec![None; current.len()];
    for (i, guid) in current.iter().enumerate() {
        let value = target_pos[guid.as_str()];
        let at = tails.partition_point(|&tail| tail < value);
        if at == tails.len() {
            tails.push(value);
            tail_indices.push(i);
        } else {
            tails[at] = value;
            tail_indices[at] = i;
        }
        if at > 0 {
            previous[i] = Some(tail_indices[at - 1]);
        }
    }
    let mut at = tail_indices.last().copied();
    while let Some(i) = at {
        keep.insert(current[i].as_str());
        at = previous[i];
    }
    keep
}

/// The `set_property` patches for one node present in both trees.
///
/// They come out deterministically: members in byte order of their names,
/// then metadata namespaces in byte order of theirs.
fn diff_properties(
    old: &Tree,
    old_node: &Node,
    new: &Tree,
    new_node: &Node,
) -> Result<Vec<Patch>, String> {
    let old_raw = old.raw(old_node);
    let new_raw = new.raw(new_node);
    if old_raw == new_raw || equal_except_structural_value(old_raw, new_raw, &old_node.kind) {
        return Ok(Vec::new());
    }
    let not_object = || "structural node is not an object".to_owned();
    let old_fields = raw::member_map(old_raw).ok_or_else(not_object)?;
    let new_fields = raw::member_map(new_raw).ok_or_else(not_object)?;

    let mut keys: Vec<&str> = old_fields
        .keys()
        .filter(|key| is_value_property(&old_node.kind, key) && *key != "metadata")
        .chain(
            new_fields
                .keys()
                .filter(|key| is_value_property(&new_node.kind, key) && *key != "metadata"),
        )
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys.dedup();

    let mut patches = Vec::new();
    let property_patch = |path: String, value: Option<&str>| Patch {
        op: Op::SetProperty,
        guid: new_node.guid.clone(),
        path,
        removed: value.is_none(),
        value: value.map(str::to_owned),
        ..Patch::default()
    };
    for key in keys {
        let old_value = old_fields.get(key).copied();
        let new_value = new_fields.get(key).copied();
        if !semantic_option_eq(old_value, new_value) {
            patches.push(property_patch(
                format!("/{}", escape_pointer_token(key)),
                new_value,
            ));
        }
    }

    let old_metadata = old_fields.get("metadata").copied();
    let new_metadata = new_fields.get("metadata").copied();
    if old_metadata.is_some() == new_metadata.is_some()
        && (old_metadata.is_none() || old_metadata == new_metadata)
    {
        return Ok(patches);
    }
    let old_namespaces = metadata_namespaces(old_metadata)?;
    let new_namespaces = metadata_namespaces(new_metadata)?;
    let mut namespaces: Vec<&str> = old_namespaces
        .keys()
        .chain(new_namespaces.keys())
        .map(String::as_str)
        .filter(|key| *key != "sync")
        .collect();
    namespaces.sort_unstable();
    namespaces.dedup();
    for key in namespaces {
        let old_value = old_namespaces.get(key).copied();
        let new_value = new_namespaces.get(key).copied();
        if !semantic_option_eq(old_value, new_value) {
            patches.push(property_patch(
                format!("/metadata/{}", escape_pointer_token(key)),
                new_value,
            ));
        }
    }
    Ok(patches)
}

fn semantic_option_eq(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => raw::semantic_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Whether two structural nodes are byte for byte the same apart from the
/// member that holds their children, which alignment has already dealt with.
fn equal_except_structural_value(a: &str, b: &str, kind: &str) -> bool {
    let key = match kind {
        "Timeline" => "tracks",
        "Stack" | "Track" => "children",
        _ => return false,
    };
    let (a, b) = (a.as_bytes(), b.as_bytes());
    match (
        raw::top_level_value_bounds(a, key),
        raw::top_level_value_bounds(b, key),
    ) {
        (Some((a_start, a_end)), Some((b_start, b_end))) => {
            a[..a_start] == b[..b_start] && a[a_end..] == b[b_end..]
        }
        _ => false,
    }
}

/// Escapes a member name as one JSON Pointer token.
pub(crate) fn escape_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Formats a number as Go's `%.3g` does, for the threshold fallback reason.
fn format_g3(value: f64) -> String {
    const PRECISION: i32 = 3;
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let scientific = format!("{:.*e}", (PRECISION - 1) as usize, value);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text.to_owned()
        }
    };
    if !(-4..PRECISION).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa), exponent.abs())
    } else {
        trim(&format!(
            "{:.*}",
            (PRECISION - 1 - exponent) as usize,
            value
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_go_percent_g() {
        assert_eq!(format_g3(0.4), "0.4");
        assert_eq!(format_g3(100.0), "100");
        assert_eq!(format_g3(0.123_456), "0.123");
        assert_eq!(format_g3(1234.0), "1.23e+03");
        assert_eq!(format_g3(0.000_012_34), "1.23e-05");
        assert_eq!(format_g3(1.0), "1");
    }

    #[test]
    fn keeps_the_longest_increasing_run() {
        let current: Vec<String> = ["c", "a", "b", "d"].map(String::from).to_vec();
        let target: HashMap<&str, usize> = [("a", 0), ("b", 1), ("c", 2), ("d", 3)].into();
        let keep = longest_increasing(&current, &target);
        assert_eq!(keep, HashSet::from(["a", "b", "d"]));
    }
}

//! Patches for a timeline in a [`Document`], rather than for timeline JSON.
//!
//! [`diff()`](crate::diff()) and [`apply()`](crate::apply()) work on a
//! timeline's JSON, as Furnace's do, so both sides make and accept the same
//! bytes. This module connects them to a timeline open in a document:
//! [`snapshot`] writes one out, [`apply_to`] applies patches to one, and a
//! [`Transaction`] turns the edits a program makes into patches.
//!
//! # Handles survive an apply
//!
//! Applying patches works out the patched timeline's JSON and reads it, then
//! puts each synced object (see [`guid`]) back into the slot of the object
//! with the same GUID, so a [`NodeId`] a program holds to the timeline, a
//! track or an item still names it afterwards, with its new contents. An
//! object the patches removed is gone, so its handle is stale; one they
//! added has a new handle, which [`GuidIndex`](crate::guid::GuidIndex) finds.
//! What travels inside an item rather than being synced on its own (its
//! markers, effects and media references) is replaced, so handles to those
//! go stale.

use std::collections::{HashMap, HashSet};

use otio_core::{Any, Document, NodeId, WriteOptions};

use crate::guid;
use crate::patch::{Error, Op, Patch, Snapshot};

fn document_error(error: &otio_core::Error) -> Error {
    Error::Document(error.to_string())
}

/// Writes `timeline` as compact OTIO JSON.
fn timeline_json(document: &Document, timeline: NodeId) -> Result<String, Error> {
    let options = WriteOptions {
        indent: None,
        ..WriteOptions::default()
    };
    otio_core::to_string_with(document, &Any::Object(timeline), &options)
        .map_err(|error| document_error(&error))
}

/// Writes `timeline` out as a snapshot of `sequence_id` at `generation`.
///
/// The objects in it are written with the GUIDs they carry; [`guid::stamp`]
/// first to give one to every object that lacks one, as a [`Transaction`]
/// does.
///
/// # Errors
///
/// [`Error::Document`] if `timeline` is not in the document or cannot be
/// written.
pub fn snapshot(
    document: &Document,
    timeline: NodeId,
    sequence_id: impl Into<String>,
    generation: i64,
) -> Result<Snapshot, Error> {
    Ok(Snapshot {
        sequence_id: sequence_id.into(),
        generation,
        timeline: timeline_json(document, timeline)?,
    })
}

/// Applies `patches` to `timeline`, which keeps its handle, as do the synced
/// objects in it that the patches keep; see the module documentation.
///
/// On failure the document is left as it was. The document's journal, if it
/// is on, records the apply as it would any edit.
///
/// # Errors
///
/// Whatever [`apply()`](crate::apply()) returns, and [`Error::Document`] if
/// `timeline` is not in the document, cannot be written, or the result
/// cannot be read back.
pub fn apply_to(document: &mut Document, timeline: NodeId, patches: &[Patch]) -> Result<(), Error> {
    if patches.is_empty() {
        return Ok(());
    }
    let before = timeline_json(document, timeline)?;
    let after = crate::apply(&before, patches)?;
    replace(document, timeline, &after)
}

/// Replaces the contents of `timeline` with the timeline `json`, keeping the
/// slot of every synced object whose GUID is in both.
fn replace(document: &mut Document, timeline: NodeId, json: &str) -> Result<(), Error> {
    let fresh = otio_core::from_str(json).map_err(|error| document_error(&error))?;
    let fresh_root = fresh
        .root()
        .ok_or_else(|| Error::Document("the patched timeline has no root".to_string()))?;

    // Everything the timeline owns now, and the slot each GUID is in.
    let old_owned = owned_from(document, timeline);
    let mut old_slots = HashMap::new();
    for id in guid::synced_objects(document, timeline) {
        if let Some(found) = guid::guid(document, id) {
            old_slots.entry(found.to_string()).or_insert(id);
        }
    }
    let parent = document.get(timeline).and_then(otio_core::Node::parent);

    // Nothing below can fail, so the document is only changed from here on.
    let arrived = document.absorb(fresh);
    let new_root = arrived[&fresh_root];
    let mut reuse = HashMap::from([(new_root, timeline)]);
    let mut taken = HashSet::from([timeline]);
    for id in guid::synced_objects(document, new_root) {
        if id == new_root {
            continue;
        }
        let Some(old) = guid::guid(document, id).and_then(|found| old_slots.get(found)) else {
            continue;
        };
        if taken.insert(*old) {
            reuse.insert(id, *old);
        }
    }

    // Move each reused object's new contents into its old slot, then point
    // every link at the slots.
    for (new, old) in &reuse {
        let node = document.remove(*new).expect("an absorbed object is live");
        *document.get_mut(*old).expect("a reused slot is live") = node;
    }
    for id in arrived.values().chain(reuse.values()) {
        if let Some(node) = document.get_mut(*id) {
            node.visit_links_mut(&mut |link| {
                if let Some(old) = reuse.get(link) {
                    *link = *old;
                }
            });
        }
    }
    if let Some(node) = document.get_mut(timeline) {
        restore_parent(node, parent);
    }

    // Drop what the timeline owned before and does not now.
    let new_owned = owned_from(document, timeline);
    for id in old_owned.difference(&new_owned) {
        document.remove(*id);
    }
    Ok(())
}

/// Every object `top` owns, however deep, and `top` itself.
fn owned_from(document: &Document, top: NodeId) -> HashSet<NodeId> {
    let mut found = HashSet::new();
    let mut pending = vec![top];
    while let Some(id) = pending.pop() {
        if !found.insert(id) {
            continue;
        }
        if let Some(node) = document.get(id) {
            node.visit_owned(&mut |owned| pending.push(owned));
        }
    }
    found
}

/// Gives `node` back the parent it had before its contents were replaced.
fn restore_parent(node: &mut otio_core::Node, parent: Option<NodeId>) {
    if let Some(item) = node.item_mut() {
        item.parent = parent;
        return;
    }
    match node {
        otio_core::Node::Transition(transition) => transition.parent = parent,
        otio_core::Node::Composable(composable) => composable.parent = parent,
        _ => {}
    }
}

/// Edits to a timeline, turned into patches when committed, or undone.
///
/// ```
/// use otio_ext_sync::document::Transaction;
///
/// let mut document = otio_core::from_str(
///     r#"{"OTIO_SCHEMA": "Timeline.1", "name": "cut",
///         "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
///             {"OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [
///                 {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"},
///                 {"OTIO_SCHEMA": "Gap.1"}, {"OTIO_SCHEMA": "Gap.1"}]}]}}"#,
/// )?;
/// let timeline = document.root().unwrap();
/// let transaction = Transaction::begin(&mut document, timeline, "main", 1)?;
/// if let Some(base) = document.get_mut(timeline).and_then(otio_core::Node::base_mut) {
///     base.name = "fine cut".to_string();
/// }
/// let patches = transaction.commit(&mut document)?;
/// assert_eq!(patches.len(), 1, "{patches:?}");
/// assert_eq!(patches[0].path, "/name");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Beginning stamps any missing GUIDs ([`guid::stamp`]) and remembers the
/// timeline as it was; committing stamps the objects the edits added and
/// diffs the two. The patches are what [`diff()`](crate::diff()) makes, so a
/// change too large for patches comes back as one
/// [`Op::ReplaceTimeline`], and an object moved between parents is a move.
#[derive(Debug, Clone)]
#[must_use = "a transaction does nothing until committed or rolled back"]
pub struct Transaction {
    timeline: NodeId,
    before: Snapshot,
}

impl Transaction {
    /// Starts a transaction on `timeline`, which is `sequence_id` at
    /// `generation`.
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if `timeline` is not in the document or cannot
    /// be written.
    pub fn begin(
        document: &mut Document,
        timeline: NodeId,
        sequence_id: impl Into<String>,
        generation: i64,
    ) -> Result<Self, Error> {
        stamp(document, timeline)?;
        Ok(Self {
            timeline,
            before: snapshot(document, timeline, sequence_id, generation)?,
        })
    }

    /// Takes up a transaction on `timeline` that began as `before`, as
    /// [`Transaction::before`] returned it, such as one begun through the C
    /// interface, which hands the snapshot to its caller to keep.
    pub const fn resume(timeline: NodeId, before: Snapshot) -> Self {
        Self { timeline, before }
    }

    /// The timeline the transaction is on.
    #[must_use]
    pub const fn timeline(&self) -> NodeId {
        self.timeline
    }

    /// The timeline as it was when the transaction began.
    #[must_use]
    pub const fn before(&self) -> &Snapshot {
        &self.before
    }

    /// Ends the transaction, returning the patches that take the timeline
    /// as it was to the timeline as it is, the next generation.
    ///
    /// An edit that changed nothing makes no patches.
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if the timeline is no longer in the document or
    /// cannot be written, and what [`diff()`](crate::diff()) returns.
    pub fn commit(self, document: &mut Document) -> Result<Vec<Patch>, Error> {
        stamp(document, self.timeline)?;
        let after = snapshot(
            document,
            self.timeline,
            self.before.sequence_id.clone(),
            self.before.generation.saturating_add(1),
        )?;
        if crate::json_eq(&self.before.timeline, &after.timeline)? {
            return Ok(Vec::new());
        }
        let (patches, _) = crate::diff(&self.before, &after)?;
        debug_assert!(
            patches.len() == 1 || patches.iter().all(|patch| patch.op != Op::ReplaceTimeline)
        );
        Ok(patches)
    }

    /// Ends the transaction, putting the timeline back as it was.
    ///
    /// Handles behave as after [`apply_to`]: the timeline and every synced
    /// object that was there when the transaction began keep theirs.
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if the timeline is no longer in the document, or
    /// what was remembered cannot be read back.
    pub fn rollback(self, document: &mut Document) -> Result<(), Error> {
        document
            .try_get(self.timeline)
            .map_err(|error| document_error(&error))?;
        replace(document, self.timeline, &self.before.timeline)
    }
}

fn stamp(document: &mut Document, timeline: NodeId) -> Result<(), Error> {
    guid::stamp(document, timeline).map_err(|error| document_error(&error))?;
    Ok(())
}

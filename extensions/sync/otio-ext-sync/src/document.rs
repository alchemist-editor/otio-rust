//! Patches for a [`Document`], rather than for timeline JSON.
//!
//! [`diff()`](crate::diff()) and [`apply()`](crate::apply()) work on a
//! timeline's JSON, as Furnace's do, so both sides make and accept the same
//! bytes. This module connects them to an open document: [`snapshot`] writes
//! one out, [`apply_to`] applies patches to one, and a [`Transaction`] turns
//! the edits a program makes into patches.
//!
//! # Handles do not survive an apply
//!
//! Applying patches here writes the document out, applies them to the JSON
//! and reads the result back, so every [`otio_core::NodeId`] a program held
//! into the document is stale afterwards; GUIDs are what stay the same, and
//! a [`GuidIndex`](crate::guid::GuidIndex) finds each object again. Applying
//! in place, keeping handles, is a later change.

use otio_core::Document;

use crate::guid;
use crate::patch::{Error, Op, Patch, Snapshot};

fn document_error(error: &otio_core::Error) -> Error {
    Error::Document(error.to_string())
}

/// Writes `document` out as a snapshot of `sequence_id` at `generation`.
///
/// # Errors
///
/// [`Error::Document`] if the document cannot be written.
pub fn snapshot(
    document: &Document,
    sequence_id: impl Into<String>,
    generation: i64,
) -> Result<Snapshot, Error> {
    Ok(Snapshot {
        sequence_id: sequence_id.into(),
        generation,
        timeline: otio_core::to_string(document).map_err(|error| document_error(&error))?,
    })
}

/// Applies `patches` to `document`, replacing its contents with the result.
///
/// Handles into the document are stale afterwards; see the module
/// documentation. If the document's journal was on, it is on again
/// afterwards, recording from the patched document. On failure the
/// document is left as it was.
///
/// # Errors
///
/// Whatever [`apply()`](crate::apply()) returns, and [`Error::Document`] if
/// the document cannot be written or the result read back.
pub fn apply_to(document: &mut Document, patches: &[Patch]) -> Result<(), Error> {
    if patches.is_empty() {
        return Ok(());
    }
    let before = otio_core::to_string(document).map_err(|error| document_error(&error))?;
    let after = crate::apply(&before, patches)?;
    let mut patched = otio_core::from_str(&after).map_err(|error| document_error(&error))?;
    if document.is_journalling() {
        patched.start_journal();
    }
    *document = patched;
    Ok(())
}

/// Edits to a document, turned into patches when committed, or undone.
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
/// let transaction = Transaction::begin(&mut document, "main", 1)?;
/// if let Some(base) = document
///     .get_mut(document.root().unwrap())
///     .and_then(otio_core::Node::base_mut)
/// {
///     base.name = "fine cut".to_string();
/// }
/// let patches = transaction.commit(&mut document)?;
/// assert_eq!(patches.len(), 1, "{patches:?}");
/// assert_eq!(patches[0].path, "/name");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// Beginning stamps any missing GUIDs ([`guid::stamp`]) and remembers the
/// document as it was; committing stamps the objects the edits added and
/// diffs the two. The patches are what [`diff()`](crate::diff()) makes, so a
/// change too large for patches comes back as one
/// [`Op::ReplaceTimeline`], and an object moved between parents is a move.
#[derive(Debug, Clone)]
#[must_use = "a transaction does nothing until committed or rolled back"]
pub struct Transaction {
    before: Snapshot,
}

impl Transaction {
    /// Starts a transaction on `document`, which is `sequence_id` at
    /// `generation`.
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if the document has no root or cannot be
    /// written.
    pub fn begin(
        document: &mut Document,
        sequence_id: impl Into<String>,
        generation: i64,
    ) -> Result<Self, Error> {
        stamp(document)?;
        Ok(Self {
            before: snapshot(document, sequence_id, generation)?,
        })
    }

    /// The document as it was when the transaction began.
    #[must_use]
    pub const fn before(&self) -> &Snapshot {
        &self.before
    }

    /// Ends the transaction, returning the patches that take the document
    /// as it was to the document as it is, the next generation.
    ///
    /// An edit that changed nothing makes no patches.
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if the document has lost its root or cannot be
    /// written, and what [`diff()`](crate::diff()) returns.
    pub fn commit(self, document: &mut Document) -> Result<Vec<Patch>, Error> {
        stamp(document)?;
        let after = snapshot(
            document,
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

    /// Ends the transaction, putting the document back as it was.
    ///
    /// Handles into the document are stale afterwards, as after
    /// [`apply_to`].
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if what was remembered cannot be read back,
    /// which would be a bug.
    pub fn rollback(self, document: &mut Document) -> Result<(), Error> {
        let mut restored =
            otio_core::from_str(&self.before.timeline).map_err(|error| document_error(&error))?;
        if document.is_journalling() {
            restored.start_journal();
        }
        *document = restored;
        Ok(())
    }
}

fn stamp(document: &mut Document) -> Result<(), Error> {
    let root = document
        .root()
        .ok_or_else(|| Error::Document("the document has no root".to_string()))?;
    guid::stamp(document, root).map_err(|error| document_error(&error))?;
    Ok(())
}

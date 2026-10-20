//! Committing a document's journalled changes, and the `document_changed`
//! hook that announces them.
//!
//! otio-core keeps the journal ([`Document::start_journal`]); it records
//! which objects any edit touched, whichever API made it. [`commit`] takes
//! what it recorded and runs the hook, so a program that syncs can attach a
//! script to it the way it attaches one to an adapter hook, in any language.

use otio_adapter::plugins;
use otio_core::{Any, AnyDictionary, Changes, Document};

use crate::guid;

/// The hook [`commit`] runs. Sync declares it when it is enabled.
pub const DOCUMENT_CHANGED: &str = "document_changed";

/// Takes the changes the document's journal recorded since it started or
/// was last committed, and runs the [`DOCUMENT_CHANGED`] hook on the
/// document's root with them.
///
/// The hook's arguments are `touched`, the GUIDs of the synced objects that
/// were added or may have changed and are still in the document, in the
/// order first touched, and `root_changed`. A removed object shows as its
/// former parent being touched. The hook runs only if something changed, the
/// document has a root, and sync is enabled.
///
/// # Errors
///
/// What a hook script returns, as [`otio_adapter::Error::Plugin`].
pub fn commit(document: &mut Document) -> Result<Changes, otio_adapter::Error> {
    let changes = document.take_changes();
    let Some(root) = document.root() else {
        return Ok(changes);
    };
    if changes.is_empty() || !plugins::registry().has_hook(DOCUMENT_CHANGED) {
        return Ok(changes);
    }
    let touched = changes
        .touched
        .iter()
        .filter_map(|id| guid::guid(document, *id))
        .map(|found| Any::String(found.to_string()))
        .collect();
    let arguments = AnyDictionary::from([
        ("touched".to_string(), Any::Vector(touched)),
        ("root_changed".to_string(), Any::Bool(changes.root_changed)),
    ]);
    plugins::run_hook(DOCUMENT_CHANGED, document, root, &arguments)?;
    // What the hook's scripts did is theirs to announce: leave it out of the
    // next commit, so a script that edits the document does not run itself
    // again.
    document.take_changes();
    Ok(changes)
}

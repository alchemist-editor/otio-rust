//! Timeline sync: sharing an OpenTimelineIO timeline between programs while
//! it is edited.
//!
//! **This is an otio-rust extension, not part of OpenTimelineIO.** It does
//! nothing until a program turns it on:
//!
//! ```
//! otio_ext_sync::install();
//! otio_ext::enable(otio_ext_sync::ID)?;
//! assert!(otio_ext::is_enabled("sync"));
//! # Ok::<(), otio_ext::Error>(())
//! ```
//!
//! What it will hold, phase by phase, is in [ADR 0011] and epic #115. So far:
//!
//! - [`guid`]: object identity through `metadata.sync.guid`.
//! - [`journal`]: committing what otio-core's change journal recorded, and
//!   the `document_changed` hook.
//! - Patches: [`diff()`] works out the patches between two snapshots of a
//!   timeline, [`apply()`] applies them, and [`patch::to_json`] and
//!   [`patch::from_json`] carry them on the wire.
//!
//! Still to come: a session with no networking of its own, and wire
//! profiles for Alchemist's Furnace and for ORI Sync.
//!
//! # Patches come from Furnace
//!
//! [`patch`], [`diff`](mod@diff) and [`apply`](mod@apply) are a port of
//! `internal/otiodiff` from Alchemist's Furnace (svc-furnace, in Go): the same
//! patch model and JSON, the same diff algorithm and fallbacks, and the same
//! checks when applying. Furnace's golden files are vendored as this crate's
//! tests, so both sides make and accept the same bytes.
//!
//! ```
//! use otio_ext_sync::{Snapshot, apply, diff, json_eq};
//!
//! let timeline = |name: &str| format!(
//!     r#"{{"OTIO_SCHEMA":"Timeline.1","metadata":{{"sync":{{"guid":"t"}}}},
//!         "tracks":{{"OTIO_SCHEMA":"Stack.1","metadata":{{"sync":{{"guid":"s"}}}},
//!         "children":[{{"OTIO_SCHEMA":"Track.1","metadata":{{"sync":{{"guid":"v1"}}}},
//!         "name":"{name}","children":[]}}]}}}}"#
//! );
//! let old = Snapshot { sequence_id: "main".into(), generation: 1, timeline: timeline("V1") };
//! let new = Snapshot { sequence_id: "main".into(), generation: 2, timeline: timeline("Video") };
//!
//! let (patches, stats) = diff(&old, &new)?;
//! assert_eq!(
//!     otio_ext_sync::patch::to_json(&patches),
//!     r#"[{"op":"set_property","guid":"v1","path":"/name","value":"Video"}]"#
//! );
//! assert!(!stats.fell_back);
//! assert!(json_eq(&apply(&old.timeline, &patches)?, &new.timeline)?);
//! # Ok::<(), otio_ext_sync::Error>(())
//! ```
//!
//! [ADR 0011]: https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0011-timeline-sync.md

pub mod apply;
pub mod diff;
pub mod guid;
pub mod journal;
pub mod patch;
mod raw;
mod tree;

pub use apply::apply;
pub use diff::{Tree, diff, diff_trees, diff_with_options};
pub use patch::{Error, Op, Options, Patch, Snapshot, Stats};

use otio_ext::Extension;

/// Compares two JSON documents as Furnace compares timelines: object members
/// in any order, array elements in order, and numbers by the text they were
/// written as, so `1` and `1.0` differ and integers of any size compare
/// exactly.
///
/// This is how to check what [`apply()`] returns against the timeline the
/// patches were made from; their bytes can differ.
///
/// # Errors
///
/// Returns [`Error::InvalidJson`] if either document is not JSON.
pub fn json_eq(a: &str, b: &str) -> Result<bool, Error> {
    if !raw::is_valid(a) {
        return Err(Error::InvalidJson("first document"));
    }
    if !raw::is_valid(b) {
        return Err(Error::InvalidJson("second document"));
    }
    Ok(raw::semantic_eq(a, b))
}

/// The id a program enables sync by.
pub const ID: &str = "sync";

/// The sync extension.
pub struct Sync;

/// The one instance [`install`] hands to the framework.
pub static SYNC: Sync = Sync;

impl Extension for Sync {
    fn id(&self) -> &'static str {
        ID
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn summary(&self) -> &'static str {
        "Timeline sync: share an OpenTimelineIO timeline between programs while it is edited."
    }

    // Sync adds no schemas: its only footprint in a file is
    // `metadata.sync.guid`, which is ordinary metadata.

    fn register(&self) -> Result<(), String> {
        otio_adapter::plugins::registry().declare_hook(journal::DOCUMENT_CHANGED);
        Ok(())
    }
}

/// Makes sync available to [`otio_ext::enable`]. Calling it again does
/// nothing.
pub fn install() {
    // Sync claims no schemas and its id is its own, so installing it cannot
    // conflict with anything but a second, different extension calling
    // itself `sync`, which is a bug in that extension.
    otio_ext::install(&SYNC).expect("another extension is installed as `sync`");
}

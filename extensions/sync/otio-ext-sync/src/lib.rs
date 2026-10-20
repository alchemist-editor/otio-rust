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
//!
//! Still to come: a session with no networking of its own, and wire
//! profiles for Alchemist's Furnace and for ORI Sync.
//!
//! [ADR 0011]: https://github.com/alchemist-editor/otio-rust/blob/main/docs/adr/0011-timeline-sync.md

pub mod guid;
pub mod journal;

use otio_ext::Extension;

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

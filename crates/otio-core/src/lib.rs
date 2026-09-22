//! The OpenTimelineIO data model in Rust.
//!
//! A [`Document`] owns every object in a timeline. Objects are reached through
//! [`NodeId`] handles rather than pointers: see
//! `docs/adr/0001-ownership-model.md` for why.
//!
//! ```
//! use otio_core::{Document, Node};
//!
//! let json = r#"{
//!     "OTIO_SCHEMA": "Timeline.1",
//!     "name": "my cut",
//!     "tracks": { "OTIO_SCHEMA": "Stack.1", "children": [] }
//! }"#;
//!
//! let document = otio_core::from_str(json)?;
//! let root = document.root().expect("a parsed document has a root");
//! assert_eq!(document.try_get(root)?.name(), "my cut");
//!
//! // Writing it back out produces canonical OTIO JSON.
//! let rewritten = otio_core::to_string(&document)?;
//! assert!(rewritten.contains("\"OTIO_SCHEMA\": \"Timeline.1\""));
//! # Ok::<(), otio_core::Error>(())
//! ```
//!
//! # Compatibility
//!
//! Schema names and versions match upstream OpenTimelineIO 0.19.0. Two
//! properties are held deliberately:
//!
//! - An object whose schema is not recognized is preserved verbatim as
//!   [`schema::UnknownSchema`], so third-party plugin data survives a read and
//!   rewrite.
//! - A field absent from the input takes upstream's default rather than
//!   failing, so files written by older versions still read.
//!
//! Objects written by older releases are upgraded on read, so a `Clip.1`'s
//! single `media_reference` becomes a `Clip.2`'s `media_references` and a
//! `Marker.2`'s colour name becomes a `Marker.3`'s colour. The reverse works
//! too: [`to_string_with`] takes the schema versions an older release knows
//! and downgrades each object on the way out. Both directions run through
//! [`registry`], which a program can extend with schemas and version
//! functions of its own, as upstream's `TypeRegistry` can be.

pub mod algorithm;
mod arena;
mod clone;
pub mod composition;
mod deserialize;
mod dtoa;
pub mod edit;
mod error;
pub mod json;
pub mod registry;
pub mod schema;
mod serialize;
pub mod upgrade;
mod value;

pub use arena::{Document, NodeId};
pub use composition::NeighborGapPolicy;
pub use deserialize::{from_str, from_str_any};
pub use error::{Error, Result};
pub use schema::{Node, TRACK_KIND_AUDIO, TRACK_KIND_VIDEO};
pub use serialize::{
    DEFAULT_INDENT, WriteOptions, to_string, to_string_any_pretty, to_string_pretty,
    to_string_pretty_from, to_string_with,
};
pub use upgrade::DEFAULT_MEDIA_KEY;
pub use value::{Any, AnyDictionary, Box2d, Color, V2d};

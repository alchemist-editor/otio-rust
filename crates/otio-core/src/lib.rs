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
//! `Marker.2`'s colour name becomes a `Marker.3`'s colour. See
//! [`upgrade`]. The reverse direction, writing a document targeted at an older
//! release, is not implemented yet.

pub mod algorithm;
mod arena;
pub mod composition;
mod deserialize;
pub mod edit;
mod error;
pub mod json;
pub mod schema;
mod serialize;
pub mod upgrade;
mod value;

pub use arena::{Document, NodeId};
pub use composition::NeighborGapPolicy;
pub use deserialize::from_str;
pub use error::{Error, Result};
pub use schema::Node;
pub use serialize::{DEFAULT_INDENT, to_string, to_string_pretty, to_string_pretty_from};
pub use value::{Any, AnyDictionary, Box2d, Color, V2d};

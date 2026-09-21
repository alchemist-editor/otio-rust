//! A Rust implementation of AAF, the Advanced Authoring Format.
//!
//! AAF is the interchange format professional editing applications use to move
//! sequences, and often the media itself, between each other. It is the format
//! behind an Avid bin export, and the one OpenTimelineIO's AAF adapter reads.
//!
//! This is a port of [`pyaaf2`](https://github.com/markreidvfx/pyaaf2), the
//! library OpenTimelineIO's AAF adapter is built on. It is a full
//! reimplementation: there is no Python involved at any stage.
//!
//! # What is here so far
//!
//! [`cfb`], the Microsoft Compound File Binary container an AAF file is stored
//! in. The AAF object model, the type definitions and the write path are still
//! to come.
//!
//! [`Auid`] is here already, because the container carries AAF class
//! identifiers on its storage entries.

mod auid;

pub mod cfb;

pub use auid::{Auid, ParseAuidError};

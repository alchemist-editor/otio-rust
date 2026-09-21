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
//! in, and on top of it the object tree: [`AafFile`] reads a file as
//! [`Object`]s, each with a class and the [`property`] values stored against
//! it, and follows the references that make the file a tree.
//!
//! What is not here yet is the meta dictionary, which holds the class, type
//! and property definitions that say what a property value *means*. Until
//! that lands, a property's data is bytes. The write path is still to come
//! too.
//!
//! # Example
//!
//! ```no_run
//! use std::fs::File;
//! use aaf::AafFile;
//!
//! let mut file = AafFile::open(File::open("example.aaf")?)?;
//! let root = file.root()?;
//!
//! for (pid, child) in file.children(&root)? {
//!     println!("property {pid:#06x} owns a {}", child.class_id());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod auid;
mod error;
mod metadict;
mod mob_id;
mod object;
mod utf16;
mod value;

pub mod cfb;
pub mod property;

pub use auid::{Auid, ParseAuidError};
pub use error::{Error, Result};
pub use metadict::{ClassDef, MetaDictionary, PropertyDef, TypeDef, TypeKind};
pub use mob_id::{MobId, ParseMobIdError};
pub use object::{AafFile, Object};
pub use value::Value;

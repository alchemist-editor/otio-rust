//! The trait a file-format adapter implements.
//!
//! An adapter turns a file of some editorial interchange format into an
//! [`otio_core::Document`] and back. Upstream OpenTimelineIO expresses this as
//! a plugin with four loosely specified Python entry points and a dictionary
//! of keyword arguments; here it is one trait, with each adapter's options
//! named and typed.
//!
//! ```no_run
//! use otio_adapter::{Adapter, TextAdapter};
//! # use otio_adapter::Result;
//! # fn example<A: TextAdapter>(input: &str) -> Result<String> {
//! let document = A::read_from_str(input, &A::ReadOptions::default())?;
//! A::write_to_string(&document, &A::WriteOptions::default())
//! # }
//! ```
//!
//! # Why options are an associated type
//!
//! Formats differ in what a caller can ask for, and the differences are not
//! cosmetic: an EDL is written in one of three dialects that real systems
//! disagree about, and ALE needs a frame rate that the file itself may not
//! state. Upstream passes these as `**adapter_argument_map`, so a misspelled
//! argument is silently ignored and an argument meant for one adapter reaches
//! another. Naming each adapter's options as a type moves that to compile
//! time, and gives the bindings something concrete to expose.
//!
//! Every options type implements [`Default`], so a caller who wants the
//! format's usual behaviour writes `&Default::default()` and stops there.
//!
//! # Bytes, and text
//!
//! [`Adapter`] is stated over bytes, because AAF is a binary container and a
//! trait that only spoke `&str` would leave it out. The formats that really
//! are text also implement [`TextAdapter`], whose methods skip the encoding
//! step; [`Adapter`] for those is a UTF-8 decode and a delegation.

mod error;

pub mod cdl;

use std::path::Path;

use otio_core::Document;

pub use error::{Error, Result};

/// Something that reads and writes one interchange format.
pub trait Adapter {
    /// The options this adapter accepts when reading.
    type ReadOptions: Default;

    /// The options this adapter accepts when writing.
    type WriteOptions: Default;

    /// The adapter's name, matching upstream's plugin manifest.
    const NAME: &'static str;

    /// The filename suffixes this adapter claims, lowercase, without a dot.
    const SUFFIXES: &'static [&'static str];

    /// Reads a document from the bytes of a file in this format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] if the input is not valid for this format, and
    /// [`Error::Encoding`] if a text format's bytes are not UTF-8.
    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document>;

    /// Writes a document as the bytes of a file in this format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Unsupported`] if the document holds something the
    /// format cannot express.
    fn write_to_bytes(document: &Document, options: &Self::WriteOptions) -> Result<Vec<u8>>;

    /// Reads a document from a file on disk.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be read, and otherwise
    /// whatever [`Adapter::read_from_bytes`] returns.
    fn read_from_file(path: impl AsRef<Path>, options: &Self::ReadOptions) -> Result<Document> {
        Self::read_from_bytes(&std::fs::read(path)?, options)
    }

    /// Writes a document to a file on disk.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be written, and otherwise
    /// whatever [`Adapter::write_to_bytes`] returns.
    fn write_to_file(
        document: &Document,
        path: impl AsRef<Path>,
        options: &Self::WriteOptions,
    ) -> Result<()> {
        std::fs::write(path, Self::write_to_bytes(document, options)?)?;
        Ok(())
    }
}

/// An adapter whose format is text rather than a binary container.
///
/// These are the formats a person can open in an editor: ALE, EDL, and the
/// two FCP XML flavours. Reading one from a `&str` skips the decoding step,
/// and writing one to a `String` skips re-checking bytes this library just
/// produced.
pub trait TextAdapter: Adapter {
    /// Reads a document from the text of a file in this format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] if the input is not valid for this format.
    fn read_from_str(input: &str, options: &Self::ReadOptions) -> Result<Document>;

    /// Writes a document as the text of a file in this format.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Unsupported`] if the document holds something the
    /// format cannot express.
    fn write_to_string(document: &Document, options: &Self::WriteOptions) -> Result<String>;
}

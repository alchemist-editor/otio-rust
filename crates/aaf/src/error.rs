//! Errors produced while reading an AAF file's objects.

use std::fmt;

use crate::cfb;

/// Shorthand for a result carrying an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// An error produced while reading an AAF file's objects.
///
/// Problems with the container itself arrive as [`Error::Cfb`]; everything
/// else here is a file whose container is sound but whose AAF content is not.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The compound file underneath could not be read.
    Cfb(cfb::Error),

    /// A property stream ended before the value it promised.
    TruncatedProperty {
        /// How many bytes the stream said it had.
        wanted: usize,
        /// How many it had.
        found: usize,
    },

    /// An index stream ended before the entries it promised.
    TruncatedIndex {
        /// How many bytes the stream said it had.
        wanted: usize,
        /// How many it had.
        found: usize,
    },

    /// A stream declares a byte order this reader does not support.
    ///
    /// Property streams are written little-endian (`0x4C`) and the format
    /// allows big-endian (`0x42`), which nothing writes. A data stream's own
    /// contents are marked unspecified (`0x55`).
    UnsupportedByteOrder {
        /// The declared byte order mark.
        mark: u8,
    },

    /// A reference key is neither 16 nor 32 bytes.
    BadKeySize {
        /// The declared key size, in bytes.
        size: u8,
    },

    /// A reference names something that is not in the file.
    MissingEntry {
        /// The name that was looked up.
        name: String,
        /// The storage it was looked up in.
        parent: String,
    },

    /// A collection's index stream is missing.
    ///
    /// A collection property names its members only in its index, so without
    /// it there is no way to find them.
    MissingIndex {
        /// The index stream's name.
        name: String,
        /// The object whose property it belongs to.
        parent: String,
    },

    /// A definition in the meta dictionary is missing a property it needs.
    MissingDefinitionProperty {
        /// The definition, named as far as it could be read.
        definition: String,
        /// The identifier of the property that is not there.
        pid: u16,
    },

    /// A property declares a type the meta dictionary does not define.
    UndefinedType {
        /// The type that is not defined.
        type_id: crate::Auid,
    },

    /// A type has no fixed size, so it cannot be a record member or an
    /// array element.
    UnsizedType {
        /// The type in question.
        type_id: crate::Auid,
    },

    /// A value's bytes are the wrong length for the type it declares.
    WrongValueSize {
        /// The type the value declares.
        type_id: crate::Auid,
        /// How many bytes that type needs.
        wanted: usize,
        /// How many the value has.
        found: usize,
    },

    /// A type nests deeper than this reader will follow.
    ///
    /// Types refer to each other by identifier, so a file can describe a
    /// record that contains itself. Nothing in the format forbids it and
    /// nothing sensible can be read from it.
    TypeTooDeep {
        /// The type the walk gave up on.
        type_id: crate::Auid,
    },

    /// A property was asked to resolve a reference it does not hold.
    NotAReference {
        /// The property's identifier.
        pid: u16,
        /// The kind of reference the caller wanted.
        expected: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cfb(e) => write!(f, "{e}"),
            Self::TruncatedProperty { wanted, found } => write!(
                f,
                "property stream is {found} bytes but its entries need {wanted}"
            ),
            Self::TruncatedIndex { wanted, found } => write!(
                f,
                "index stream is {found} bytes but its entries need {wanted}"
            ),
            Self::UnsupportedByteOrder { mark } => {
                write!(f, "unsupported byte order mark: {mark:#04x}")
            }
            Self::BadKeySize { size } => {
                write!(
                    f,
                    "unsupported reference key size: {size}, expected 16 or 32"
                )
            }
            Self::MissingEntry { name, parent } => {
                write!(f, "'{parent}' has nothing named '{name}'")
            }
            Self::MissingIndex { name, parent } => {
                write!(f, "'{parent}' has no index stream '{name}'")
            }
            Self::MissingDefinitionProperty { definition, pid } => write!(
                f,
                "{definition} is missing the property {pid:#06x} it needs to be read"
            ),
            Self::UndefinedType { type_id } => {
                write!(f, "the meta dictionary does not define the type {type_id}")
            }
            Self::UnsizedType { type_id } => write!(
                f,
                "the type {type_id} has no fixed size, so it cannot be stored inline"
            ),
            Self::WrongValueSize {
                type_id,
                wanted,
                found,
            } => write!(
                f,
                "a value of type {type_id} needs {wanted} bytes but has {found}"
            ),
            Self::TypeTooDeep { type_id } => {
                write!(f, "the type {type_id} nests too deeply to read")
            }
            Self::NotAReference { pid, expected } => {
                write!(f, "property {pid:#06x} is not {expected}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cfb(e) => Some(e),
            _ => None,
        }
    }
}

impl From<cfb::Error> for Error {
    fn from(e: cfb::Error) -> Self {
        Self::Cfb(e)
    }
}

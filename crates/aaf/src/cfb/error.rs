//! Errors produced while reading a compound file.

use std::fmt;
use std::io;

/// Shorthand for a result carrying an [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

/// An error produced while reading a compound file.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The underlying reader failed.
    Io(io::Error),

    /// The file does not start with the compound file signature.
    ///
    /// Every compound file begins with the eight bytes
    /// `D0 CF 11 E0 A1 B1 1A E1`.
    BadSignature {
        /// The first eight bytes of the file, as found.
        found: [u8; 8],
    },

    /// The header declares an unsupported sector size.
    ///
    /// The format allows any power of two, but only 512 (version 3) and 4096
    /// (version 4) occur in practice, and those are what AAF writes.
    UnsupportedSectorSize {
        /// The declared sector size, in bytes.
        size: u32,
    },

    /// The header declares a mini sector size other than 64 bytes.
    UnsupportedMiniSectorSize {
        /// The declared mini sector size, in bytes.
        size: u32,
    },

    /// The header declares a byte order other than little-endian.
    UnsupportedByteOrder {
        /// The declared byte order mark; little-endian files carry `0xFFFE`.
        mark: u16,
    },

    /// A sector chain loops back on itself.
    ///
    /// A well-formed chain ends at `ENDOFCHAIN`. A loop would make a stream
    /// read run forever, so it is rejected as soon as it is detected.
    CyclicChain {
        /// Whether the loop is in the mini FAT rather than the FAT.
        mini: bool,
        /// The sector the chain started at.
        start: u32,
    },

    /// A chain refers to a sector past the end of the allocation table.
    SectorOutOfRange {
        /// The out-of-range sector number.
        sector: u32,
        /// The number of entries the allocation table has.
        table_len: u32,
    },

    /// A directory entry refers to a directory entry that does not exist.
    DirEntryOutOfRange {
        /// The out-of-range directory entry number.
        id: u32,
        /// The number of directory entries the file has.
        count: u32,
    },

    /// The directory tree is not a tree: an entry is reachable twice.
    ///
    /// Directory children are held in a red-black tree, so a cycle or a shared
    /// subtree would make a listing run forever or repeat itself.
    CorruptDirectoryTree {
        /// The directory entry reached more than once.
        id: u32,
    },

    /// The file has no root directory entry, or its first entry is not one.
    MissingRootEntry,

    /// A directory entry's name is longer than the 64-byte field allows.
    BadDirEntryName {
        /// The directory entry carrying the bad name.
        id: u32,
        /// The declared name length, in bytes.
        len: u16,
    },

    /// A directory entry claims more bytes than its sector chain holds.
    ///
    /// The length on the entry and the chain in the allocation table are two
    /// statements of the same fact, and this file's disagree. Believing the
    /// entry would mean reading sectors that were never allocated.
    StreamLongerThanChain {
        /// The directory entry making the claim.
        id: u32,
        /// The length the entry declares, in bytes.
        declared: u64,
        /// The number of bytes the entry's sector chain actually holds.
        allocated: u64,
    },

    /// An operation that needs a storage was given a stream, or the reverse.
    WrongEntryType {
        /// The directory entry.
        id: u32,
        /// The kind of entry the operation needed.
        expected: &'static str,
    },

    /// A writer was asked to create an entry whose name is already taken.
    EntryExists {
        /// The path of the entry that already exists.
        path: String,
    },

    /// A writer was asked to create an entry with an unusable name.
    ///
    /// A directory entry holds at most 32 UTF-16 code units of name.
    BadName {
        /// The name, as given.
        name: String,
    },

    /// A writer reached a state the layout it reproduces cannot represent.
    ///
    /// The writer follows pyaaf2's allocation and directory algorithms step
    /// for step, and pyaaf2 raises at these same points: a storage with more
    /// children than the directory can index, a mini stream that would need
    /// to grow by more than one sector at once, a red-black tree insertion
    /// that loses its footing. None of them happen for a well-formed series
    /// of operations.
    Unrepresentable {
        /// What went wrong, in a few words.
        what: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::BadSignature { found } => {
                write!(f, "not a compound file: signature is {found:02x?}")
            }
            Self::UnsupportedSectorSize { size } => {
                write!(f, "unsupported sector size: {size}, expected 512 or 4096")
            }
            Self::UnsupportedMiniSectorSize { size } => {
                write!(f, "unsupported mini sector size: {size}, expected 64")
            }
            Self::UnsupportedByteOrder { mark } => {
                write!(
                    f,
                    "unsupported byte order mark: {mark:#06x}, expected 0xfffe"
                )
            }
            Self::CyclicChain { mini, start } => {
                let table = if *mini { "mini FAT" } else { "FAT" };
                write!(f, "cyclic {table} chain starting at sector {start}")
            }
            Self::SectorOutOfRange { sector, table_len } => write!(
                f,
                "sector {sector} is past the end of the allocation table ({table_len} entries)"
            ),
            Self::DirEntryOutOfRange { id, count } => write!(
                f,
                "directory entry {id} does not exist ({count} entries in the file)"
            ),
            Self::CorruptDirectoryTree { id } => {
                write!(f, "corrupt directory tree: entry {id} is reachable twice")
            }
            Self::MissingRootEntry => write!(f, "the file has no root directory entry"),
            Self::BadDirEntryName { id, len } => write!(
                f,
                "directory entry {id} declares a {len} byte name, the field holds at most 64"
            ),
            Self::StreamLongerThanChain {
                id,
                declared,
                allocated,
            } => write!(
                f,
                "directory entry {id} claims {declared} bytes but its chain holds {allocated}"
            ),
            Self::WrongEntryType { id, expected } => {
                write!(f, "directory entry {id} is not a {expected}")
            }
            Self::EntryExists { path } => write!(f, "{path} already exists"),
            Self::BadName { name } => write!(
                f,
                "'{name}' cannot name a directory entry: at most 32 UTF-16 code units fit"
            ),
            Self::Unrepresentable { what } => {
                write!(f, "the compound file cannot be laid out: {what}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::Io(e) => e,
            other => io::Error::new(io::ErrorKind::InvalidData, other),
        }
    }
}

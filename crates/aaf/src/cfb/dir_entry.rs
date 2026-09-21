//! Directory entries: the named storages and streams inside a compound file.

use std::cmp::Ordering;
use std::fmt;

use super::error::{Error, Result};
use super::sector::{self, SectorId};
use crate::Auid;

/// The size of a directory entry on disk, in bytes.
pub(crate) const DIR_ENTRY_LEN: usize = 128;

/// The directory entry number of the root storage.
pub const ROOT_ID: DirId = DirId(0);

/// The largest value that is a directory entry number rather than a marker.
const MAX_REGULAR_ID: u32 = 0xffff_fffa;

/// A directory entry number.
///
/// Directory entries live in a flat array on disk and refer to each other by
/// index, so this is a typed index into that array rather than a pointer. It
/// is only meaningful against the file it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DirId(pub(crate) u32);

impl DirId {
    /// The entry's number, as it appears on disk.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for DirId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What a directory entry names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntryType {
    /// An unused slot in the directory array.
    Empty,
    /// A storage: a directory, holding other entries.
    Storage,
    /// A stream: a file, holding bytes.
    Stream,
    /// A lock-bytes entry. Not used by AAF.
    LockBytes,
    /// A property entry. Not used by AAF.
    Property,
    /// The root storage, which also owns the mini stream.
    RootStorage,
    /// A type byte the format does not define.
    Unknown(u8),
}

impl EntryType {
    fn from_byte(b: u8) -> Self {
        match b {
            0x00 => Self::Empty,
            0x01 => Self::Storage,
            0x02 => Self::Stream,
            0x03 => Self::LockBytes,
            0x04 => Self::Property,
            0x05 => Self::RootStorage,
            other => Self::Unknown(other),
        }
    }

    /// Whether entries can be nested inside this one.
    #[must_use]
    pub const fn is_storage(self) -> bool {
        matches!(self, Self::Storage | Self::RootStorage)
    }

    /// Whether this entry holds stream bytes.
    #[must_use]
    pub const fn is_stream(self) -> bool {
        matches!(self, Self::Stream)
    }
}

/// The colour of a directory entry in its parent's red-black tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// A red node.
    Red,
    /// A black node.
    Black,
}

/// One entry in a compound file's directory.
///
/// Entries form a tree of storages and streams, but not by holding a list of
/// children: each storage points at the root of a red-black tree of its
/// children, and each child points at its own left and right siblings. Use
/// [`CompoundFile::children`] rather than walking the links by hand.
///
/// [`CompoundFile::children`]: super::CompoundFile::children
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub(crate) id: DirId,
    pub(crate) name: String,
    pub(crate) entry_type: EntryType,
    pub(crate) color: Color,
    pub(crate) left: Option<DirId>,
    pub(crate) right: Option<DirId>,
    pub(crate) child: Option<DirId>,
    pub(crate) class_id: Option<Auid>,
    pub(crate) state_bits: u32,
    pub(crate) created: u64,
    pub(crate) modified: u64,
    pub(crate) start_sector: Option<SectorId>,
    pub(crate) stream_len: u64,
}

impl DirEntry {
    /// This entry's number.
    #[must_use]
    pub const fn id(&self) -> DirId {
        self.id
    }

    /// This entry's name, without its parent's path.
    ///
    /// The root storage is conventionally named `Root Entry`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What this entry names.
    #[must_use]
    pub const fn entry_type(&self) -> EntryType {
        self.entry_type
    }

    /// Whether entries can be nested inside this one.
    #[must_use]
    pub const fn is_storage(&self) -> bool {
        self.entry_type.is_storage()
    }

    /// Whether this entry holds stream bytes.
    #[must_use]
    pub const fn is_stream(&self) -> bool {
        self.entry_type.is_stream()
    }

    /// Whether this is the root storage.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        matches!(self.entry_type, EntryType::RootStorage)
    }

    /// This entry's colour in its parent's red-black tree.
    #[must_use]
    pub const fn color(&self) -> Color {
        self.color
    }

    /// The class of the object stored here, if one is recorded.
    ///
    /// AAF puts the class AUID of every object on its storage entry, so this
    /// identifies an object's class without reading its properties.
    #[must_use]
    pub const fn class_id(&self) -> Option<Auid> {
        self.class_id
    }

    /// The user flags on this entry. AAF leaves these at zero.
    #[must_use]
    pub const fn state_bits(&self) -> u32 {
        self.state_bits
    }

    /// The creation time, as a Windows `FILETIME`.
    ///
    /// This is 100-nanosecond intervals since 1601-01-01 UTC. AAF writers
    /// generally leave it at zero.
    #[must_use]
    pub const fn created(&self) -> u64 {
        self.created
    }

    /// The modification time, as a Windows `FILETIME`.
    #[must_use]
    pub const fn modified(&self) -> u64 {
        self.modified
    }

    /// The length of this entry's stream, in bytes.
    ///
    /// On the root storage this is the length of the mini stream, which is not
    /// a stream a caller can read.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.stream_len
    }

    /// Whether this entry's stream is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.stream_len == 0
    }

    /// The first sector of this entry's stream, if it has one.
    #[must_use]
    pub const fn start_sector(&self) -> Option<SectorId> {
        self.start_sector
    }

    /// Parses one 128-byte directory entry.
    pub(crate) fn parse(id: DirId, data: &[u8]) -> Result<Self> {
        debug_assert_eq!(data.len(), DIR_ENTRY_LEN);

        let entry_type = EntryType::from_byte(data[66]);

        // Unused slots are not always zeroed, so a nonsense length in one is
        // not a broken file. In a slot that is in use it is.
        let name_len = u16(data, 64);
        let name = if name_len > 64 {
            if entry_type != EntryType::Empty {
                return Err(Error::BadDirEntryName {
                    id: id.0,
                    len: name_len,
                });
            }
            String::new()
        } else {
            crate::utf16::decode_le(&data[..name_len as usize])
        };

        let class_id =
            Auid::from_bytes_le(data[80..96].try_into().expect("slice is sixteen bytes"));
        let start_sector = u32(data, 116);

        Ok(Self {
            id,
            name,
            entry_type,
            color: if data[67] == 0x01 {
                Color::Black
            } else {
                Color::Red
            },
            left: dir_id(u32(data, 68)),
            right: dir_id(u32(data, 72)),
            child: dir_id(u32(data, 76)),
            class_id: (!class_id.is_nil()).then_some(class_id),
            state_bits: u32(data, 96),
            created: u64(data, 100),
            modified: u64(data, 108),
            start_sector: sector::is_regular(start_sector).then_some(start_sector),
            stream_len: u64(data, 120),
        })
    }
}

/// Orders two directory entry names the way the format's red-black tree does.
///
/// Shorter names always sort before longer ones, and names of the same length
/// are compared case-insensitively. This is not lexicographic order, so a
/// listing sorted by this function will not look alphabetical.
///
/// # Example
///
/// ```
/// use std::cmp::Ordering;
/// use aaf::cfb::cmp_names;
///
/// assert_eq!(cmp_names("zz", "aaa"), Ordering::Less);
/// assert_eq!(cmp_names("Mobs", "mobs"), Ordering::Equal);
/// ```
#[must_use]
pub fn cmp_names(a: &str, b: &str) -> Ordering {
    // The format counts UTF-16 code units, which is what the name field holds.
    let a_units = a.encode_utf16().count();
    let b_units = b.encode_utf16().count();
    a_units.cmp(&b_units).then_with(|| {
        a.chars()
            .flat_map(char::to_uppercase)
            .cmp(b.chars().flat_map(char::to_uppercase))
    })
}

fn dir_id(raw: u32) -> Option<DirId> {
    (raw <= MAX_REGULAR_ID).then_some(DirId(raw))
}

fn u16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().expect("slice is two bytes"))
}

fn u32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("slice is four bytes"))
}

fn u64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().expect("slice is eight bytes"))
}

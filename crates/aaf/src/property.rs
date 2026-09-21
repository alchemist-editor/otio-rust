//! The `properties` stream: how an AAF object's property values are stored.
//!
//! Every AAF object is a storage in the compound file, and every one of them
//! holds a stream named `properties`. That stream is a short header, a table
//! of fixed-size entries, and then the values themselves back to back:
//!
//! ```text
//! byte order (1)  version (1)  entry count (2)
//! entry 0: pid (2)  format (2)  length (2)
//! entry 1: ...
//! value 0 bytes, value 1 bytes, ...
//! ```
//!
//! A value's meaning depends on its [`PropertyFormat`]. Plain data is stored
//! inline; a reference to another object is stored as the *name* of the
//! storage holding it, so following a reference means looking that name up in
//! the compound file. Collections keep their members in a sibling stream, the
//! index, named after the property.
//!
//! Nothing here interprets a [`PropertyFormat::Data`] value. Knowing that some
//! bytes are an `int32` or a `Rational` needs the file's own type definitions,
//! which is the layer above this one.

use crate::error::{Error, Result};
use crate::utf16::decode_le;
use crate::{Auid, MobId};

/// The property stream's own header, before the entry table.
const STREAM_HEADER_LEN: usize = 4;

/// Each entry in the table is a pid, a format and a length.
const ENTRY_LEN: usize = 6;

/// The byte order mark a little-endian property stream carries.
const LITTLE_ENDIAN: u8 = 0x4c;

/// The byte a stream property's name is prefixed with.
const STREAM_UNSPECIFIED_ENDIAN: u8 = 0x55;

/// How a property's value is stored.
///
/// The codes are the ones the format writes. Three of them — the two data
/// collections and the stored-object-id weak reference — are defined by the
/// specification but do not appear in files anyone writes; they are here so an
/// unexpected file is described rather than rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PropertyFormat {
    /// A value stored inline, to be read with the property's type definition.
    Data,
    /// A stream, stored elsewhere in the compound file under a name.
    DataStream,
    /// A reference to an object this one owns.
    StrongRef,
    /// An ordered collection of owned objects.
    StrongRefVector,
    /// An unordered collection of owned objects, keyed by a unique property.
    StrongRefSet,
    /// A reference to an object owned by something else.
    WeakRef,
    /// An ordered collection of references to objects owned elsewhere.
    WeakRefVector,
    /// An unordered collection of references to objects owned elsewhere.
    WeakRefSet,
    /// A weak reference stored as an object id. Specified but unused.
    WeakRefStoredObjectId,
    /// A unique object id. Specified but unused.
    UniqueObjectId,
    /// An opaque stream. Specified but unused.
    OpaqueStream,
    /// An ordered collection of inline values. Specified but unused.
    DataVector,
    /// An unordered collection of inline values. Specified but unused.
    DataSet,
    /// A code the specification does not define.
    Unknown(u16),
}

impl PropertyFormat {
    /// Reads a format code as it appears in a property stream.
    #[must_use]
    pub const fn from_code(code: u16) -> Self {
        match code {
            0x82 => Self::Data,
            0x42 => Self::DataStream,
            0x22 => Self::StrongRef,
            0x32 => Self::StrongRefVector,
            0x3a => Self::StrongRefSet,
            0x02 => Self::WeakRef,
            0x12 => Self::WeakRefVector,
            0x1a => Self::WeakRefSet,
            0x03 => Self::WeakRefStoredObjectId,
            0x86 => Self::UniqueObjectId,
            0x40 => Self::OpaqueStream,
            0xd2 => Self::DataVector,
            0xda => Self::DataSet,
            other => Self::Unknown(other),
        }
    }

    /// The format code as it appears on disk.
    #[must_use]
    pub const fn to_code(self) -> u16 {
        match self {
            Self::Data => 0x82,
            Self::DataStream => 0x42,
            Self::StrongRef => 0x22,
            Self::StrongRefVector => 0x32,
            Self::StrongRefSet => 0x3a,
            Self::WeakRef => 0x02,
            Self::WeakRefVector => 0x12,
            Self::WeakRefSet => 0x1a,
            Self::WeakRefStoredObjectId => 0x03,
            Self::UniqueObjectId => 0x86,
            Self::OpaqueStream => 0x40,
            Self::DataVector => 0xd2,
            Self::DataSet => 0xda,
            Self::Unknown(code) => code,
        }
    }

    /// Whether this format names objects this one owns.
    ///
    /// Owned objects live in storages nested inside this object's own, so
    /// these are the references that make the file a tree.
    #[must_use]
    pub const fn is_strong(self) -> bool {
        matches!(
            self,
            Self::StrongRef | Self::StrongRefVector | Self::StrongRefSet
        )
    }
}

/// What a reference points at.
///
/// Collections are keyed by whichever property the target class declares
/// unique. That is an [`Auid`] for most classes and a [`MobId`] for mobs and
/// essence data, and the key's size on disk says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefKey {
    /// A 16-byte key.
    Auid(Auid),
    /// A 32-byte key.
    MobId(MobId),
}

impl RefKey {
    /// Reads a key of `size` bytes, which the format allows to be 16 or 32.
    fn parse(data: &[u8], size: u8) -> Result<Self> {
        match size {
            16 if data.len() >= 16 => Ok(Self::Auid(Auid::from_bytes_le(
                data[..16].try_into().expect("slice is sixteen bytes"),
            ))),
            32 if data.len() >= 32 => Ok(Self::MobId(MobId::from_bytes(
                data[..32].try_into().expect("slice is thirty-two bytes"),
            ))),
            16 | 32 => Err(Error::TruncatedProperty {
                wanted: size as usize,
                found: data.len(),
            }),
            other => Err(Error::BadKeySize { size: other }),
        }
    }

    /// The key's size on disk, in bytes.
    #[must_use]
    pub const fn size(self) -> u8 {
        match self {
            Self::Auid(_) => 16,
            Self::MobId(_) => 32,
        }
    }
}

/// One property of an object, as it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// The property's identifier, unique within its class.
    pub pid: u16,
    /// How the value is stored.
    pub format: PropertyFormat,
    /// The value, decoded as far as the format alone allows.
    pub value: PropertyValue,
}

/// A property's value, decoded as far as the format alone allows.
///
/// A [`PropertyValue::Data`] is still raw bytes: turning it into a number, a
/// string or a rational needs the property's type definition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PropertyValue {
    /// Bytes to be read with the property's type definition.
    Data(Vec<u8>),

    /// A stream stored under `name`, beside this object in the compound file.
    Stream {
        /// The stream's name.
        name: String,
    },

    /// One owned object, in the storage named `name` inside this one.
    StrongRef {
        /// The storage's name.
        name: String,
    },

    /// Owned objects in order, listed in the stream `<index_name> index`.
    StrongRefVector {
        /// The base name the index stream and the members are named after.
        index_name: String,
    },

    /// Owned objects by key, listed in the stream `<index_name> index`.
    StrongRefSet {
        /// The base name the index stream and the members are named after.
        index_name: String,
    },

    /// A reference to an object owned elsewhere in the file.
    WeakRef {
        /// Which of the file's reference targets to look the key up in.
        weakref_index: u16,
        /// The pid of the property holding the target's unique key.
        key_pid: u16,
        /// The target's unique key.
        key: RefKey,
    },

    /// References to objects owned elsewhere, in the stream `<index_name> index`.
    WeakRefArray {
        /// The base name the index stream is named after.
        index_name: String,
    },

    /// A value in a format the specification defines but nothing writes.
    Opaque(Vec<u8>),
}

impl Property {
    /// The name of this property's index stream, if it has one.
    ///
    /// Collections keep their members in a sibling stream named after the
    /// property, with `" index"` appended.
    #[must_use]
    pub fn index_stream_name(&self) -> Option<String> {
        let base = match &self.value {
            PropertyValue::StrongRefVector { index_name }
            | PropertyValue::StrongRefSet { index_name }
            | PropertyValue::WeakRefArray { index_name } => index_name,
            _ => return None,
        };
        Some(format!("{base} index"))
    }
}

/// The name of the storage holding one member of a collection.
///
/// Members are named after their property, with the member's own key in
/// braces: the third member of `Mobs-1901` is `Mobs-1901{2}`. The key is
/// written in lowercase hexadecimal with no padding.
///
/// # Example
///
/// ```
/// use aaf::property::member_storage_name;
///
/// assert_eq!(member_storage_name("Slots-4403", 0), "Slots-4403{0}");
/// assert_eq!(member_storage_name("Mobs-1901", 0x8b), "Mobs-1901{8b}");
/// ```
#[must_use]
pub fn member_storage_name(index_name: &str, local_key: u32) -> String {
    format!("{index_name}{{{local_key:x}}}")
}

/// The properties of one object, as read from its `properties` stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyStream {
    /// The format version the object was written with. Current files write 32.
    pub version: u8,
    /// The properties, in the order the stream lists them.
    pub properties: Vec<Property>,
}

impl PropertyStream {
    /// Parses a `properties` stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream is truncated, declares a byte order
    /// other than little-endian, or holds a reference key of an unsupported
    /// size.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < STREAM_HEADER_LEN {
            return Err(Error::TruncatedProperty {
                wanted: STREAM_HEADER_LEN,
                found: data.len(),
            });
        }

        let byte_order = data[0];
        if byte_order != LITTLE_ENDIAN {
            return Err(Error::UnsupportedByteOrder { mark: byte_order });
        }
        let version = data[1];
        let count = u16::from_le_bytes([data[2], data[3]]) as usize;

        let table_end = STREAM_HEADER_LEN + count * ENTRY_LEN;
        if data.len() < table_end {
            return Err(Error::TruncatedProperty {
                wanted: table_end,
                found: data.len(),
            });
        }

        // The table gives each value's length but not its position; the values
        // follow the table back to back, in the table's own order.
        let mut properties = Vec::with_capacity(count);
        let mut at = table_end;
        for entry in data[STREAM_HEADER_LEN..table_end].chunks_exact(ENTRY_LEN) {
            let pid = u16::from_le_bytes([entry[0], entry[1]]);
            let format = PropertyFormat::from_code(u16::from_le_bytes([entry[2], entry[3]]));
            let len = u16::from_le_bytes([entry[4], entry[5]]) as usize;

            let end = at + len;
            if data.len() < end {
                return Err(Error::TruncatedProperty {
                    wanted: end,
                    found: data.len(),
                });
            }
            properties.push(Property {
                pid,
                format,
                value: PropertyValue::parse(format, &data[at..end])?,
            });
            at = end;
        }

        Ok(Self {
            version,
            properties,
        })
    }

    /// The property with this pid, if the object has one.
    #[must_use]
    pub fn get(&self, pid: u16) -> Option<&Property> {
        self.properties.iter().find(|p| p.pid == pid)
    }
}

impl PropertyValue {
    fn parse(format: PropertyFormat, data: &[u8]) -> Result<Self> {
        Ok(match format {
            PropertyFormat::Data => Self::Data(data.to_vec()),

            PropertyFormat::DataStream => {
                // A stream property's name is prefixed with the byte order of
                // the stream's own contents, which is always "unspecified".
                let Some((&mark, name)) = data.split_first() else {
                    return Err(Error::TruncatedProperty {
                        wanted: 1,
                        found: 0,
                    });
                };
                if mark != STREAM_UNSPECIFIED_ENDIAN {
                    return Err(Error::UnsupportedByteOrder { mark });
                }
                Self::Stream {
                    name: decode_le(name),
                }
            }

            PropertyFormat::StrongRef => Self::StrongRef {
                name: decode_le(data),
            },
            PropertyFormat::StrongRefVector => Self::StrongRefVector {
                index_name: decode_le(data),
            },
            PropertyFormat::StrongRefSet => Self::StrongRefSet {
                index_name: decode_le(data),
            },
            PropertyFormat::WeakRefVector | PropertyFormat::WeakRefSet => Self::WeakRefArray {
                index_name: decode_le(data),
            },

            PropertyFormat::WeakRef => {
                if data.len() < 5 {
                    return Err(Error::TruncatedProperty {
                        wanted: 5,
                        found: data.len(),
                    });
                }
                Self::WeakRef {
                    weakref_index: u16::from_le_bytes([data[0], data[1]]),
                    key_pid: u16::from_le_bytes([data[2], data[3]]),
                    key: RefKey::parse(&data[5..], data[4])?,
                }
            }

            _ => Self::Opaque(data.to_vec()),
        })
    }
}

/// The index of a strong reference vector: its members, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorIndex {
    /// The next key the writer would hand out.
    pub next_free_key: u32,
    /// The last key the writer would hand out.
    pub last_free_key: u32,
    /// Each member's key, in order. See [`member_storage_name`].
    pub local_keys: Vec<u32>,
}

impl VectorIndex {
    /// Parses a strong reference vector's index stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream is shorter than the count it declares.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (count, rest) = split_count(data, 12)?;
        let wanted = 12 + count * 4;
        if data.len() < wanted {
            return Err(Error::TruncatedIndex {
                wanted,
                found: data.len(),
            });
        }
        Ok(Self {
            next_free_key: u32_at(data, 4),
            last_free_key: u32_at(data, 8),
            local_keys: rest[..count * 4]
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().expect("slice is four bytes")))
                .collect(),
        })
    }
}

/// One member of a strong reference set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetEntry {
    /// The member's key. See [`member_storage_name`].
    pub local_key: u32,
    /// How many references the writer counted. Always 1 in practice.
    pub ref_count: u32,
    /// The member's unique key, which is what the set is keyed by.
    pub key: RefKey,
}

/// The index of a strong reference set: its members, by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetIndex {
    /// The next key the writer would hand out.
    pub next_free_key: u32,
    /// The last key the writer would hand out.
    pub last_free_key: u32,
    /// The pid of the property that holds each member's unique key.
    pub key_pid: u16,
    /// The members.
    pub entries: Vec<SetEntry>,
}

impl SetIndex {
    /// Parses a strong reference set's index stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream is shorter than the count it declares,
    /// or declares a key size other than 16 or 32.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (count, rest) = split_count(data, 15)?;
        let key_size = data[14];
        let entry_len = 8 + key_size as usize;
        let wanted = 15 + count * entry_len;
        if data.len() < wanted {
            return Err(Error::TruncatedIndex {
                wanted,
                found: data.len(),
            });
        }

        let mut entries = Vec::with_capacity(count);
        for entry in rest[..count * entry_len].chunks_exact(entry_len) {
            entries.push(SetEntry {
                local_key: u32::from_le_bytes(entry[..4].try_into().expect("slice is four bytes")),
                ref_count: u32::from_le_bytes(entry[4..8].try_into().expect("slice is four bytes")),
                key: RefKey::parse(&entry[8..], key_size)?,
            });
        }

        Ok(Self {
            next_free_key: u32_at(data, 4),
            last_free_key: u32_at(data, 8),
            key_pid: u16::from_le_bytes([data[12], data[13]]),
            entries,
        })
    }
}

/// The index of a weak reference collection: the keys it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeakRefArrayIndex {
    /// Which of the file's reference targets to look the keys up in.
    pub weakref_index: u16,
    /// The pid of the property that holds each target's unique key.
    pub key_pid: u16,
    /// The keys, in order.
    pub keys: Vec<RefKey>,
}

impl WeakRefArrayIndex {
    /// Parses a weak reference collection's index stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the stream is shorter than the count it declares,
    /// or declares a key size other than 16 or 32.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let (count, rest) = split_count(data, 9)?;
        let key_size = data[8];
        let wanted = 9 + count * key_size as usize;
        if data.len() < wanted {
            return Err(Error::TruncatedIndex {
                wanted,
                found: data.len(),
            });
        }

        let mut keys = Vec::with_capacity(count);
        for key in rest[..count * key_size as usize].chunks_exact(key_size as usize) {
            keys.push(RefKey::parse(key, key_size)?);
        }

        Ok(Self {
            weakref_index: u16::from_le_bytes([data[4], data[5]]),
            key_pid: u16::from_le_bytes([data[6], data[7]]),
            keys,
        })
    }
}

/// Reads an index stream's leading count and checks its fixed header fits.
fn split_count(data: &[u8], header_len: usize) -> Result<(usize, &[u8])> {
    if data.len() < header_len {
        return Err(Error::TruncatedIndex {
            wanted: header_len,
            found: data.len(),
        });
    }
    Ok((u32_at(data, 0) as usize, &data[header_len..]))
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("slice is four bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a property stream from `(pid, format code, value bytes)` triples.
    fn stream(version: u8, entries: &[(u16, u16, &[u8])]) -> Vec<u8> {
        let mut out = vec![LITTLE_ENDIAN, version];
        out.extend((entries.len() as u16).to_le_bytes());
        for (pid, format, value) in entries {
            out.extend(pid.to_le_bytes());
            out.extend(format.to_le_bytes());
            out.extend((value.len() as u16).to_le_bytes());
        }
        for (_, _, value) in entries {
            out.extend_from_slice(value);
        }
        out
    }

    /// Encodes a string the way the format stores names: UTF-16LE plus a NUL.
    fn name(text: &str) -> Vec<u8> {
        let mut out: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        out.extend([0, 0]);
        out
    }

    #[test]
    fn parses_the_formats_the_fixtures_do_not_exercise() {
        // A data stream's name is prefixed with an "unspecified endian" byte.
        let mut stream_value = vec![STREAM_UNSPECIFIED_ENDIAN];
        stream_value.extend(name("EssenceData-2c02"));

        let parsed = PropertyStream::parse(&stream(
            32,
            &[
                (0x2c02, 0x42, &stream_value),
                (0x0003, 0x1a, &name("Descriptors-2603")),
            ],
        ))
        .expect("parses");

        assert_eq!(parsed.version, 32);
        assert_eq!(
            parsed.get(0x2c02).map(|p| &p.value),
            Some(&PropertyValue::Stream {
                name: "EssenceData-2c02".to_owned()
            })
        );
        assert_eq!(parsed.properties[1].format, PropertyFormat::WeakRefSet);
        assert_eq!(
            parsed.get(0x0003).map(|p| &p.value),
            Some(&PropertyValue::WeakRefArray {
                index_name: "Descriptors-2603".to_owned()
            })
        );
    }

    #[test]
    fn a_weak_reference_carries_its_targets_key() {
        let key = Auid::from_bytes_be([
            0x0d, 0x01, 0x01, 0x01, 0x01, 0x01, 0x2f, 0x00, 0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06,
            0x01, 0x01,
        ]);

        let mut value = vec![];
        value.extend(7u16.to_le_bytes()); // weakref index
        value.extend(0x0006u16.to_le_bytes()); // key pid
        value.push(16); // key size
        value.extend(key.to_bytes_le());

        let parsed = PropertyStream::parse(&stream(32, &[(0x0001, 0x02, &value)])).expect("parses");
        assert_eq!(
            parsed.get(0x0001).map(|p| &p.value),
            Some(&PropertyValue::WeakRef {
                weakref_index: 7,
                key_pid: 0x0006,
                key: RefKey::Auid(key),
            })
        );
    }

    #[test]
    fn a_thirty_two_byte_key_reads_as_a_mob_id() {
        let mob = MobId::from_bytes([0xab; 32]);
        let mut value = vec![];
        value.extend(0u16.to_le_bytes());
        value.extend(0u16.to_le_bytes());
        value.push(32);
        value.extend(mob.to_bytes());

        let parsed = PropertyStream::parse(&stream(32, &[(0x0001, 0x02, &value)])).expect("parses");
        let Some(PropertyValue::WeakRef { key, .. }) = parsed.get(0x0001).map(|p| &p.value) else {
            panic!("expected a weak reference");
        };
        assert_eq!(*key, RefKey::MobId(mob));
        assert_eq!(key.size(), 32);
    }

    #[test]
    fn rejects_a_stream_that_promises_more_than_it_holds() {
        let mut data = stream(32, &[(1, 0x82, &[1, 2, 3, 4])]);
        data.truncate(data.len() - 2);
        assert!(matches!(
            PropertyStream::parse(&data),
            Err(Error::TruncatedProperty {
                wanted: 14,
                found: 12
            })
        ));

        assert!(matches!(
            PropertyStream::parse(&[LITTLE_ENDIAN, 32]),
            Err(Error::TruncatedProperty {
                wanted: 4,
                found: 2
            })
        ));
    }

    #[test]
    fn rejects_a_big_endian_stream() {
        // The format allows 0x42 for big-endian; nothing writes it, and
        // reading it as little-endian would silently produce nonsense.
        let mut data = stream(32, &[]);
        data[0] = 0x42;
        assert!(matches!(
            PropertyStream::parse(&data),
            Err(Error::UnsupportedByteOrder { mark: 0x42 })
        ));
    }

    #[test]
    fn rejects_a_reference_key_that_is_neither_size() {
        let value = [0, 0, 0, 0, 24, 0, 0, 0];
        assert!(matches!(
            PropertyStream::parse(&stream(32, &[(1, 0x02, &value)])),
            Err(Error::BadKeySize { size: 24 })
        ));
    }

    #[test]
    fn parses_a_vector_index() {
        let mut data = vec![];
        data.extend(3u32.to_le_bytes()); // count
        data.extend(3u32.to_le_bytes()); // next free key
        data.extend(u32::MAX.to_le_bytes()); // last free key
        for key in [0u32, 1, 2] {
            data.extend(key.to_le_bytes());
        }

        let index = VectorIndex::parse(&data).expect("parses");
        assert_eq!(index.local_keys, vec![0, 1, 2]);
        assert_eq!(index.next_free_key, 3);
        assert_eq!(index.last_free_key, u32::MAX);

        data.truncate(data.len() - 1);
        assert!(matches!(
            VectorIndex::parse(&data),
            Err(Error::TruncatedIndex { .. })
        ));
    }

    #[test]
    fn parses_a_set_index() {
        let key = Auid::from_bytes_le([0x11; 16]);
        let mut data = vec![];
        data.extend(1u32.to_le_bytes()); // count
        data.extend(5u32.to_le_bytes()); // next free key
        data.extend(u32::MAX.to_le_bytes()); // last free key
        data.extend(0x0006u16.to_le_bytes()); // key pid
        data.push(16); // key size
        data.extend(4u32.to_le_bytes()); // local key
        data.extend(1u32.to_le_bytes()); // ref count
        data.extend(key.to_bytes_le());

        let index = SetIndex::parse(&data).expect("parses");
        assert_eq!(index.key_pid, 0x0006);
        assert_eq!(index.entries.len(), 1);
        assert_eq!(index.entries[0].local_key, 4);
        assert_eq!(index.entries[0].ref_count, 1);
        assert_eq!(index.entries[0].key, RefKey::Auid(key));
    }

    #[test]
    fn parses_a_weak_reference_index() {
        let key = Auid::from_bytes_le([0x22; 16]);
        let mut data = vec![];
        data.extend(2u32.to_le_bytes()); // count
        data.extend(9u16.to_le_bytes()); // weakref index
        data.extend(0x0006u16.to_le_bytes()); // key pid
        data.push(16); // key size
        data.extend(key.to_bytes_le());
        data.extend(key.to_bytes_le());

        let index = WeakRefArrayIndex::parse(&data).expect("parses");
        assert_eq!(index.weakref_index, 9);
        assert_eq!(index.key_pid, 0x0006);
        assert_eq!(index.keys, vec![RefKey::Auid(key); 2]);
    }

    #[test]
    fn member_names_match_the_format() {
        assert_eq!(member_storage_name("Mobs-1901", 0), "Mobs-1901{0}");
        assert_eq!(member_storage_name("Mobs-1901", 255), "Mobs-1901{ff}");
    }
}

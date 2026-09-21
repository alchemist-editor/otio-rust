//! Decoding property values against the type definitions a file carries.
//!
//! A property's bytes mean nothing on their own. The file's meta dictionary
//! says which type each property is, and this turns bytes plus a type into a
//! [`Value`]: an integer of the right width and signedness, a string, a
//! rational, a named enumeration element, an array of any of those.
//!
//! # Example
//!
//! ```no_run
//! use std::fs::File;
//! use aaf::{AafFile, MetaDictionary, Value};
//!
//! let mut file = AafFile::open(File::open("example.aaf")?)?;
//! let metadict = MetaDictionary::read(&mut file)?;
//! let object = file.root()?;
//!
//! for property in object.properties() {
//!     let Some(def) = metadict.property(object.class_id(), property.pid) else {
//!         continue;
//!     };
//!     if let Ok(value) = metadict.decode(def.type_id, property) {
//!         println!("{} = {value:?}", def.name);
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # What is not decoded here
//!
//! References are left as they were stored. A strong reference names a
//! storage and a weak reference carries a key; resolving either means reading
//! another object, which is [`AafFile`](crate::AafFile)'s job, not a pure
//! decode of some bytes. Both come back as [`Value::Reference`] carrying what
//! the property held.

use std::fmt;

use crate::error::{Error, Result};
use crate::metadict::{MetaDictionary, TypeKind};
use crate::property::{Property, PropertyValue};
use crate::utf16::decode_le;
use crate::{Auid, MobId};

/// How deep a type may nest before this gives up.
///
/// Types refer to each other by identifier, so nothing structurally stops a
/// record from containing itself. A file that does would otherwise recurse
/// until the stack ran out.
const MAX_DEPTH: usize = 32;

/// The type of an AUID, which is a record this decodes whole.
const AUID_TYPE: Auid = Auid::from_bytes_be([
    0x01, 0x03, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x0e, 0x2b, 0x34, 0x01, 0x04, 0x01, 0x01,
]);

/// The type of a MobID, which is a record this decodes whole.
const MOB_ID_TYPE: Auid = Auid::from_bytes_be([
    0x01, 0x03, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x0e, 0x2b, 0x34, 0x01, 0x04, 0x01, 0x01,
]);

/// The character type, whose arrays are strings rather than numbers.
const CHARACTER_TYPE: Auid = Auid::from_bytes_be([
    0x01, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x0e, 0x2b, 0x34, 0x01, 0x04, 0x01, 0x01,
]);

/// A decoded property value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    UInt(u64),
    /// A 16-byte identifier.
    Auid(Auid),
    /// A 32-byte mob identifier.
    MobId(MobId),
    /// A string.
    String(String),
    /// A single character.
    Char(char),

    /// An element of an enumeration, with its name if the file names it.
    Enum {
        /// The stored value.
        value: i64,
        /// The element's name, if the enumeration defines one for this value.
        name: Option<String>,
    },

    /// An element of an extendible enumeration, with its name if known.
    ExtEnum {
        /// The stored value.
        value: Auid,
        /// The element's name, if the enumeration defines one for this value.
        name: Option<String>,
    },

    /// Named members of their own types, in storage order.
    Record(Vec<(String, Value)>),

    /// Elements of one type, in order.
    Array(Vec<Value>),

    /// Elements of one type, unordered.
    Set(Vec<Value>),

    /// A stream stored elsewhere in the file, named rather than read.
    Stream {
        /// The stream's name.
        name: String,
    },

    /// A reference to another object, left as it was stored.
    Reference(PropertyValue),

    /// A value whose type this could not resolve, left as bytes.
    ///
    /// Carries the type it claimed to be, so a caller can say what it was
    /// unable to read rather than only that it failed.
    Unresolved {
        /// The type the property declared.
        type_id: Auid,
        /// The bytes as stored.
        data: Vec<u8>,
    },
}

impl fmt::Display for Value {
    /// Renders a value the way a person would want to read it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::UInt(v) => write!(f, "{v}"),
            Self::Auid(v) => write!(f, "{v}"),
            Self::MobId(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "{v}"),
            Self::Char(v) => write!(f, "{v}"),
            Self::Enum { value, name } => match name {
                Some(name) => write!(f, "{name}"),
                None => write!(f, "{value}"),
            },
            Self::ExtEnum { value, name } => match name {
                Some(name) => write!(f, "{name}"),
                None => write!(f, "{value}"),
            },
            Self::Record(members) => {
                f.write_str("{")?;
                for (i, (name, value)) in members.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{name}: {value}")?;
                }
                f.write_str("}")
            }
            Self::Array(items) | Self::Set(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Self::Stream { name } => write!(f, "<stream {name}>"),
            Self::Reference(_) => f.write_str("<reference>"),
            Self::Unresolved { type_id, data } => {
                write!(f, "<{} bytes of {type_id}>", data.len())
            }
        }
    }
}

impl Value {
    /// The value as an integer, whether it was stored signed or unsigned.
    ///
    /// Enumeration elements count, since their stored value is an integer. An
    /// unsigned value too large for `i64` gives `None` rather than wrapping.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(v) | Self::Enum { value: v, .. } => Some(*v),
            Self::UInt(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// The value as a string, if it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            Self::Enum { name, .. } | Self::ExtEnum { name, .. } => name.as_deref(),
            _ => None,
        }
    }

    /// A record member by name.
    #[must_use]
    pub fn member(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Record(members) => members
                .iter()
                .find(|(member, _)| member == name)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The value as a rational, if it is one.
    ///
    /// AAF stores rates and edit rates as a `Rational`, a record of a
    /// numerator and a denominator. This is the shape `opentime` wants.
    #[must_use]
    pub fn as_rational(&self) -> Option<(i64, i64)> {
        Some((
            self.member("Numerator")?.as_i64()?,
            self.member("Denominator")?.as_i64()?,
        ))
    }
}

impl MetaDictionary {
    /// Decodes a property's value using the type it declares.
    ///
    /// # Errors
    ///
    /// Returns an error if the type is not one the dictionary defines, or if
    /// the stored bytes are the wrong length for it.
    pub fn decode(&self, type_id: Auid, property: &Property) -> Result<Value> {
        match &property.value {
            PropertyValue::Data(data) => self.decode_bytes(type_id, data, 0),
            PropertyValue::Stream { name } => Ok(Value::Stream { name: name.clone() }),
            other => Ok(Value::Reference(other.clone())),
        }
    }

    /// Decodes bytes known to be of a given type.
    ///
    /// # Errors
    ///
    /// Returns an error if the type is not one the dictionary defines, or if
    /// the bytes are the wrong length for it.
    pub fn decode_bytes(&self, type_id: Auid, data: &[u8], depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(Error::TypeTooDeep { type_id });
        }

        // Two records are really single values and are decoded whole rather
        // than member by member.
        match type_id {
            AUID_TYPE => return Ok(Value::Auid(auid_from(data)?)),
            MOB_ID_TYPE => {
                let bytes: [u8; 32] = data.try_into().map_err(|_| Error::WrongValueSize {
                    type_id,
                    wanted: 32,
                    found: data.len(),
                })?;
                return Ok(Value::MobId(MobId::from_bytes(bytes)));
            }
            _ => {}
        }

        let type_def = self
            .type_def(type_id)
            .ok_or(Error::UndefinedType { type_id })?;

        match &type_def.kind {
            TypeKind::Int { size, signed } => decode_int(type_id, data, *size, *signed),

            TypeKind::Character => {
                let unit =
                    u16::from_le_bytes(data.get(..2).and_then(|b| b.try_into().ok()).ok_or(
                        Error::WrongValueSize {
                            type_id,
                            wanted: 2,
                            found: data.len(),
                        },
                    )?);
                Ok(Value::Char(
                    char::decode_utf16([unit])
                        .next()
                        .and_then(std::result::Result::ok)
                        .unwrap_or(char::REPLACEMENT_CHARACTER),
                ))
            }

            TypeKind::String { .. } => Ok(Value::String(decode_le(data))),

            TypeKind::Enum {
                element_type,
                elements,
            } => {
                let value = self
                    .decode_bytes(*element_type, data, depth + 1)?
                    .as_i64()
                    .ok_or(Error::UndefinedType {
                        type_id: *element_type,
                    })?;
                Ok(Value::Enum {
                    value,
                    name: elements
                        .iter()
                        .find(|(v, _)| *v == value)
                        .map(|(_, name)| name.clone()),
                })
            }

            TypeKind::ExtEnum { elements } => {
                let value = auid_from(data)?;
                Ok(Value::ExtEnum {
                    value,
                    name: elements
                        .iter()
                        .find(|(v, _)| *v == value)
                        .map(|(_, name)| name.clone()),
                })
            }

            TypeKind::Rename { renamed } => self.decode_bytes(*renamed, data, depth + 1),

            TypeKind::Record { members } => {
                let mut out = Vec::with_capacity(members.len());
                let mut at = 0;
                for (name, member_type) in members {
                    let size =
                        self.byte_size(*member_type, depth + 1)
                            .ok_or(Error::UnsizedType {
                                type_id: *member_type,
                            })?;
                    let end = at + size;
                    let bytes = data.get(at..end).ok_or(Error::WrongValueSize {
                        type_id,
                        wanted: end,
                        found: data.len(),
                    })?;
                    out.push((
                        name.clone(),
                        self.decode_bytes(*member_type, bytes, depth + 1)?,
                    ));
                    at = end;
                }
                Ok(Value::Record(out))
            }

            TypeKind::FixedArray {
                element_type,
                count,
            } => Ok(Value::Array(self.decode_elements(
                *element_type,
                data,
                Some(*count as usize),
                depth,
            )?)),

            TypeKind::VarArray { element_type } => {
                // An array of characters is a run of strings, not of numbers.
                if *element_type == CHARACTER_TYPE {
                    return Ok(Value::Array(
                        utf16_strings(data).into_iter().map(Value::String).collect(),
                    ));
                }
                Ok(Value::Array(self.decode_elements(
                    *element_type,
                    data,
                    None,
                    depth,
                )?))
            }

            TypeKind::Set { element_type } => Ok(Value::Set(self.decode_elements(
                *element_type,
                data,
                None,
                depth,
            )?)),

            TypeKind::Indirect | TypeKind::Opaque => {
                // An indirect value carries the identity of its own type.
                let Some((&mark, rest)) = data.split_first() else {
                    return Err(Error::WrongValueSize {
                        type_id,
                        wanted: 17,
                        found: 0,
                    });
                };
                if mark != 0x4c {
                    return Err(Error::UnsupportedByteOrder { mark });
                }
                let inner = auid_from(rest.get(..16).ok_or(Error::WrongValueSize {
                    type_id,
                    wanted: 17,
                    found: data.len(),
                })?)?;
                self.decode_bytes(inner, &rest[16..], depth + 1)
            }

            TypeKind::Stream => Ok(Value::Unresolved {
                type_id,
                data: data.to_vec(),
            }),

            // A reference stored as data rather than as a reference property.
            TypeKind::StrongRef { .. } | TypeKind::WeakRef { .. } | TypeKind::Unknown { .. } => {
                Ok(Value::Unresolved {
                    type_id,
                    data: data.to_vec(),
                })
            }
        }
    }

    /// Decodes a run of same-typed elements laid out back to back.
    fn decode_elements(
        &self,
        element_type: Auid,
        data: &[u8],
        count: Option<usize>,
        depth: usize,
    ) -> Result<Vec<Value>> {
        let size = self
            .byte_size(element_type, depth + 1)
            .ok_or(Error::UnsizedType {
                type_id: element_type,
            })?;
        if size == 0 {
            return Err(Error::UnsizedType {
                type_id: element_type,
            });
        }

        let available = data.len() / size;
        let count = count.unwrap_or(available);
        if count > available {
            return Err(Error::WrongValueSize {
                type_id: element_type,
                wanted: count * size,
                found: data.len(),
            });
        }

        (0..count)
            .map(|i| self.decode_bytes(element_type, &data[i * size..(i + 1) * size], depth + 1))
            .collect()
    }

    /// How many bytes a value of this type occupies, if the type has a fixed size.
    ///
    /// Strings, variable arrays, sets and streams have no fixed size, so they
    /// cannot be members of a record or elements of an array, and this gives
    /// `None` for them.
    #[must_use]
    pub fn byte_size(&self, type_id: Auid, depth: usize) -> Option<usize> {
        if depth > MAX_DEPTH {
            return None;
        }
        match type_id {
            AUID_TYPE => return Some(16),
            MOB_ID_TYPE => return Some(32),
            _ => {}
        }

        match &self.type_def(type_id)?.kind {
            TypeKind::Int { size, .. } => Some(*size as usize),
            TypeKind::Character => Some(2),
            TypeKind::ExtEnum { .. } => Some(16),
            TypeKind::Enum { element_type, .. } => self.byte_size(*element_type, depth + 1),
            TypeKind::Rename { renamed } => self.byte_size(*renamed, depth + 1),
            TypeKind::Record { members } => members
                .iter()
                .map(|(_, member)| self.byte_size(*member, depth + 1))
                .sum(),
            TypeKind::FixedArray {
                element_type,
                count,
            } => self
                .byte_size(*element_type, depth + 1)
                .map(|size| size * *count as usize),
            _ => None,
        }
    }
}

fn decode_int(type_id: Auid, data: &[u8], size: u8, signed: bool) -> Result<Value> {
    let wrong = || Error::WrongValueSize {
        type_id,
        wanted: size as usize,
        found: data.len(),
    };
    if data.len() != size as usize {
        return Err(wrong());
    }

    // Sign-extend from the stored width, so a negative `int16` stays negative
    // once it is widened to 64 bits.
    let mut buffer = [0u8; 8];
    buffer[..data.len()].copy_from_slice(data);
    let raw = u64::from_le_bytes(buffer);

    if signed {
        let bits = u32::from(size) * 8;
        let value = if bits < 64 {
            let shift = 64 - bits;
            ((raw << shift) as i64) >> shift
        } else {
            raw as i64
        };
        Ok(Value::Int(value))
    } else {
        Ok(Value::UInt(raw))
    }
}

fn auid_from(data: &[u8]) -> Result<Auid> {
    <[u8; 16]>::try_from(data)
        .map(Auid::from_bytes_le)
        .map_err(|_| Error::WrongValueSize {
            type_id: AUID_TYPE,
            wanted: 16,
            found: data.len(),
        })
}

/// Splits a run of NUL-terminated UTF-16 strings.
fn utf16_strings(data: &[u8]) -> Vec<String> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let terminators = units.iter().filter(|unit| **unit == 0).count();
    units
        .split(|unit| *unit == 0)
        .take(terminators)
        .map(|run| {
            char::decode_utf16(run.iter().copied())
                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadict::TypeDef;
    use crate::property::PropertyFormat;

    /// A distinct identifier per test type, so the tests read as names.
    fn id(n: u8) -> Auid {
        let mut bytes = [0u8; 16];
        bytes[0] = n;
        Auid::from_bytes_be(bytes)
    }

    const INT32: u8 = 1;
    const INT16: u8 = 2;
    const UINT8: u8 = 3;
    const CHAR: u8 = 4;
    const ARRAY: u8 = 5;
    const SET: u8 = 6;
    const STREAM: u8 = 7;
    const OPAQUE: u8 = 8;
    const RECORD: u8 = 9;

    /// A dictionary holding the types these tests need.
    ///
    /// The two fixture files do not use fixed arrays, sets, characters,
    /// opaque values or streams, so those paths are exercised here against a
    /// dictionary built by hand rather than left untested.
    fn dictionary() -> MetaDictionary {
        let mut dict = MetaDictionary::default();
        let mut define = |n: u8, name: &str, kind: TypeKind| {
            dict.define_type(TypeDef {
                auid: id(n),
                name: name.to_owned(),
                kind,
            });
        };
        define(
            INT32,
            "Int32",
            TypeKind::Int {
                size: 4,
                signed: true,
            },
        );
        define(
            INT16,
            "Int16",
            TypeKind::Int {
                size: 2,
                signed: true,
            },
        );
        define(
            UINT8,
            "UInt8",
            TypeKind::Int {
                size: 1,
                signed: false,
            },
        );
        define(CHAR, "Character", TypeKind::Character);
        define(
            ARRAY,
            "Int16Array4",
            TypeKind::FixedArray {
                element_type: id(INT16),
                count: 4,
            },
        );
        define(
            SET,
            "UInt8Set",
            TypeKind::Set {
                element_type: id(UINT8),
            },
        );
        define(STREAM, "Stream", TypeKind::Stream);
        define(OPAQUE, "Opaque", TypeKind::Opaque);
        define(
            RECORD,
            "Pair",
            TypeKind::Record {
                members: vec![
                    ("first".to_owned(), id(INT16)),
                    ("second".to_owned(), id(UINT8)),
                ],
            },
        );
        dict
    }

    fn decode(type_id: u8, data: &[u8]) -> Result<Value> {
        dictionary().decode_bytes(id(type_id), data, 0)
    }

    #[test]
    fn signed_integers_keep_their_sign_when_widened() {
        assert_eq!(decode(INT16, &[0xff, 0xff]).unwrap(), Value::Int(-1));
        assert_eq!(decode(INT16, &[0x00, 0x80]).unwrap(), Value::Int(-32768));
        assert_eq!(
            decode(INT32, &[0xfe, 0xff, 0xff, 0xff]).unwrap(),
            Value::Int(-2)
        );
        assert_eq!(decode(UINT8, &[0xff]).unwrap(), Value::UInt(255));
    }

    #[test]
    fn an_integer_of_the_wrong_width_is_an_error() {
        assert!(matches!(
            decode(INT32, &[0x01, 0x02]),
            Err(Error::WrongValueSize {
                wanted: 4,
                found: 2,
                ..
            })
        ));
    }

    #[test]
    fn a_fixed_array_decodes_exactly_its_count() {
        let data = [0x01, 0x00, 0xff, 0xff, 0x02, 0x00, 0x03, 0x00];
        assert_eq!(
            decode(ARRAY, &data).unwrap(),
            Value::Array(vec![
                Value::Int(1),
                Value::Int(-1),
                Value::Int(2),
                Value::Int(3)
            ])
        );
    }

    #[test]
    fn a_fixed_array_missing_elements_is_an_error() {
        assert!(matches!(
            decode(ARRAY, &[0x01, 0x00, 0x02, 0x00]),
            Err(Error::WrongValueSize { wanted: 8, .. })
        ));
    }

    #[test]
    fn a_set_takes_as_many_elements_as_the_bytes_hold() {
        assert_eq!(
            decode(SET, &[0x07, 0x09, 0x0b]).unwrap(),
            Value::Set(vec![Value::UInt(7), Value::UInt(9), Value::UInt(11)])
        );
    }

    #[test]
    fn a_character_is_one_utf16_unit() {
        assert_eq!(decode(CHAR, &[0x41, 0x00]).unwrap(), Value::Char('A'));
        assert_eq!(decode(CHAR, &[0xe9, 0x00]).unwrap(), Value::Char('é'));
    }

    #[test]
    fn a_record_reads_its_members_in_order() {
        assert_eq!(
            decode(RECORD, &[0xff, 0xff, 0x2a]).unwrap(),
            Value::Record(vec![
                ("first".to_owned(), Value::Int(-1)),
                ("second".to_owned(), Value::UInt(42)),
            ])
        );
    }

    /// A stream's bytes live elsewhere, so a stream typed as data stays bytes.
    #[test]
    fn a_stream_typed_value_is_left_as_stored() {
        assert_eq!(
            decode(STREAM, &[0x01, 0x02]).unwrap(),
            Value::Unresolved {
                type_id: id(STREAM),
                data: vec![0x01, 0x02],
            }
        );
    }

    #[test]
    fn an_opaque_value_carries_the_type_of_what_is_inside_it() {
        let mut data = vec![0x4c];
        data.extend_from_slice(&id(INT16).to_bytes_le());
        data.extend_from_slice(&[0xff, 0xff]);
        assert_eq!(decode(OPAQUE, &data).unwrap(), Value::Int(-1));
    }

    #[test]
    fn an_opaque_value_in_an_order_we_cannot_read_is_an_error() {
        let mut data = vec![0x42];
        data.extend_from_slice(&id(INT16).to_bytes_le());
        data.extend_from_slice(&[0x00, 0x01]);
        assert!(matches!(
            dictionary().decode_bytes(id(OPAQUE), &data, 0),
            Err(Error::UnsupportedByteOrder { mark: 0x42 })
        ));
    }

    #[test]
    fn a_type_the_dictionary_does_not_define_is_an_error() {
        assert!(matches!(
            decode(200, &[0x00]),
            Err(Error::UndefinedType { .. })
        ));
    }

    /// A record cannot hold a member whose width depends on its contents.
    #[test]
    fn a_record_of_an_unsized_member_is_an_error() {
        let mut dict = dictionary();
        dict.define_type(TypeDef {
            auid: id(100),
            name: "Bad".to_owned(),
            kind: TypeKind::Record {
                members: vec![("only".to_owned(), id(STREAM))],
            },
        });
        assert!(matches!(
            dict.decode_bytes(id(100), &[0x00], 0),
            Err(Error::UnsizedType { .. })
        ));
    }

    /// Nothing on disk stops a record from containing itself.
    #[test]
    fn a_self_referential_record_gives_up_rather_than_recursing() {
        let mut dict = MetaDictionary::default();
        dict.define_type(TypeDef {
            auid: id(100),
            name: "Loop".to_owned(),
            kind: TypeKind::Record {
                members: vec![("self".to_owned(), id(100))],
            },
        });
        assert_eq!(dict.byte_size(id(100), 0), None);
        assert!(dict.decode_bytes(id(100), &[0x00; 8], 0).is_err());
    }

    #[test]
    fn a_stream_property_is_named_rather_than_read() {
        let property = Property {
            pid: 1,
            format: PropertyFormat::DataStream,
            value: PropertyValue::Stream {
                name: "audio".to_owned(),
            },
        };
        assert_eq!(
            dictionary().decode(id(STREAM), &property).unwrap(),
            Value::Stream {
                name: "audio".to_owned()
            }
        );
    }

    #[test]
    fn a_rational_reads_back_as_a_pair() {
        let value = Value::Record(vec![
            ("Numerator".to_owned(), Value::Int(30000)),
            ("Denominator".to_owned(), Value::Int(1001)),
        ]);
        assert_eq!(value.as_rational(), Some((30000, 1001)));
        assert_eq!(
            value.member("Numerator").and_then(Value::as_i64),
            Some(30000)
        );
        assert_eq!(value.to_string(), "{Numerator: 30000, Denominator: 1001}");
    }

    #[test]
    fn a_run_of_strings_splits_on_its_terminators() {
        // "ab\0c\0" — the trailing run after the last terminator is not a string.
        let data = [
            0x61, 0x00, 0x62, 0x00, 0x00, 0x00, 0x63, 0x00, 0x00, 0x00, 0x64, 0x00,
        ];
        assert_eq!(utf16_strings(&data), vec!["ab".to_owned(), "c".to_owned()]);
    }
}

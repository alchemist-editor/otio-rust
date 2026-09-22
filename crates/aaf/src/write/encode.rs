//! Encoding values against the types properties declare.
//!
//! This is `encode` from each of pyaaf2's type definition classes in
//! `types.py`, with its conversions: a string names an enumeration element
//! or parses as a rational, a timestamp fills a `TimeStamp` record, an
//! indirect value carries the type pyaaf2 would infer for it. pyaaf2's
//! quirks are kept where they change the bytes, and noted where they are.

use super::AafWriter;
use super::model::{Kind, dict_elements};
use super::object::encode_string;
use super::value::{Rational, WriteValue};
use crate::Auid;
use crate::builtin::auid;
use crate::error::{Error, Result};

const BOOLEAN: Auid = auid("01040100-0000-0000-060e-2b3401040101");
const MOBID: Auid = auid("01030200-0000-0000-060e-2b3401040101");
const AUID_TYPE: Auid = auid("01030100-0000-0000-060e-2b3401040101");
const DATESTRUCT: Auid = auid("03010500-0000-0000-060e-2b3401040101");
const TIMESTAMP: Auid = auid("03010700-0000-0000-060e-2b3401040101");
pub(crate) const RATIONAL: Auid = auid("03010100-0000-0000-060e-2b3401040101");
pub(crate) const STRING: Auid = auid("01100200-0000-0000-060e-2b3401040101");
pub(crate) const INT32: Auid = auid("01010700-0000-0000-060e-2b3401040101");

impl AafWriter {
    fn invalid(&self, type_id: Auid, reason: impl Into<String>) -> Error {
        Error::InvalidValue {
            type_name: self
                .model
                .type_def(type_id)
                .map_or_else(|| type_id.to_string(), |t| t.name.clone()),
            reason: reason.into(),
        }
    }

    fn wrong_value(&self, type_id: Auid, value: &WriteValue) -> Error {
        self.invalid(type_id, format!("{} does not fit", value.kind()))
    }

    /// The type a record member is, looked up the way pyaaf2 looks it up: by
    /// the name of the type the member refers to.
    fn member_type(&self, member: Auid) -> Result<Auid> {
        let name = &self
            .model
            .type_def(member)
            .ok_or(Error::UndefinedType { type_id: member })?
            .name;
        Ok(self
            .model
            .type_named(name)
            .ok_or(Error::UndefinedType { type_id: member })?
            .auid)
    }

    /// Encodes a value as the given type.
    pub(crate) fn encode(&self, type_id: Auid, value: &WriteValue) -> Result<Vec<u8>> {
        let t = self
            .model
            .type_def(type_id)
            .ok_or(Error::UndefinedType { type_id })?;
        match &t.kind {
            Kind::Int { size, signed } => self.encode_int(type_id, *size, *signed, value),
            Kind::Enum {
                element,
                names,
                values,
            } => {
                if t.auid == BOOLEAN {
                    return Ok(vec![u8::from(truthy(value))]);
                }
                for (index, name) in dict_elements(values, names) {
                    let matches = match value {
                        WriteValue::Str(s) => *s == name,
                        WriteValue::Int(i) => *i == index,
                        WriteValue::Bool(b) => i64::from(*b) == index,
                        _ => false,
                    };
                    if matches {
                        return self.encode(*element, &WriteValue::Int(index));
                    }
                }
                Err(self.invalid(type_id, format!("no element {value:?}")))
            }
            Kind::ExtEnum { names, values } => {
                for (key, name) in dict_elements(values, names) {
                    let matches = match value {
                        WriteValue::Auid(a) => *a == key,
                        WriteValue::Str(s) => s.to_lowercase() == name.to_lowercase(),
                        _ => return Err(self.wrong_value(type_id, value)),
                    };
                    if matches {
                        return Ok(key.to_bytes_le().to_vec());
                    }
                }
                Err(self.invalid(type_id, format!("no element {value:?}")))
            }
            Kind::Record { members } => self.encode_record(type_id, members, value),
            Kind::FixedArray { element, count } => {
                let WriteValue::Array(items) = value else {
                    return Err(self.wrong_value(type_id, value));
                };
                if items.is_empty() {
                    return Err(self.invalid(type_id, "pyaaf2 cannot store an empty fixed array"));
                }
                if items.len() > *count as usize {
                    return Err(self.invalid(type_id, format!("at most {count} elements fit")));
                }
                let mut out = Vec::new();
                for item in items {
                    out.extend(self.encode(*element, item)?);
                }
                // pyaaf2 pads from the index of the last element rather than
                // the count of them, so a fixed array is always stored one
                // element longer than it is, with zeros.
                let byte_size = self.byte_size(*element)?;
                out.resize(
                    out.len() + (*count as usize - (items.len() - 1)) * byte_size,
                    0,
                );
                Ok(out)
            }
            Kind::VarArray { element } => {
                let WriteValue::Array(items) = value else {
                    return Err(self.wrong_value(type_id, value));
                };
                let element_type = self
                    .model
                    .type_def(*element)
                    .ok_or(Error::UndefinedType { type_id: *element })?;
                if element_type.name == "Character" {
                    let mut out = Vec::new();
                    for item in items {
                        let WriteValue::Str(s) = item else {
                            return Err(self.wrong_value(type_id, item));
                        };
                        out.extend(encode_string(s));
                    }
                    return Ok(out);
                }
                let mut out = Vec::new();
                for item in items {
                    out.extend(self.encode(*element, item)?);
                }
                Ok(out)
            }
            Kind::Set { element } => {
                let WriteValue::Array(items) = value else {
                    return Err(self.wrong_value(type_id, value));
                };
                let mut seen: Vec<&WriteValue> = Vec::new();
                let mut out = Vec::new();
                for item in items {
                    if seen.contains(&item) {
                        continue;
                    }
                    seen.push(item);
                    out.extend(self.encode(*element, item)?);
                }
                Ok(out)
            }
            Kind::String { .. } => match value {
                WriteValue::Str(s) => Ok(encode_string(s)),
                _ => Err(self.wrong_value(type_id, value)),
            },
            Kind::Rename { renamed } => self.encode(*renamed, value),
            Kind::Indirect | Kind::Opaque => self.encode_indirect(value),
            Kind::StrongRef { .. } | Kind::WeakRef { .. } | Kind::Stream => {
                Err(self.invalid(type_id, "a reference or stream is not stored as a value"))
            }
            Kind::Character | Kind::GenericCharacter => {
                Err(self.invalid(type_id, "pyaaf2 cannot encode a single character"))
            }
        }
    }

    fn encode_int(
        &self,
        type_id: Auid,
        size: u8,
        signed: bool,
        value: &WriteValue,
    ) -> Result<Vec<u8>> {
        let v = match value {
            WriteValue::Int(i) => i128::from(*i),
            WriteValue::Bool(b) => i128::from(*b),
            _ => return Err(self.wrong_value(type_id, value)),
        };
        let bits = u32::from(size) * 8;
        if !matches!(size, 1 | 2 | 4 | 8) {
            return Err(self.invalid(type_id, format!("unknown integer size {size}")));
        }
        let (min, max) = if signed {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        };
        if v < min || v > max {
            return Err(self.invalid(type_id, format!("{v} is out of range")));
        }
        Ok(v.to_le_bytes()[..usize::from(size)].to_vec())
    }

    fn encode_record(
        &self,
        type_id: Auid,
        members: &[(String, Auid)],
        value: &WriteValue,
    ) -> Result<Vec<u8>> {
        match (type_id, value) {
            (MOBID, WriteValue::MobId(m)) => return Ok(m.to_bytes().to_vec()),
            (AUID_TYPE, WriteValue::Auid(a)) => return Ok(a.to_bytes_le().to_vec()),
            (TIMESTAMP, WriteValue::Timestamp(t)) => {
                let date = WriteValue::record([
                    ("year", WriteValue::Int(i64::from(t.year))),
                    ("month", WriteValue::Int(i64::from(t.month))),
                    ("day", WriteValue::Int(i64::from(t.day))),
                ]);
                let time = WriteValue::record([
                    ("hour", WriteValue::Int(i64::from(t.hour))),
                    ("minute", WriteValue::Int(i64::from(t.minute))),
                    ("second", WriteValue::Int(i64::from(t.second))),
                    ("fraction", WriteValue::Int(0)),
                ]);
                let (Some(d), Some(tm)) = (members.first(), members.get(1)) else {
                    return Err(self.invalid(type_id, "a timestamp has a date and a time"));
                };
                let mut out = self.encode(self.member_type(d.1)?, &date)?;
                out.extend(self.encode(self.member_type(tm.1)?, &time)?);
                return Ok(out);
            }
            (DATESTRUCT, WriteValue::Timestamp(t)) => {
                let date = WriteValue::record([
                    ("year", WriteValue::Int(i64::from(t.year))),
                    ("month", WriteValue::Int(i64::from(t.month))),
                    ("day", WriteValue::Int(i64::from(t.day))),
                ]);
                return self.encode_record(type_id, members, &date);
            }
            (RATIONAL, v) if !matches!(v, WriteValue::Record(_)) => {
                let r = self.rational(type_id, v)?;
                let fields = WriteValue::record([
                    ("Numerator", WriteValue::Int(r.numerator)),
                    ("Denominator", WriteValue::Int(r.denominator)),
                ]);
                return self.encode_record(type_id, members, &fields);
            }
            _ => {}
        }
        let WriteValue::Record(fields) = value else {
            return Err(self.wrong_value(type_id, value));
        };
        let mut out = Vec::new();
        for (name, member) in members {
            let field = fields
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v)
                .ok_or_else(|| self.invalid(type_id, format!("no member '{name}'")))?;
            out.extend(self.encode(self.member_type(*member)?, field)?);
        }
        Ok(out)
    }

    /// `AAFRational(value)`.
    fn rational(&self, type_id: Auid, value: &WriteValue) -> Result<Rational> {
        match value {
            WriteValue::Rational(r) => Ok(*r),
            WriteValue::Int(i) => Ok(Rational::new(*i, 1)),
            WriteValue::Bool(b) => Ok(Rational::new(i64::from(*b), 1)),
            WriteValue::Str(s) => {
                Rational::parse(s).map_err(|e| self.invalid(type_id, e.to_string()))
            }
            _ => Err(self.wrong_value(type_id, value)),
        }
    }

    /// The value of an indirect property: a byte order mark, the type, and
    /// the value as that type.
    pub(crate) fn encode_indirect(&self, value: &WriteValue) -> Result<Vec<u8>> {
        let (type_id, inner) = match value {
            WriteValue::Typed { type_id, value } => (*type_id, value.as_ref()),
            WriteValue::Str(_) => (STRING, value),
            WriteValue::Rational(_) => (RATIONAL, value),
            WriteValue::Int(_) | WriteValue::Bool(_) => (INT32, value),
            _ => {
                return Err(Error::InvalidValue {
                    type_name: "Indirect".to_owned(),
                    reason: format!(
                        "pyaaf2 infers no type for {}; give one with WriteValue::typed",
                        value.kind()
                    ),
                });
            }
        };
        let t = self
            .model
            .type_def(type_id)
            .ok_or(Error::UndefinedType { type_id })?;
        let mut out = vec![0x4c];
        out.extend_from_slice(&t.auid.to_bytes_le());
        out.extend(self.encode(t.auid, inner)?);
        Ok(out)
    }

    /// How many bytes a value of a fixed-size type takes.
    fn byte_size(&self, type_id: Auid) -> Result<usize> {
        let t = self
            .model
            .type_def(type_id)
            .ok_or(Error::UndefinedType { type_id })?;
        match &t.kind {
            Kind::Int { size, .. } => Ok(usize::from(*size)),
            Kind::Enum { element, .. } => self.byte_size(*element),
            Kind::FixedArray { element, count } => Ok(self.byte_size(*element)? * *count as usize),
            Kind::Record { members } => {
                let mut size = 0;
                for (_, member) in members {
                    size += self.byte_size(self.member_type(*member)?)?;
                }
                Ok(size)
            }
            _ => Err(Error::UnsizedType { type_id }),
        }
    }
}

/// Python truthiness, for storing a value as a `Boolean`.
fn truthy(value: &WriteValue) -> bool {
    match value {
        WriteValue::Bool(b) => *b,
        WriteValue::Int(i) => *i != 0,
        WriteValue::Str(s) => !s.is_empty(),
        WriteValue::Array(a) => !a.is_empty(),
        WriteValue::Objects(o) => !o.is_empty(),
        WriteValue::Record(r) => !r.is_empty(),
        WriteValue::Rational(r) => r.numerator != 0,
        _ => true,
    }
}

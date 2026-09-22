//! AAF values the way upstream sees them, and the metadata made from them.
//!
//! Upstream's adapter never looks at an AAF property directly. It asks
//! pyaaf2 for the property's value, gets back a Python object — an `int`, a
//! `str`, a `Fraction`, a `datetime`, a `dict`, a `set`, a list of AAF
//! objects — and then decides what to keep by asking what *kind* of Python
//! object it got. So the metadata a file comes out with is shaped as much by
//! pyaaf2's choice of Python types as by the file.
//!
//! This module reproduces that in two steps rather than guessing at the
//! result. [`Py`] is the Python value pyaaf2 would have produced, decoded
//! the way pyaaf2 decodes each type; [`Transcriber::transcribe_property`] is
//! upstream's `_transcribe_property` over it, branch for branch.
//!
//! # The rules worth knowing
//!
//! They are upstream's, and they are what a reader of the metadata notices:
//!
//! - **A list keeps only what has a name and a value.** A list of AAF objects
//!   becomes a dictionary keyed by each member's name, and only members that
//!   also carry a value — tagged values, parameters, control points — put
//!   anything in it. A list of plain numbers has no names at all, so a
//!   descriptor's `Summary` comes out as `{}`.
//! - **A set is kept as a list.** A marker's `DescribedSlots` is `[9]`.
//! - **A weak reference is followed once.** At the top of an object's
//!   metadata a weak reference is replaced by the object it names; inside
//!   that object only a data definition is followed, which is what stops an
//!   operation definition dragging the file's whole dictionary in behind it.
//! - **Anything else is printed.** A rational prints as Python prints a
//!   `Fraction`, a timestamp as a `datetime`, an identifier in its text
//!   form, and a missing value as `None`.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek};
use std::sync::OnceLock;

use aaf::property::{Property, PropertyValue};
use aaf::{Auid, MobId, Object, TypeKind, Value};
use otio_core::{Any, AnyDictionary};

use crate::Transcriber;
use crate::error::Result;

/// How deep values nest before giving up.
///
/// Strong references can nest as deeply as a file likes and a type can in
/// principle contain itself. Nothing real comes close to this.
const MAX_DEPTH: u32 = 32;

/// The types pyaaf2 turns into something other than a dictionary.
const AUID_TYPE: &str = "01030100-0000-0000-060e-2b3401040101";
const MOB_ID_TYPE: &str = "01030200-0000-0000-060e-2b3401040101";
const RATIONAL_TYPE: &str = "03010100-0000-0000-060e-2b3401040101";
const DATE_TYPE: &str = "03010500-0000-0000-060e-2b3401040101";
const TIME_TYPE: &str = "03010600-0000-0000-060e-2b3401040101";
const TIMESTAMP_TYPE: &str = "03010700-0000-0000-060e-2b3401040101";
const BOOLEAN_TYPE: &str = "01040100-0000-0000-060e-2b3401040101";
const CHARACTER_TYPE: &str = "01100100-0000-0000-060e-2b3401040101";

/// The interpolations upstream names, by the definition that identifies them.
///
/// Anything else is recorded as `Linear`, which is upstream's fallback.
const INTERPOLATIONS: [(&str, &str); 4] = [
    ("5b6c85a5-0ede-11d3-80a9-006008143e6f", "Constant"),
    ("5b6c85a4-0ede-11d3-80a9-006008143e6f", "Linear"),
    ("df394eda-6ac6-4566-8dbe-f28b0bdd781a", "Bezier"),
    ("a04a5439-8a0e-4cb7-975f-a5b255866883", "Cubic"),
];

/// The AAF classes pyaaf2 gives a Python class of their own.
///
/// An object of any other class is read as a plain `AAFObject`, and that
/// matters in two ways: its metadata says `ClassName: AAFObject`, and it is
/// not an instance of anything — an unregistered class that AAF says is a
/// kind of `SourceClip` is not a source clip to upstream, and so is not one
/// here either. The list is pyaaf2's `register_class` calls at the pinned
/// revision.
const REGISTERED: [(&str, &str); 74] = [
    ("0d010101-0101-2600-060e-2b3402060101", "AIFCDescriptor"),
    ("0d010101-0101-2800-060e-2b3402060101", "CDCIDescriptor"),
    ("0d010101-0201-0000-060e-2b3402060101", "ClassDef"),
    ("0d010101-0101-1f00-060e-2b3402060101", "CodecDef"),
    ("0d010101-0101-0800-060e-2b3402060101", "CommentMarker"),
    ("0d010101-0101-3500-060e-2b3402060101", "CompositionMob"),
    ("0d010101-0101-3d00-060e-2b3402060101", "ConstantValue"),
    ("0d010101-0101-2000-060e-2b3402060101", "ContainerDef"),
    ("0d010101-0101-1800-060e-2b3402060101", "ContentStorage"),
    ("0d010101-0101-1900-060e-2b3402060101", "ControlPoint"),
    ("0d010101-0101-1b00-060e-2b3402060101", "DataDef"),
    (
        "0d010101-0101-4300-060e-2b3402060101",
        "DataEssenceDescriptor",
    ),
    ("0d010101-0101-1a00-060e-2b3402060101", "DefinitionObject"),
    ("0d010101-0101-4100-060e-2b3402060101", "DescriptiveMarker"),
    ("0d010101-0101-2200-060e-2b3402060101", "Dictionary"),
    (
        "0d010101-0101-2700-060e-2b3402060101",
        "DigitalImageDescriptor",
    ),
    ("0d010101-0101-0400-060e-2b3402060101", "EdgeCode"),
    ("0d010101-0101-2300-060e-2b3402060101", "EssenceData"),
    ("0d010101-0101-2400-060e-2b3402060101", "EssenceDescriptor"),
    ("0d010101-0101-0500-060e-2b3402060101", "EssenceGroup"),
    ("0d010101-0101-3900-060e-2b3402060101", "EventMobSlot"),
    ("0d010101-0101-2500-060e-2b3402060101", "FileDescriptor"),
    ("0d010101-0101-0900-060e-2b3402060101", "Filler"),
    ("0d010101-0101-2f00-060e-2b3402060101", "Header"),
    ("0d010101-0101-4a00-060e-2b3402060101", "ImportDescriptor"),
    ("0d010101-0101-2100-060e-2b3402060101", "InterpolationDef"),
    ("0d010101-0101-3600-060e-2b3402060101", "MasterMob"),
    ("0d010101-0225-0000-060e-2b3402060101", "MetaDictionary"),
    ("0d010101-0101-3400-060e-2b3402060101", "Mob"),
    ("0d010101-0101-3800-060e-2b3402060101", "MobSlot"),
    ("0d010101-0101-4400-060e-2b3402060101", "MultipleDescriptor"),
    ("0d010101-0101-0b00-060e-2b3402060101", "NestedScope"),
    ("0d010101-0101-1c00-060e-2b3402060101", "OperationDef"),
    ("0d010101-0101-0a00-060e-2b3402060101", "OperationGroup"),
    ("0d010101-0101-4800-060e-2b3402060101", "PCMDescriptor"),
    ("0d010101-0101-1d00-060e-2b3402060101", "ParameterDef"),
    ("0d010101-0101-4900-060e-2b3402060101", "PhysicalDescriptor"),
    ("0d010101-0101-1e00-060e-2b3402060101", "PluginDef"),
    ("0d010101-0202-0000-060e-2b3402060101", "PropertyDef"),
    ("0d010101-0101-0c00-060e-2b3402060101", "Pulldown"),
    ("0d010101-0101-2900-060e-2b3402060101", "RGBADescriptor"),
    ("0d010101-0101-0d00-060e-2b3402060101", "ScopeReference"),
    ("0d010101-0101-0e00-060e-2b3402060101", "Selector"),
    ("0d010101-0101-0f00-060e-2b3402060101", "Sequence"),
    ("0d010101-0101-4200-060e-2b3402060101", "SoundDescriptor"),
    ("0d010101-0101-1100-060e-2b3402060101", "SourceClip"),
    ("0d010101-0101-3700-060e-2b3402060101", "SourceMob"),
    ("0d010101-0101-3a00-060e-2b3402060101", "StaticMobSlot"),
    ("0d010101-0101-3f00-060e-2b3402060101", "TaggedValue"),
    ("0d010101-0101-4c00-060e-2b3402060101", "TaggedValueDef"),
    ("0d010101-0101-2e00-060e-2b3402060101", "TapeDescriptor"),
    ("0d010101-0101-1400-060e-2b3402060101", "Timecode"),
    ("0d010101-0101-3b00-060e-2b3402060101", "TimelineMobSlot"),
    ("0d010101-0101-1700-060e-2b3402060101", "Transition"),
    ("0d010101-0203-0000-060e-2b3402060101", "TypeDef"),
    ("0d010101-0223-0000-060e-2b3402060101", "TypeDefCharacter"),
    ("0d010101-0207-0000-060e-2b3402060101", "TypeDefEnum"),
    ("0d010101-0220-0000-060e-2b3402060101", "TypeDefExtEnum"),
    ("0d010101-0208-0000-060e-2b3402060101", "TypeDefFixedArray"),
    (
        "0e040101-0000-0000-060e-2b3402060101",
        "TypeDefGenericCharacter",
    ),
    ("0d010101-0221-0000-060e-2b3402060101", "TypeDefIndirect"),
    ("0d010101-0204-0000-060e-2b3402060101", "TypeDefInt"),
    ("0d010101-0222-0000-060e-2b3402060101", "TypeDefOpaque"),
    ("0d010101-020d-0000-060e-2b3402060101", "TypeDefRecord"),
    ("0d010101-020e-0000-060e-2b3402060101", "TypeDefRename"),
    ("0d010101-020a-0000-060e-2b3402060101", "TypeDefSet"),
    ("0d010101-020c-0000-060e-2b3402060101", "TypeDefStream"),
    ("0d010101-020b-0000-060e-2b3402060101", "TypeDefString"),
    ("0d010101-0205-0000-060e-2b3402060101", "TypeDefStrongRef"),
    ("0d010101-0209-0000-060e-2b3402060101", "TypeDefVarArray"),
    ("0d010101-0206-0000-060e-2b3402060101", "TypeDefWeakRef"),
    ("0d010101-0101-3e00-060e-2b3402060101", "VaryingValue"),
    ("0d010101-0101-2c00-060e-2b3402060101", "WAVEDescriptor"),
    ("0d010101-0101-3c00-060e-2b3402060101", "Parameter"),
];

/// The classes above by identifier, built once.
fn registered() -> &'static HashMap<Auid, &'static str> {
    static TABLE: OnceLock<HashMap<Auid, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        REGISTERED
            .iter()
            // `Parameter` is abstract in pyaaf2 and never registered; it is
            // listed only so that `is_a` can be asked about it.
            .filter(|(_, name)| *name != "Parameter")
            .map(|(id, name)| (auid(id), *name))
            .collect()
    })
}

/// An AUID written as a constant above.
pub(crate) fn auid(text: &str) -> Auid {
    text.parse()
        .expect("the constants in this module are valid AUIDs")
}

/// A value as pyaaf2 hands it to upstream.
#[derive(Debug, Clone)]
pub(crate) enum Py {
    /// Python's `None`: a property with no data, or a reference to nothing.
    None,
    /// A `bool`, which is what AAF's `Boolean` enumeration decodes to.
    Bool(bool),
    /// An `int`.
    Int(i64),
    /// An `int` too large for `i64`.
    UInt(u64),
    /// A `float`.
    Float(f64),
    /// A `str`.
    Str(String),
    /// A `tuple`, which is how pyaaf2 returns a fixed array of integers.
    Tuple(Vec<Py>),
    /// A `list`: a variable array, or the objects a collection holds.
    List(Vec<Py>),
    /// A `set`.
    Set(Vec<Py>),
    /// A `dict`, which is what a record decodes to when pyaaf2 has no class
    /// for it.
    Dict(Vec<(String, Py)>),
    /// An `AAFRational`, which prints without reducing.
    Rational(i64, i64),
    /// An `AUID`.
    Auid(Auid),
    /// A `MobID`.
    MobId(MobId),
    /// A `date`, `time` or `datetime`, already as the text `str()` gives.
    Printed(String),
    /// An AAF object.
    Object(Object),
}

impl Py {
    /// The value as `str()` prints it, for the values that are printed.
    pub(crate) fn printed(&self) -> String {
        match self {
            Self::None => "None".to_owned(),
            Self::Bool(true) => "True".to_owned(),
            Self::Bool(false) => "False".to_owned(),
            Self::Int(v) => v.to_string(),
            Self::UInt(v) => v.to_string(),
            Self::Float(v) => python_float(*v),
            Self::Str(v) | Self::Printed(v) => v.clone(),
            Self::Tuple(items) => match items.as_slice() {
                [one] => format!("({},)", one.repr()),
                _ => format!(
                    "({})",
                    items.iter().map(Self::repr).collect::<Vec<_>>().join(", ")
                ),
            },
            Self::List(items) => format!(
                "[{}]",
                items.iter().map(Self::repr).collect::<Vec<_>>().join(", ")
            ),
            Self::Set(items) => format!(
                "{{{}}}",
                items.iter().map(Self::repr).collect::<Vec<_>>().join(", ")
            ),
            Self::Dict(members) => format!(
                "{{{}}}",
                members
                    .iter()
                    .map(|(k, v)| format!("'{k}': {}", v.repr()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Rational(n, 1) => n.to_string(),
            Self::Rational(n, d) => format!("{n}/{d}"),
            Self::Auid(v) => v.to_string(),
            Self::MobId(v) => v.to_string(),
            Self::Object(_) => "<AAFObject>".to_owned(),
        }
    }

    /// The value as `repr()` prints it, which is how it appears inside a
    /// printed container.
    fn repr(&self) -> String {
        match self {
            Self::Str(v) => format!("'{v}'"),
            other => other.printed(),
        }
    }

    /// The value as a number, the way `float()` would take it.
    pub(crate) fn as_f64(&self) -> Option<f64> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "Python's float() loses the same precision"
        )]
        match self {
            Self::Bool(v) => Some(f64::from(u8::from(*v))),
            Self::Int(v) => Some(*v as f64),
            Self::UInt(v) => Some(*v as f64),
            Self::Float(v) => Some(*v),
            Self::Rational(n, d) if *d != 0 => Some(*n as f64 / *d as f64),
            _ => None,
        }
    }

    /// The value as an integer, if it is one.
    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            Self::UInt(v) => i64::try_from(*v).ok(),
            Self::Bool(v) => Some(i64::from(*v)),
            _ => None,
        }
    }
}

/// A float as Python's `repr` writes it.
pub(crate) fn python_float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e16 {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

impl<R: Read + Seek> Transcriber<R> {
    /// The Python class pyaaf2 reads an object as.
    ///
    /// Used as `ClassName` in metadata, which is upstream's `_get_class_name`.
    pub(crate) fn py_class(&self, object: &Object) -> &'static str {
        registered()
            .get(&object.class_id())
            .copied()
            .unwrap_or("AAFObject")
    }

    /// Whether an object is an instance of a pyaaf2 class.
    ///
    /// The object has to be of a class pyaaf2 registers — an unregistered one
    /// is a bare `AAFObject` whatever AAF says it descends from — and that
    /// class has to descend from the one asked about, by AAF's own
    /// inheritance, which pyaaf2's Python classes follow.
    pub(crate) fn py_is(&self, object: &Object, aaf_class: &str) -> bool {
        registered().contains_key(&object.class_id()) && self.aaf.is_a(object, aaf_class)
    }

    /// What pyaaf2's `name` attribute gives for an object.
    ///
    /// Every object has one: most classes fall back to the name of their AAF
    /// class, so a filler's name is "Filler". A mob and a definition read
    /// their `Name`, a slot its `SlotName`, a tagged value its `Name` and a
    /// parameter the name of the parameter definition it points at. `None`
    /// means the attribute is there but empty, or, for a parameter whose
    /// definition cannot be found, that asking for it fails.
    pub(crate) fn py_name(&mut self, object: &Object) -> Result<Option<String>> {
        let property = if self.py_is(object, "Mob")
            || self.py_is(object, "DefinitionObject")
            || self.py_is(object, "TaggedValue")
        {
            "Name"
        } else if self.py_is(object, "MobSlot") {
            "SlotName"
        } else if self.py_is(object, "Parameter") {
            return self.parameter_name(object);
        } else {
            return Ok(Some(
                self.aaf.class_name(object).unwrap_or_default().to_owned(),
            ));
        };
        Ok(match self.value_of(object, property)? {
            Py::Str(name) => Some(name),
            _ => None,
        })
    }

    /// The name of the definition a parameter points at.
    fn parameter_name(&mut self, parameter: &Object) -> Result<Option<String>> {
        let Some(definition) = self.parameter_def(parameter)? else {
            return Ok(None);
        };
        Ok(match self.value_of(&definition, "Name")? {
            Py::Str(name) => Some(name),
            _ => None,
        })
    }

    /// The parameter definition a parameter points at, if the file has it.
    pub(crate) fn parameter_def(&mut self, parameter: &Object) -> Result<Option<Object>> {
        let Py::Auid(key) = self.value_of(parameter, "Definition")? else {
            return Ok(None);
        };
        Ok(self.definitions()?.get(&key).cloned())
    }

    /// Whether pyaaf2's object has a `value` attribute.
    pub(crate) fn py_has_value(&self, object: &Object) -> bool {
        self.py_is(object, "TaggedValue")
            || self.py_is(object, "ConstantValue")
            || self.py_is(object, "ControlPoint")
    }

    /// What pyaaf2's `value` attribute gives, for an object that has one.
    ///
    /// A control point's is its value as a float; the others' is their
    /// `Value` property.
    pub(crate) fn py_value(&mut self, object: &Object) -> Result<Option<Py>> {
        let value = self.value_of(object, "Value")?;
        if self.py_is(object, "ControlPoint") {
            return Ok(value.as_f64().map(Py::Float));
        }
        Ok(Some(value))
    }

    /// A named property's value, as pyaaf2 gives it.
    ///
    /// A class that does not define the property, and an object that does not
    /// carry it, both give `None`.
    pub(crate) fn value_of(&mut self, object: &Object, name: &str) -> Result<Py> {
        let Ok(pid) = self.aaf.pid(object, name) else {
            return Ok(Py::None);
        };
        let Some(property) = object.get(pid).cloned() else {
            return Ok(Py::None);
        };
        self.property_py(object, &property, 0)
    }

    /// A property's value, as pyaaf2's property object gives it.
    fn property_py(&mut self, owner: &Object, property: &Property, depth: u32) -> Result<Py> {
        match &property.value {
            PropertyValue::Data(data) => {
                let Some(def) = self.aaf.metadict().property(owner.class_id(), property.pid) else {
                    return Ok(Py::None);
                };
                let type_id = def.type_id;
                Ok(self.decode(type_id, data, depth).unwrap_or(Py::None))
            }
            PropertyValue::StrongRef { .. } => Ok(self
                .aaf
                .file()
                .strong_ref(owner, property)
                .map_or(Py::None, Py::Object)),
            PropertyValue::StrongRefVector { .. } => Ok(Py::List(
                self.aaf
                    .file()
                    .strong_ref_vector(owner, property)?
                    .into_iter()
                    .map(Py::Object)
                    .collect(),
            )),
            PropertyValue::StrongRefSet { .. } => Ok(Py::List(
                self.aaf
                    .file()
                    .strong_ref_set(owner, property)?
                    .into_iter()
                    .map(|(_, object)| Py::Object(object))
                    .collect(),
            )),
            PropertyValue::WeakRef { key, .. } => {
                Ok(self.resolve(*key)?.map_or(Py::None, Py::Object))
            }
            PropertyValue::WeakRefArray { .. } => {
                let index = self.aaf.file().weak_ref_array(owner, property)?;
                let mut out = Vec::with_capacity(index.keys.len());
                for key in index.keys {
                    out.push(self.resolve(key)?.map_or(Py::None, Py::Object));
                }
                Ok(Py::List(out))
            }
            // A stream is essence rather than a value; nothing upstream
            // transcribes carries one.
            _ => Ok(Py::None),
        }
    }

    /// Bytes decoded against a type, the way pyaaf2's type classes do it.
    fn decode(&self, type_id: Auid, data: &[u8], depth: u32) -> Option<Py> {
        if depth > MAX_DEPTH {
            return None;
        }
        let metadict = self.aaf.metadict();
        let id = type_id.to_string();
        match id.as_str() {
            AUID_TYPE | MOB_ID_TYPE => {
                return match metadict.decode_bytes(type_id, data, 0).ok()? {
                    Value::Auid(v) => Some(Py::Auid(v)),
                    Value::MobId(v) => Some(Py::MobId(v)),
                    _ => None,
                };
            }
            BOOLEAN_TYPE => return Some(Py::Bool(data == [1])),
            _ => {}
        }
        let def = metadict.type_def(type_id)?;
        match &def.kind {
            TypeKind::Int { .. } => match metadict.decode_bytes(type_id, data, 0).ok()? {
                Value::Int(v) => Some(Py::Int(v)),
                Value::UInt(v) => Some(i64::try_from(v).map_or(Py::UInt(v), Py::Int)),
                _ => None,
            },
            TypeKind::Enum { .. } => match metadict.decode_bytes(type_id, data, 0).ok()? {
                Value::Enum {
                    name: Some(name), ..
                } => Some(Py::Str(name)),
                Value::Enum { value, .. } => Some(Py::Int(value)),
                _ => None,
            },
            TypeKind::ExtEnum { .. } => match metadict.decode_bytes(type_id, data, 0).ok()? {
                Value::ExtEnum {
                    name: Some(name), ..
                } => Some(Py::Str(name)),
                Value::ExtEnum { value, .. } => Some(Py::Auid(value)),
                _ => None,
            },
            TypeKind::String { .. } | TypeKind::Character | TypeKind::Stream => {
                Some(Py::Str(utf16_until_nul(data)))
            }
            TypeKind::Rename { renamed } => self.decode(*renamed, data, depth + 1),
            TypeKind::Indirect | TypeKind::Opaque => {
                let (&mark, rest) = data.split_first()?;
                if mark != 0x4c || rest.len() < 16 {
                    return None;
                }
                let bytes: [u8; 16] = rest[..16].try_into().ok()?;
                let inner = Auid::from_bytes_le(bytes);
                self.decode(inner, &rest[16..], depth + 1)
            }
            TypeKind::Record { members } => self.decode_record(type_id, members, data, depth),
            TypeKind::FixedArray {
                element_type,
                count,
            } => {
                let is_int = matches!(
                    metadict.type_def(*element_type).map(|def| &def.kind),
                    Some(TypeKind::Int { .. })
                );
                let items = self.decode_elements(*element_type, data, Some(*count), depth)?;
                Some(if is_int {
                    Py::Tuple(items)
                } else {
                    Py::List(items)
                })
            }
            TypeKind::VarArray { element_type } => {
                if element_type.to_string() == CHARACTER_TYPE {
                    return Some(Py::List(
                        utf16_strings(data).into_iter().map(Py::Str).collect(),
                    ));
                }
                Some(Py::List(self.decode_elements(
                    *element_type,
                    data,
                    None,
                    depth,
                )?))
            }
            TypeKind::Set { element_type } => {
                let mut items = self.decode_elements(*element_type, data, None, depth)?;
                // Python iterates a set of small integers in ascending order,
                // which is the only kind of set these files hold.
                items.sort_by_key(|item| item.as_i64().unwrap_or(i64::MAX));
                items.dedup_by(|a, b| a.as_i64().is_some() && a.as_i64() == b.as_i64());
                Some(Py::Set(items))
            }
            _ => None,
        }
    }

    /// A record, as pyaaf2 decodes it: a Python object for the few it knows,
    /// a dictionary of its members otherwise.
    fn decode_record(
        &self,
        type_id: Auid,
        members: &[(String, Auid)],
        data: &[u8],
        depth: u32,
    ) -> Option<Py> {
        let metadict = self.aaf.metadict();
        let mut out = Vec::with_capacity(members.len());
        let mut at = 0;
        for (name, member_type) in members {
            let size = metadict.byte_size(*member_type, 0)?;
            let bytes = data.get(at..at + size)?;
            out.push((name.clone(), self.decode(*member_type, bytes, depth + 1)?));
            at += size;
        }
        let int = |name: &str| {
            out.iter()
                .find(|(found, _)| found == name)
                .and_then(|(_, value)| value.as_i64())
        };
        let id = type_id.to_string();
        match id.as_str() {
            RATIONAL_TYPE => {
                let (n, d) = (int("Numerator")?, int("Denominator")?);
                // pyaaf2 reads 0/0, which Storyboard Pro writes, as 0/1.
                return Some(match (n, d) {
                    (0, 0) => Py::Rational(0, 1),
                    _ => Py::Rational(n, d),
                });
            }
            DATE_TYPE => {
                if let Some(text) = date_text(int("year")?, int("month")?, int("day")?) {
                    return Some(Py::Printed(text));
                }
            }
            TIME_TYPE => {
                if let Some(text) = time_text(
                    int("hour")?,
                    int("minute")?,
                    int("second")?,
                    int("fraction")?,
                ) {
                    return Some(Py::Printed(text));
                }
            }
            TIMESTAMP_TYPE => {
                let part = |name: &str| {
                    out.iter()
                        .find(|(found, _)| found == name)
                        .map(|(_, value)| value)
                };
                // Only a date that decoded to a date is combined, and only
                // with a time that decoded to a time; otherwise the record
                // stays a dictionary, with whatever did decode inside it.
                if let (Some(Py::Printed(date)), Some(Py::Printed(time))) =
                    (part("date"), part("time"))
                {
                    return Some(Py::Printed(format!("{date} {time}")));
                }
            }
            _ => {}
        }
        Some(Py::Dict(out))
    }

    /// A run of same-typed elements laid out back to back.
    fn decode_elements(
        &self,
        element_type: Auid,
        data: &[u8],
        count: Option<u32>,
        depth: u32,
    ) -> Option<Vec<Py>> {
        let size = self.aaf.metadict().byte_size(element_type, 0)?;
        if size == 0 {
            return None;
        }
        let count = count.map_or(data.len() / size, |count| count as usize);
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let bytes = data.get(index * size..(index + 1) * size)?;
            out.push(self.decode(element_type, bytes, depth + 1)?);
        }
        Some(out)
    }

    /// An AAF object's properties, as the `AAF` metadata dictionary.
    ///
    /// Upstream's `_transcribe_aaf_object_properties`: the Python class name
    /// as `ClassName`, then every property the object carries.
    pub(crate) fn object_properties(&mut self, object: &Object) -> Result<AnyDictionary> {
        let mut out = BTreeMap::new();
        out.insert(
            "ClassName".to_owned(),
            Any::String(self.py_class(object).to_owned()),
        );
        for (name, property) in self.named_properties(object) {
            let value = self.property_py(object, &property, 0)?;
            let value = self.transcribe_property(&value, 0)?;
            out.insert(name, value);
        }
        Ok(out)
    }

    /// An object's properties with the names its class gives them, in the
    /// order they are stored.
    fn named_properties(&self, object: &Object) -> Vec<(String, Property)> {
        object
            .properties()
            .iter()
            .filter_map(|property| {
                let def = self
                    .aaf
                    .metadict()
                    .property(object.class_id(), property.pid)?;
                Some((def.name.clone(), property.clone()))
            })
            .collect()
    }

    /// Upstream's `_transcribe_property`: a Python value as OTIO metadata.
    pub(crate) fn transcribe_property(&mut self, value: &Py, depth: u32) -> Result<Any> {
        if depth > MAX_DEPTH {
            return Ok(Any::Null);
        }
        Ok(match value {
            Py::Str(text) => Any::String(text.clone()),
            Py::Bool(v) => Any::Bool(*v),
            Py::Int(v) => Any::Int(*v),
            Py::UInt(v) => Any::UInt(*v),
            Py::Float(v) => Any::Double(*v),
            Py::Dict(members) => {
                let mut out = BTreeMap::new();
                for (key, member) in members {
                    out.insert(key.clone(), self.transcribe_property(member, depth + 1)?);
                }
                Any::Dictionary(out)
            }
            // `list(prop)`, and then whatever OTIO makes of each element.
            Py::Set(items) => Any::Vector(items.iter().map(plain_any).collect()),
            Py::List(items) => Any::Dictionary(self.transcribe_list(items, depth)?),
            Py::Object(object) => Any::Dictionary(self.transcribe_object(object, depth)?),
            other => Any::String(other.printed()),
        })
    }

    /// A list, keyed by the names of the objects in it.
    fn transcribe_list(&mut self, items: &[Py], depth: u32) -> Result<AnyDictionary> {
        let mut out = BTreeMap::new();
        for item in items {
            // Only AAF objects have a `name`; numbers, strings and records
            // in a list are skipped.
            let Py::Object(child) = item else { continue };
            let Some(name) = self.py_name(child)? else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            if self.py_is(child, "VaryingValue") {
                let keyframes = self.keyframes(child)?;
                out.insert(name, keyframes);
            } else if self.py_is(child, "ParameterDefinition") {
                let value = self.transcribe_object(child, depth + 1)?;
                out.insert(name, Any::Dictionary(value));
            } else if self.py_has_value(child) {
                if let Some(value) = self.py_value(child)? {
                    let value = self.transcribe_property(&value, depth + 1)?;
                    out.insert(name, value);
                }
            }
        }
        Ok(out)
    }

    /// An AAF object met as a value: its class and its properties, with only
    /// data definitions followed among its weak references.
    fn transcribe_object(&mut self, object: &Object, depth: u32) -> Result<AnyDictionary> {
        let mut out = BTreeMap::new();
        out.insert(
            "ClassName".to_owned(),
            Any::String(self.py_class(object).to_owned()),
        );
        for (name, property) in self.named_properties(object) {
            let value = self.property_py(object, &property, depth + 1)?;
            if matches!(property.value, PropertyValue::WeakRef { .. }) {
                let is_datadef = matches!(&value, Py::Object(target)
                    if self.py_is(target, "DataDefinition"));
                if !is_datadef {
                    continue;
                }
            }
            let value = self.transcribe_property(&value, depth + 1)?;
            out.insert(name, value);
        }
        Ok(out)
    }

    /// A varying value's keyframes, as upstream records them.
    fn keyframes(&mut self, varying: &Object) -> Result<Any> {
        let mut points = Vec::new();
        for point in self.control_points(varying)? {
            let time = self.value_of(&point, "Time")?.as_f64();
            let value = self.value_of(&point, "Value")?.as_f64();
            // A value `float()` cannot take is skipped, as upstream skips
            // what it cannot transcribe.
            let (Some(time), Some(value)) = (time, value) else {
                continue;
            };
            points.push(Any::Vector(vec![Any::Double(time), Any::Double(value)]));
        }
        let interpolation = self.interpolation_name(varying)?;
        let mut out = BTreeMap::new();
        out.insert("_aaf_keyframed_property".to_owned(), Any::Bool(true));
        out.insert("keyframe_values".to_owned(), Any::Vector(points));
        out.insert(
            "keyframe_interpolation".to_owned(),
            Any::String(interpolation.to_owned()),
        );
        out.insert("keyframe_baked_values".to_owned(), Any::Null);
        Ok(Any::Dictionary(out))
    }

    /// A varying value's control points, in order.
    pub(crate) fn control_points(&mut self, varying: &Object) -> Result<Vec<Object>> {
        Ok(match self.value_of(varying, "PointList")? {
            Py::List(items) => items
                .into_iter()
                .filter_map(|item| match item {
                    Py::Object(object) => Some(object),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
    }

    /// Upstream's name for how a varying value interpolates.
    pub(crate) fn interpolation_name(&mut self, varying: &Object) -> Result<&'static str> {
        let Py::Object(definition) = self.value_of(varying, "Interpolation")? else {
            return Ok("Linear");
        };
        let Py::Auid(id) = self.value_of(&definition, "Identification")? else {
            return Ok("Linear");
        };
        let id = id.to_string();
        Ok(INTERPOLATIONS
            .iter()
            .find(|(key, _)| *key == id)
            .map_or("Linear", |(_, name)| name))
    }
}

/// A value inside a list OTIO was handed, as OTIO stores it.
fn plain_any(value: &Py) -> Any {
    match value {
        Py::Bool(v) => Any::Bool(*v),
        Py::Int(v) => Any::Int(*v),
        Py::UInt(v) => Any::UInt(*v),
        Py::Float(v) => Any::Double(*v),
        Py::Str(v) => Any::String(v.clone()),
        Py::None => Any::Null,
        other => Any::String(other.printed()),
    }
}

/// UTF-16LE text up to the first NUL, as pyaaf2's `decode_utf16le` reads it.
fn utf16_until_nul(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// A run of NUL-terminated UTF-16LE strings.
fn utf16_strings(data: &[u8]) -> Vec<String> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let mut out = Vec::new();
    let mut current = Vec::new();
    for unit in units {
        if unit == 0 {
            out.push(String::from_utf16_lossy(&current));
            current.clear();
        } else {
            current.push(unit);
        }
    }
    if !current.is_empty() {
        out.push(String::from_utf16_lossy(&current));
    }
    out
}

/// A date as Python's `str(date)` prints it, if Python would accept it.
fn date_text(year: i64, month: i64, day: i64) -> Option<String> {
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=days).contains(&day) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// A time as Python's `str(time)` prints it, if Python would accept it.
///
/// pyaaf2 passes AAF's `fraction` as microseconds, so a nonzero one prints.
fn time_text(hour: i64, minute: i64, second: i64, fraction: i64) -> Option<String> {
    if !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
        || !(0..1_000_000).contains(&fraction)
    {
        return None;
    }
    Some(if fraction == 0 {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{hour:02}:{minute:02}:{second:02}.{fraction:06}")
    })
}

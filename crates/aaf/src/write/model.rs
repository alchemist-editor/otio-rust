//! The meta dictionary of a file being written.
//!
//! A new file carries its whole data model: every class, property and type
//! definition, each as an object of its own. pyaaf2 builds them one by one
//! in a fixed order — the `Root` class, the standard classes, their aliases,
//! the two types `Root` needs, then the standard types category by category —
//! and after the file's first objects are in place, the extension model the
//! same way. The order is part of the file: it decides the local key each
//! definition is filed under and the order the definitions are written in.
//!
//! [`Model`] is the part of that a writer consults: which class defines
//! which property, what each type is. The definition objects themselves live
//! with every other object, and each change here is made to both.

use std::collections::{HashMap, HashSet};

use super::object::{
    SF_DATA, SF_DATA_STREAM, SF_STRONG, SF_STRONG_SET, SF_STRONG_VECTOR, SF_WEAK, SF_WEAK_SET,
    SF_WEAK_VECTOR, encode_string,
};
use super::{AafWriter, ObjRef};
use crate::Auid;
use crate::builtin::raw;
use crate::builtin::{Class, EnumType, ExtEnumType, ExtType, RecordType, auid};
use crate::error::{Error, Result};

pub(crate) const METADICT_CLASS: Auid = auid("0d010101-0225-0000-060e-2b3402060101");
const CLASSDEF_CLASS: Auid = auid("0d010101-0201-0000-060e-2b3402060101");
const PROPERTYDEF_CLASS: Auid = auid("0d010101-0202-0000-060e-2b3402060101");
pub(crate) const PARAMETER_CLASS: Auid = auid("0d010101-0101-3c00-060e-2b3402060101");
pub(crate) const MOB_CLASS: Auid = auid("0d010101-0101-3400-060e-2b3402060101");
const ESSENCEDATA_CLASS: Auid = auid("0d010101-0101-2300-060e-2b3402060101");
const GENERIC_CHARACTER_CLASS: Auid = auid("0e040101-0000-0000-060e-2b3402060101");
const GENERIC_CHARACTER_SIZE: Auid = auid("0e040101-0101-0111-060e-2b3401010101");

/// The weak reference paths to the class and type definitions.
const CLASSDEFS_PATH: [u16; 2] = [0x0001, 0x0003];
const TYPEDEFS_PATH: [u16; 2] = [0x0001, 0x0004];

const PID_NAME: u16 = 0x0006;
const PID_AUID: u16 = 0x0005;
const PID_CLASSDEFS: u16 = 0x0003;
const PID_TYPEDEFS: u16 = 0x0004;

/// One class definition.
#[derive(Debug, Clone)]
pub(crate) struct ClassInfo {
    pub(crate) name: String,
    pub(crate) auid: Auid,
    /// The parent's identifier; a root class names itself.
    pub(crate) parent: Auid,
    pub(crate) concrete: bool,
    /// The properties this class adds, in the order its definition holds them.
    pub(crate) props: Vec<usize>,
    by_pid: HashMap<u16, usize>,
    pub(crate) obj: ObjRef,
}

/// One property definition.
#[derive(Debug, Clone)]
pub(crate) struct PropInfo {
    pub(crate) name: String,
    pub(crate) auid: Auid,
    pub(crate) pid: u16,
    pub(crate) type_id: Auid,
    pub(crate) optional: bool,
    pub(crate) unique: bool,
}

/// One type definition.
#[derive(Debug, Clone)]
pub(crate) struct TypeInfo {
    pub(crate) name: String,
    pub(crate) auid: Auid,
    pub(crate) kind: Kind,
    pub(crate) obj: ObjRef,
}

/// What a type is, with what the writer needs to encode one.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    Int {
        size: u8,
        signed: bool,
    },
    /// The element names and values as stored, which may repeat a value.
    Enum {
        element: Auid,
        names: Vec<String>,
        values: Vec<i64>,
    },
    ExtEnum {
        names: Vec<String>,
        values: Vec<Auid>,
    },
    Record {
        members: Vec<(String, Auid)>,
    },
    FixedArray {
        element: Auid,
        count: u32,
    },
    VarArray {
        element: Auid,
    },
    Set {
        element: Auid,
    },
    String {
        element: Auid,
    },
    Rename {
        renamed: Auid,
    },
    StrongRef {
        class: Auid,
    },
    WeakRef {
        class: Auid,
        target_set: Vec<Auid>,
    },
    Stream,
    Opaque,
    Character,
    GenericCharacter,
    Indirect,
}

impl Kind {
    /// The class of the definition object for this kind of type.
    fn class_id(&self) -> Auid {
        match self {
            Self::Int { .. } => auid("0d010101-0204-0000-060e-2b3402060101"),
            Self::StrongRef { .. } => auid("0d010101-0205-0000-060e-2b3402060101"),
            Self::WeakRef { .. } => auid("0d010101-0206-0000-060e-2b3402060101"),
            Self::Enum { .. } => auid("0d010101-0207-0000-060e-2b3402060101"),
            Self::FixedArray { .. } => auid("0d010101-0208-0000-060e-2b3402060101"),
            Self::VarArray { .. } => auid("0d010101-0209-0000-060e-2b3402060101"),
            Self::Set { .. } => auid("0d010101-020a-0000-060e-2b3402060101"),
            Self::String { .. } => auid("0d010101-020b-0000-060e-2b3402060101"),
            Self::Stream => auid("0d010101-020c-0000-060e-2b3402060101"),
            Self::Record { .. } => auid("0d010101-020d-0000-060e-2b3402060101"),
            Self::Rename { .. } => auid("0d010101-020e-0000-060e-2b3402060101"),
            Self::ExtEnum { .. } => auid("0d010101-0220-0000-060e-2b3402060101"),
            Self::Indirect => auid("0d010101-0221-0000-060e-2b3402060101"),
            Self::Opaque => auid("0d010101-0222-0000-060e-2b3402060101"),
            Self::Character => auid("0d010101-0223-0000-060e-2b3402060101"),
            Self::GenericCharacter => GENERIC_CHARACTER_CLASS,
        }
    }
}

/// An enumeration's elements as pyaaf2 sees them: a dict from value to
/// name, so a repeated value keeps its first place and its last name.
pub(crate) fn dict_elements<K: PartialEq + Copy>(
    values: &[K],
    names: &[String],
) -> Vec<(K, String)> {
    let mut out: Vec<(K, String)> = Vec::new();
    for (value, name) in values.iter().zip(names) {
        match out.iter_mut().find(|(v, _)| v == value) {
            Some(slot) => slot.1.clone_from(name),
            None => out.push((*value, name.clone())),
        }
    }
    out
}

/// The classes, properties and types a writer knows.
#[derive(Debug, Default, Clone)]
pub(crate) struct Model {
    pub(crate) classes: Vec<ClassInfo>,
    class_by_name: HashMap<String, usize>,
    class_by_auid: HashMap<Auid, usize>,
    pub(crate) props: Vec<PropInfo>,
    pub(crate) types: Vec<TypeInfo>,
    type_by_name: HashMap<String, usize>,
    type_by_auid: HashMap<Auid, usize>,
    local_pids: HashSet<u16>,
    next_pid: u32,
}

impl Model {
    pub(crate) fn new() -> Self {
        Self {
            next_pid: 0xffff,
            ..Self::default()
        }
    }

    pub(crate) fn class_index(&self, auid: Auid) -> Option<usize> {
        self.class_by_auid.get(&auid).copied()
    }

    pub(crate) fn class_named(&self, name: &str) -> Option<usize> {
        self.class_by_name.get(name).copied()
    }

    pub(crate) fn type_def(&self, auid: Auid) -> Option<&TypeInfo> {
        self.type_by_auid.get(&auid).map(|&i| &self.types[i])
    }

    pub(crate) fn type_named(&self, name: &str) -> Option<&TypeInfo> {
        self.type_by_name.get(name).map(|&i| &self.types[i])
    }

    /// The class a class inherits from, or `None` at the root.
    fn parent(&self, class: usize) -> Option<usize> {
        let c = &self.classes[class];
        if c.parent == c.auid {
            return None;
        }
        self.class_index(c.parent)
    }

    /// The class and its ancestors, nearest first.
    pub(crate) fn relatives(&self, class: usize) -> Vec<usize> {
        let mut out = vec![class];
        let mut current = class;
        while let Some(parent) = self.parent(current) {
            if out.contains(&parent) {
                break;
            }
            out.push(parent);
            current = parent;
        }
        out
    }

    /// Every property an object of the class can hold: its own first, then
    /// each ancestor's.
    pub(crate) fn all_propertydefs(&self, class: usize) -> Vec<usize> {
        self.relatives(class)
            .into_iter()
            .flat_map(|c| self.classes[c].props.iter().copied())
            .collect()
    }

    /// The definition of a property of an object of this class, by
    /// identifier.
    pub(crate) fn propdef_for_pid(&self, class: usize, pid: u16) -> Option<usize> {
        self.relatives(class)
            .into_iter()
            .find_map(|c| self.classes[c].by_pid.get(&pid).copied())
    }

    /// pyaaf2's `ClassDef.isinstance`: whether `other` is `class` or derives
    /// from it.
    pub(crate) fn is_instance(&self, class: usize, other: usize) -> bool {
        let auid = self.classes[class].auid;
        self.relatives(other)
            .into_iter()
            .any(|c| self.classes[c].auid == auid)
    }

    /// Whether `class` is the class `ancestor` or derives from it.
    pub(crate) fn derives_from(&self, class: usize, ancestor: Auid) -> bool {
        self.relatives(class)
            .into_iter()
            .any(|c| self.classes[c].auid == ancestor)
    }

    /// The identifier of the property objects of this class are keyed by.
    ///
    /// Parameters have none in the model; pyaaf2 uses the identifier of
    /// `DefinitionObject::Identification` for them, as the AAF SDK does. It
    /// asks whether `Parameter` derives from the class rather than the other
    /// way round, and that is kept.
    pub(crate) fn unique_key_pid(&self, class: usize) -> Option<u16> {
        for p in self.all_propertydefs(class) {
            if self.props[p].unique {
                return Some(self.props[p].pid);
            }
        }
        let parameter = self.class_index(PARAMETER_CLASS)?;
        self.is_instance(class, parameter).then_some(0x1b01)
    }

    /// The size of that key: 32 bytes for mobs and essence, 16 otherwise.
    pub(crate) fn unique_key_size(&self, class: usize) -> u8 {
        let is = |id| {
            self.class_index(id)
                .is_some_and(|other| self.is_instance(class, other))
        };
        if is(MOB_CLASS) || is(ESSENCEDATA_CLASS) {
            32
        } else {
            16
        }
    }

    /// How a property of this type is stored.
    pub(crate) fn store_format(&self, type_id: Auid) -> Result<u8> {
        let t = self
            .type_def(type_id)
            .ok_or(Error::UndefinedType { type_id })?;
        Ok(match &t.kind {
            Kind::StrongRef { .. } => SF_STRONG,
            Kind::WeakRef { .. } => SF_WEAK,
            Kind::Stream => SF_DATA_STREAM,
            Kind::VarArray { element } => match self.store_format(*element)? {
                SF_WEAK => SF_WEAK_VECTOR,
                SF_STRONG => SF_STRONG_VECTOR,
                _ => SF_DATA,
            },
            Kind::Set { element } => match self.store_format(*element)? {
                SF_STRONG => SF_STRONG_SET,
                SF_WEAK => SF_WEAK_SET,
                SF_DATA => SF_DATA,
                _ => {
                    return Err(Error::InvalidValue {
                        type_name: t.name.clone(),
                        reason: "a set of that element type cannot be stored".to_owned(),
                    });
                }
            },
            _ => SF_DATA,
        })
    }

    /// The class a reference type, or a collection of references, refers to.
    pub(crate) fn ref_classdef(&self, type_id: Auid) -> Option<usize> {
        match &self.type_def(type_id)?.kind {
            Kind::StrongRef { class } | Kind::WeakRef { class, .. } => self.class_index(*class),
            Kind::VarArray { element } | Kind::Set { element } => self.ref_classdef(*element),
            _ => None,
        }
    }

    /// pyaaf2's `next_free_pid`: the next identifier down from `0xffff` that
    /// is not taken.
    fn next_free_pid(&mut self) -> Result<u16> {
        loop {
            let pid = u16::try_from(self.next_pid)
                .ok()
                .filter(|p| *p >= 0x8000)
                .ok_or(Error::Unsupported {
                    what: "more dynamic property identifiers than there are",
                })?;
            self.next_pid -= 1;
            if !self.local_pids.contains(&pid) {
                self.local_pids.insert(pid);
                return Ok(pid);
            }
        }
    }
}

/// Adds a string property the way pyaaf2's `add_*_property` helpers do.
fn string(text: &str) -> Vec<u8> {
    encode_string(text)
}

fn utf16_array<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<u8> {
    items.into_iter().flat_map(encode_string).collect()
}

fn auid_array(items: &[Auid]) -> Vec<u8> {
    items.iter().flat_map(|a| a.to_bytes_le()).collect()
}

fn s64_array(items: &[i64]) -> Vec<u8> {
    items.iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl AafWriter {
    // --- class definitions ----------------------------------------------

    /// pyaaf2's `MetaDictionary.register_classdef`.
    pub(crate) fn register_classdef(&mut self, class: &Class) -> Result<usize> {
        let index = if let Some(i) = self.model.class_named(class.name) {
            i
        } else if self.model.class_index(class.auid).is_some() {
            return Err(Error::Unsupported {
                what: "a class registered under a second name",
            });
        } else {
            let obj = self.new_obj(CLASSDEF_CLASS);
            self.put_data(obj, PID_NAME, string(class.name));
            self.put_data(obj, PID_AUID, class.auid.to_bytes_le().to_vec());
            self.put_data(obj, 0x000a, vec![u8::from(class.concrete)]);
            self.add_weakref(
                obj,
                0x0008,
                &CLASSDEFS_PATH,
                class.parent.unwrap_or(class.auid),
            )?;
            self.add_set_property(obj, 0x0009, "Properties", PID_AUID, 16);
            self.model.classes.push(ClassInfo {
                name: class.name.to_owned(),
                auid: class.auid,
                parent: class.parent.unwrap_or(class.auid),
                concrete: class.concrete,
                props: Vec::new(),
                by_pid: HashMap::new(),
                obj,
            });
            self.model.classes.len() - 1
        };

        for prop in class.properties {
            let pid = match prop.pid {
                None => self.model.next_free_pid()?,
                Some(pid) => {
                    if pid >= 0x8000 && !self.model.local_pids.insert(pid) {
                        return Err(Error::Unsupported {
                            what: "a property identifier registered twice",
                        });
                    }
                    pid
                }
            };
            self.register_propertydef_on(
                index,
                prop.name,
                prop.auid,
                Some(pid),
                prop.type_id,
                prop.optional,
                prop.unique,
            )?;
        }

        let (name, auid, obj) = {
            let c = &self.model.classes[index];
            (c.name.clone(), c.auid, c.obj)
        };
        self.model
            .class_by_name
            .insert(class.name.to_owned(), index);
        self.model.class_by_auid.insert(auid, index);
        if name != "Root" {
            self.add2set(
                self.metadict,
                PID_CLASSDEFS,
                auid.to_bytes_le().to_vec(),
                obj,
            )?;
        }
        Ok(index)
    }

    /// pyaaf2's `ClassDef.register_propertydef`. Returns the definition's
    /// index; a property the class already defines is left as it is.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn register_propertydef_on(
        &mut self,
        class: usize,
        name: &str,
        prop_auid: Auid,
        pid: Option<u16>,
        type_id: Auid,
        optional: bool,
        unique: bool,
    ) -> Result<usize> {
        if let Some(&existing) = self.model.classes[class]
            .props
            .iter()
            .find(|&&p| self.model.props[p].auid == prop_auid)
        {
            return Ok(existing);
        }
        let pid = match pid {
            Some(pid) => pid,
            None => self.model.next_free_pid()?,
        };

        let obj = self.new_obj(PROPERTYDEF_CLASS);
        self.put_data(obj, PID_NAME, string(name));
        self.put_data(obj, 0x000c, vec![u8::from(optional)]);
        self.put_data(obj, 0x000e, vec![u8::from(unique)]);
        self.put_data(obj, 0x000d, pid.to_le_bytes().to_vec());
        self.put_data(obj, PID_AUID, prop_auid.to_bytes_le().to_vec());
        self.put_data(obj, 0x000b, type_id.to_bytes_le().to_vec());

        self.model.props.push(PropInfo {
            name: name.to_owned(),
            auid: prop_auid,
            pid,
            type_id,
            optional,
            unique,
        });
        let index = self.model.props.len() - 1;

        let class_obj = self.model.classes[class].obj;
        self.add2set(class_obj, 0x0009, prop_auid.to_bytes_le().to_vec(), obj)?;
        let c = &mut self.model.classes[class];
        c.props.push(index);
        c.by_pid.insert(pid, index);
        Ok(index)
    }

    /// A weak reference to a class or type definition, as pyaaf2's
    /// `add_weakref_property` makes one.
    fn add_weakref(&mut self, obj: ObjRef, pid: u16, path: &[u16], target: Auid) -> Result<()> {
        let index = self.weakref_index(path)?;
        let data = Self::weakref_data(index, PID_AUID, &target.to_bytes_le());
        self.put_prop(
            obj,
            super::object::Prop {
                pid,
                format: SF_WEAK,
                data,
                body: super::object::Body::Weak,
            },
        );
        Ok(())
    }

    // --- type definitions -----------------------------------------------

    /// Makes the definition object for a type and files it in the meta
    /// dictionary, replacing any type of the same identifier.
    pub(crate) fn register_typedef(
        &mut self,
        name: &str,
        type_auid: Auid,
        kind: Kind,
    ) -> Result<usize> {
        let obj = self.new_obj(kind.class_id());
        self.put_data(obj, PID_NAME, string(name));
        self.put_data(obj, PID_AUID, type_auid.to_bytes_le().to_vec());
        match &kind {
            Kind::Int { size, signed } => {
                self.put_data(obj, 0x000f, vec![*size]);
                self.put_data(obj, 0x0010, vec![u8::from(*signed)]);
            }
            Kind::StrongRef { class } => self.add_weakref(obj, 0x0011, &CLASSDEFS_PATH, *class)?,
            Kind::WeakRef { class, target_set } => {
                self.add_weakref(obj, 0x0012, &CLASSDEFS_PATH, *class)?;
                self.put_data(obj, 0x0013, auid_array(target_set));
            }
            Kind::Enum {
                element,
                names,
                values,
            } => {
                self.add_weakref(obj, 0x0014, &TYPEDEFS_PATH, *element)?;
                self.put_data(obj, 0x0015, utf16_array(names.iter().map(String::as_str)));
                self.put_data(obj, 0x0016, s64_array(values));
            }
            Kind::FixedArray { element, count } => {
                self.add_weakref(obj, 0x0017, &TYPEDEFS_PATH, *element)?;
                self.put_data(obj, 0x0018, count.to_le_bytes().to_vec());
            }
            Kind::VarArray { element } => {
                self.add_weakref(obj, 0x0019, &TYPEDEFS_PATH, *element)?
            }
            Kind::Set { element } => self.add_weakref(obj, 0x001a, &TYPEDEFS_PATH, *element)?,
            Kind::String { element } => self.add_weakref(obj, 0x001b, &TYPEDEFS_PATH, *element)?,
            Kind::Rename { renamed } => self.add_weakref(obj, 0x001e, &TYPEDEFS_PATH, *renamed)?,
            Kind::Record { members } => {
                self.put_data(
                    obj,
                    0x001d,
                    utf16_array(members.iter().map(|(n, _)| n.as_str())),
                );
                // pyaaf2's add_typedef_weakref_vector_property: note the
                // 32-character name, where other indexes get 22.
                let index = self.weakref_index(&TYPEDEFS_PATH)?;
                let index_name = super::object::mangle("MemberTypes", 0x001c, 32);
                self.put_prop(
                    obj,
                    super::object::Prop {
                        pid: 0x001c,
                        format: SF_WEAK_VECTOR,
                        data: encode_string(&index_name),
                        body: super::object::Body::WeakArray {
                            index_name,
                            weakref_index: index,
                            key_pid: PID_AUID,
                            key_size: 16,
                            keys: members
                                .iter()
                                .map(|(_, t)| t.to_bytes_le().to_vec())
                                .collect(),
                        },
                    },
                );
            }
            Kind::ExtEnum { names, values } => {
                self.put_data(obj, 0x001f, utf16_array(names.iter().map(String::as_str)));
                self.put_data(obj, 0x0020, auid_array(values));
            }
            Kind::GenericCharacter => {
                let size = raw::GENERIC_CHARACTER_SIZES
                    .iter()
                    .find(|(a, _)| *a == type_auid)
                    .map_or(1, |(_, size)| *size);
                let class = self
                    .model
                    .class_index(GENERIC_CHARACTER_CLASS)
                    .ok_or_else(|| Error::UndefinedClass {
                        name: GENERIC_CHARACTER_CLASS.to_string(),
                    })?;
                let pid = self
                    .model
                    .all_propertydefs(class)
                    .into_iter()
                    .filter(|&p| self.model.props[p].auid == GENERIC_CHARACTER_SIZE)
                    .map(|p| self.model.props[p].pid)
                    .next_back()
                    .ok_or(Error::Unsupported {
                        what: "a generic character type without a size property",
                    })?;
                self.put_data(obj, pid, vec![size]);
            }
            Kind::Stream | Kind::Opaque | Kind::Character | Kind::Indirect => {}
        }

        self.add2set(
            self.metadict,
            PID_TYPEDEFS,
            type_auid.to_bytes_le().to_vec(),
            obj,
        )?;
        self.model.types.push(TypeInfo {
            name: name.to_owned(),
            auid: type_auid,
            kind,
            obj,
        });
        let index = self.model.types.len() - 1;
        self.model.type_by_name.insert(name.to_owned(), index);
        self.model.type_by_auid.insert(type_auid, index);
        Ok(index)
    }

    /// pyaaf2's `TypeDefEnum.register_element`: adds an element unless its
    /// name or value is taken, rewriting the element list in value order.
    fn register_enum_element(&mut self, type_index: usize, name: &str, value: i64) {
        let Kind::Enum { names, values, .. } = &self.model.types[type_index].kind else {
            return;
        };
        let mut elements = dict_elements(values, names);
        elements.sort();
        if elements.iter().any(|(v, n)| *v == value || n == name) {
            return;
        }
        elements.push((value, name.to_owned()));
        let new_names: Vec<String> = elements.iter().map(|(_, n)| n.clone()).collect();
        let new_values: Vec<i64> = elements.iter().map(|(v, _)| *v).collect();
        let obj = self.model.types[type_index].obj;
        self.put_data(
            obj,
            0x0015,
            utf16_array(new_names.iter().map(String::as_str)),
        );
        self.put_data(obj, 0x0016, s64_array(&new_values));
        if let Kind::Enum { names, values, .. } = &mut self.model.types[type_index].kind {
            *names = new_names;
            *values = new_values;
        }
    }

    /// pyaaf2's `TypeDefExtEnum.register_element`: adds an element unless its
    /// name or identifier is taken, keeping the existing order.
    fn register_ext_enum_element(&mut self, type_index: usize, name: &str, value: Auid) {
        let Kind::ExtEnum { names, values } = &self.model.types[type_index].kind else {
            return;
        };
        let mut elements = dict_elements(values, names);
        if elements.iter().any(|(v, n)| *v == value || n == name) {
            return;
        }
        elements.push((value, name.to_owned()));
        let new_names: Vec<String> = elements.iter().map(|(_, n)| n.clone()).collect();
        let new_values: Vec<Auid> = elements.iter().map(|(v, _)| *v).collect();
        let obj = self.model.types[type_index].obj;
        self.put_data(
            obj,
            0x001f,
            utf16_array(new_names.iter().map(String::as_str)),
        );
        self.put_data(obj, 0x0020, auid_array(&new_values));
        if let Kind::ExtEnum { names, values } = &mut self.model.types[type_index].kind {
            *names = new_names;
            *values = new_values;
        }
    }

    /// Registers an enumeration, or adds its elements to the one of that
    /// name already there.
    fn register_enum(&mut self, t: &EnumType) -> Result<()> {
        if let Some(&index) = self.model.type_by_name.get(t.name) {
            for (value, name) in t.elements {
                self.register_enum_element(index, name, *value);
            }
            return Ok(());
        }
        self.register_typedef(
            t.name,
            t.auid,
            Kind::Enum {
                element: t.element_type,
                names: t.elements.iter().map(|(_, n)| (*n).to_owned()).collect(),
                values: t.elements.iter().map(|(v, _)| *v).collect(),
            },
        )?;
        Ok(())
    }

    fn register_ext_enum(&mut self, t: &ExtEnumType) -> Result<()> {
        if let Some(&index) = self.model.type_by_name.get(t.name) {
            for (value, name) in t.elements {
                self.register_ext_enum_element(index, name, *value);
            }
            return Ok(());
        }
        self.register_typedef(
            t.name,
            t.auid,
            Kind::ExtEnum {
                names: t.elements.iter().map(|(_, n)| (*n).to_owned()).collect(),
                values: t.elements.iter().map(|(v, _)| *v).collect(),
            },
        )?;
        Ok(())
    }

    fn register_record(&mut self, t: &RecordType) -> Result<()> {
        let members = t
            .members
            .iter()
            .map(|(n, a)| ((*n).to_owned(), *a))
            .collect();
        self.register_typedef(t.name, t.auid, Kind::Record { members })?;
        Ok(())
    }

    /// Builds the meta dictionary pyaaf2's `MetaDictionary.__init__` builds:
    /// the standard classes and types, none of them in the file yet.
    pub(crate) fn build_base_model(&mut self) -> Result<()> {
        self.add_set_property(
            self.metadict,
            PID_CLASSDEFS,
            "ClassDefinitions",
            PID_AUID,
            16,
        );
        self.add_set_property(self.metadict, PID_TYPEDEFS, "TypeDefinitions", PID_AUID, 16);

        for class in raw::CLASSES {
            self.register_classdef(class)?;
        }
        for (alias, name) in raw::CLASS_ALIASES {
            if let Some(index) = self.model.class_named(name) {
                self.model.class_by_name.insert((*alias).to_owned(), index);
            }
        }

        for t in raw::ROOT_STRONG_REFS {
            self.register_typedef(t.name, t.auid, Kind::StrongRef { class: t.other })?;
        }

        // pyaaf2 walks the categories in this order.
        for t in raw::INTS {
            self.register_typedef(
                t.name,
                t.auid,
                Kind::Int {
                    size: t.size,
                    signed: t.signed,
                },
            )?;
        }
        for t in raw::ENUMS {
            self.register_enum(t)?;
        }
        for t in raw::RECORDS {
            self.register_record(t)?;
        }
        for t in raw::FIXED_ARRAYS {
            self.register_typedef(
                t.name,
                t.auid,
                Kind::FixedArray {
                    element: t.element_type,
                    count: t.count,
                },
            )?;
        }
        for t in raw::VAR_ARRAYS {
            self.register_typedef(t.name, t.auid, Kind::VarArray { element: t.other })?;
        }
        for t in raw::RENAMES {
            self.register_typedef(t.name, t.auid, Kind::Rename { renamed: t.other })?;
        }
        for t in raw::STRINGS {
            self.register_typedef(t.name, t.auid, Kind::String { element: t.other })?;
        }
        for t in raw::STREAMS {
            self.register_typedef(t.name, t.auid, Kind::Stream)?;
        }
        for t in raw::OPAQUES {
            self.register_typedef(t.name, t.auid, Kind::Opaque)?;
        }
        for t in raw::EXT_ENUMS {
            self.register_ext_enum(t)?;
        }
        for t in raw::CHARACTERS {
            self.register_typedef(t.name, t.auid, Kind::Character)?;
        }
        for t in raw::GENERIC_CHARACTERS {
            self.register_typedef(t.name, t.auid, Kind::GenericCharacter)?;
        }
        for t in raw::INDIRECTS {
            self.register_typedef(t.name, t.auid, Kind::Indirect)?;
        }
        for t in raw::SETS {
            self.register_typedef(t.name, t.auid, Kind::Set { element: t.other })?;
        }
        for t in raw::STRONG_REFS {
            self.register_typedef(t.name, t.auid, Kind::StrongRef { class: t.other })?;
        }
        for t in raw::WEAK_REFS {
            self.register_typedef(
                t.name,
                t.auid,
                Kind::WeakRef {
                    class: t.target,
                    target_set: t.target_set.to_vec(),
                },
            )?;
        }
        Ok(())
    }

    /// pyaaf2's `MetaDictionary.register_extensions`: Avid's classes and
    /// types, registered on a dictionary that is already in the file, so
    /// that each new definition is attached as it is made.
    pub(crate) fn register_extensions(&mut self) -> Result<()> {
        for class in raw::EXT_CLASSES {
            self.register_classdef(class)?;
        }
        for (alias, name) in raw::EXT_CLASS_ALIASES {
            if let Some(index) = self.model.class_named(name) {
                self.model.class_by_name.insert((*alias).to_owned(), index);
            }
        }
        for t in raw::EXT_TYPES {
            match t {
                ExtType::Int(t) => {
                    self.register_typedef(
                        t.name,
                        t.auid,
                        Kind::Int {
                            size: t.size,
                            signed: t.signed,
                        },
                    )?;
                }
                ExtType::Enum(t) => self.register_enum(t)?,
                ExtType::Record(t) => self.register_record(t)?,
                ExtType::FixedArray(t) => {
                    self.register_typedef(
                        t.name,
                        t.auid,
                        Kind::FixedArray {
                            element: t.element_type,
                            count: t.count,
                        },
                    )?;
                }
                ExtType::VarArray(t) => {
                    self.register_typedef(t.name, t.auid, Kind::VarArray { element: t.other })?;
                }
                ExtType::Rename(t) => {
                    self.register_typedef(t.name, t.auid, Kind::Rename { renamed: t.other })?;
                }
                ExtType::String(t) => {
                    self.register_typedef(t.name, t.auid, Kind::String { element: t.other })?;
                }
                ExtType::Stream(t) => {
                    self.register_typedef(t.name, t.auid, Kind::Stream)?;
                }
                ExtType::Opaque(t) => {
                    self.register_typedef(t.name, t.auid, Kind::Opaque)?;
                }
                ExtType::ExtEnum(t) => self.register_ext_enum(t)?,
                ExtType::Character(t) => {
                    self.register_typedef(t.name, t.auid, Kind::Character)?;
                }
                ExtType::Indirect(t) => {
                    self.register_typedef(t.name, t.auid, Kind::Indirect)?;
                }
                ExtType::Set(t) => {
                    self.register_typedef(t.name, t.auid, Kind::Set { element: t.other })?;
                }
                ExtType::StrongRef(t) => {
                    self.register_typedef(t.name, t.auid, Kind::StrongRef { class: t.other })?;
                }
                ExtType::WeakRef(t) => {
                    self.register_typedef(
                        t.name,
                        t.auid,
                        Kind::WeakRef {
                            class: t.target,
                            target_set: t.target_set.to_vec(),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }
}

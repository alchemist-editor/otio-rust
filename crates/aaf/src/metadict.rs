//! The meta dictionary: the definitions that say what a file's objects mean.
//!
//! An AAF file describes itself. Alongside its content it carries a meta
//! dictionary listing every class it uses, every property those classes have,
//! and every type those properties are. Without it a property is a number and
//! some bytes; with it, property `0x4403` on a mob is `Slots`, a set of
//! `MobSlot` objects.
//!
//! That is what this module reads. [`MetaDictionary::read`] walks the
//! dictionary out of an open file and gives you [`ClassDef`],
//! [`PropertyDef`] and [`TypeDef`] tables to look names and types up in.
//!
//! # What a file leaves out
//!
//! A file's dictionary describes the classes the file *stores*, which is not
//! quite every class it uses. The clearest case is the root object: `Root` is
//! part of the format rather than of any file, so nothing stores it, and
//! nothing in the file says that its property 1 is the meta dictionary and
//! property 2 the header. Upstream `pyaaf2` fills those in from built-in
//! tables it carries; those tables are not ported yet, so a lookup of a class
//! the file does not store returns `None` rather than a wrong answer.
//!
//! # Inheritance
//!
//! Classes form a single-inheritance tree, and a class holds only the
//! properties it adds. The ones it inherits live on its ancestors, so
//! [`MetaDictionary::all_properties`] and [`MetaDictionary::property`] walk up
//! the parent chain. The root of the tree, `InterchangeObject`, is its own
//! parent, which is how the walk knows to stop.
//!
//! # Example
//!
//! ```no_run
//! use std::fs::File;
//! use aaf::{AafFile, MetaDictionary};
//!
//! let mut file = AafFile::open(File::open("example.aaf")?)?;
//! let metadict = MetaDictionary::read(&mut file)?;
//!
//! let root = file.root()?;
//! for property in root.properties() {
//!     let name = metadict
//!         .property(root.class_id(), property.pid)
//!         .map_or("<unknown>", |def| def.name.as_str());
//!     println!("{name}");
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;
use std::io::{Read, Seek};

use crate::Auid;
use crate::error::{Error, Result};
use crate::object::{AafFile, Object};
use crate::property::{PropertyValue, RefKey};
use crate::utf16::decode_le;

/// Property identifiers shared by the definition classes.
mod pid {
    /// The definition's own identifier.
    pub const AUID: u16 = 0x0005;
    /// The definition's name.
    pub const NAME: u16 = 0x0006;

    /// A class's parent class.
    pub const PARENT: u16 = 0x0008;
    /// A class's own properties.
    pub const PROPERTIES: u16 = 0x0009;
    /// Whether a class can be instantiated.
    pub const CONCRETE: u16 = 0x000a;

    /// A property's type.
    pub const TYPE: u16 = 0x000b;
    /// Whether a property may be absent.
    pub const OPTIONAL: u16 = 0x000c;
    /// A property's identifier within its class.
    pub const PID: u16 = 0x000d;
    /// Whether a property is its class's unique key.
    pub const UNIQUE: u16 = 0x000e;

    /// An integer type's width in bytes.
    pub const INT_SIZE: u16 = 0x000f;
    /// Whether an integer type is signed.
    pub const INT_SIGNED: u16 = 0x0010;
    /// What a strong reference type points at.
    pub const STRONGREF_TARGET: u16 = 0x0011;
    /// What a weak reference type points at.
    pub const WEAKREF_TARGET: u16 = 0x0012;
    /// Where a weak reference type's targets are owned.
    pub const WEAKREF_TARGET_SET: u16 = 0x0013;
    /// An enumeration's underlying integer type.
    pub const ENUM_TYPE: u16 = 0x0014;
    /// An enumeration's element names.
    pub const ENUM_NAMES: u16 = 0x0015;
    /// An enumeration's element values.
    pub const ENUM_VALUES: u16 = 0x0016;
    /// A fixed array's element type.
    pub const FIXED_TYPE: u16 = 0x0017;
    /// A fixed array's length.
    pub const FIXED_COUNT: u16 = 0x0018;
    /// A variable array's element type.
    pub const VAR_TYPE: u16 = 0x0019;
    /// A set's element type.
    pub const SET_TYPE: u16 = 0x001a;
    /// A string's character type.
    pub const STRING_TYPE: u16 = 0x001b;
    /// A record's member types.
    pub const RECORD_TYPES: u16 = 0x001c;
    /// A record's member names.
    pub const RECORD_NAMES: u16 = 0x001d;
    /// What a renamed type is an alias for.
    pub const RENAME_TYPE: u16 = 0x001e;
    /// An extendible enumeration's element names.
    pub const EXTENUM_NAMES: u16 = 0x001f;
    /// An extendible enumeration's element values.
    pub const EXTENUM_VALUES: u16 = 0x0020;

    /// The meta dictionary's class definitions.
    pub const CLASSDEFS: u16 = 0x0003;
    /// The meta dictionary's type definitions.
    pub const TYPEDEFS: u16 = 0x0004;

    /// The root object's meta dictionary.
    pub const ROOT_METADICT: u16 = 0x0001;
}

/// The class identifiers of the definition classes themselves.
mod class {
    use crate::Auid;

    /// Builds one of the `0d010101-02xx-0000-060e-2b3402060101` identifiers.
    const fn meta(kind: u16) -> Auid {
        let [hi, lo] = kind.to_be_bytes();
        Auid::from_bytes_be([
            0x0d, 0x01, 0x01, 0x01, hi, lo, 0x00, 0x00, 0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01,
            0x01,
        ])
    }

    /// An integer type.
    pub const TYPE_INT: Auid = meta(0x0204);
    /// A strong reference type.
    pub const TYPE_STRONGREF: Auid = meta(0x0205);
    /// A weak reference type.
    pub const TYPE_WEAKREF: Auid = meta(0x0206);
    /// An enumeration type.
    pub const TYPE_ENUM: Auid = meta(0x0207);
    /// A fixed-length array type.
    pub const TYPE_FIXED_ARRAY: Auid = meta(0x0208);
    /// A variable-length array type.
    pub const TYPE_VAR_ARRAY: Auid = meta(0x0209);
    /// A set type.
    pub const TYPE_SET: Auid = meta(0x020a);
    /// A string type.
    pub const TYPE_STRING: Auid = meta(0x020b);
    /// A stream type.
    pub const TYPE_STREAM: Auid = meta(0x020c);
    /// A record type.
    pub const TYPE_RECORD: Auid = meta(0x020d);
    /// A renamed type.
    pub const TYPE_RENAME: Auid = meta(0x020e);
    /// An extendible enumeration type.
    pub const TYPE_EXT_ENUM: Auid = meta(0x0220);
    /// An indirect type.
    pub const TYPE_INDIRECT: Auid = meta(0x0221);
    /// An opaque type.
    pub const TYPE_OPAQUE: Auid = meta(0x0222);
    /// A character type.
    pub const TYPE_CHARACTER: Auid = meta(0x0223);
}

/// One property a class declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDef {
    /// The property's own identifier, unique across all of AAF.
    pub auid: Auid,
    /// The property's name, as applications show it.
    pub name: String,
    /// The property's identifier within its class, as stored on disk.
    pub pid: u16,
    /// The type of the property's value.
    pub type_id: Auid,
    /// Whether an object of this class may leave the property out.
    pub optional: bool,
    /// Whether this property is its class's unique key.
    ///
    /// Sets of objects are keyed by whichever property this is true of.
    pub unique: bool,
}

/// One class the file uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDef {
    /// The class's identifier, which is what a storage entry carries.
    pub auid: Auid,
    /// The class's name, as applications show it.
    pub name: String,
    /// The class this one inherits from. The root class is its own parent.
    pub parent: Auid,
    /// Whether objects of this class can exist, as opposed to only its
    /// subclasses'.
    pub concrete: bool,
    /// The properties this class adds, in pid order. Inherited ones are on
    /// its ancestors.
    pub properties: Vec<PropertyDef>,
}

/// One type the file uses.
///
/// The variants are AAF's own type categories. Every one carries its identity
/// and name; the rest of each variant is what that category needs to describe
/// a value, with other types referred to by identifier rather than inlined,
/// because types are freely recursive.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TypeKind {
    /// An integer of a given width and signedness.
    Int {
        /// The width in bytes: 1, 2, 4 or 8.
        size: u8,
        /// Whether the integer is signed.
        signed: bool,
    },
    /// A reference to an object this one owns.
    StrongRef {
        /// The class of object referred to.
        target: Auid,
    },
    /// A reference to an object owned elsewhere in the file.
    WeakRef {
        /// The class of object referred to.
        target: Auid,
        /// The path of properties, from the root, to where those objects are
        /// owned.
        target_set: Vec<Auid>,
    },
    /// A named set of integer values.
    Enum {
        /// The underlying integer type.
        element_type: Auid,
        /// The elements, as value and name.
        elements: Vec<(i64, String)>,
    },
    /// A fixed number of elements of one type.
    FixedArray {
        /// The element type.
        element_type: Auid,
        /// How many elements there always are.
        count: u32,
    },
    /// Any number of elements of one type, in order.
    VarArray {
        /// The element type.
        element_type: Auid,
    },
    /// Any number of elements of one type, unordered.
    Set {
        /// The element type.
        element_type: Auid,
    },
    /// A string of characters.
    String {
        /// The character type.
        element_type: Auid,
    },
    /// A stream of bytes, stored outside the property.
    Stream,
    /// Named members of their own types, stored one after another.
    Record {
        /// The members, as name and type, in storage order.
        members: Vec<(String, Auid)>,
    },
    /// Another name for an existing type.
    Rename {
        /// The type this is an alias for.
        renamed: Auid,
    },
    /// A set of named values that a file may add to.
    ExtEnum {
        /// The elements, as value and name.
        elements: Vec<(Auid, String)>,
    },
    /// A value that carries its own type.
    Indirect,
    /// A value whose type the reader is not expected to know.
    Opaque,
    /// A single character.
    Character,
    /// A category this reader does not know.
    Unknown {
        /// The class of the definition object, which is what says the category.
        class_id: Auid,
    },
}

/// One type definition: its identity, its name and its category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    /// The type's identifier, which is what a property definition refers to.
    pub auid: Auid,
    /// The type's name, as applications show it.
    pub name: String,
    /// What kind of type it is, and the detail that kind needs.
    pub kind: TypeKind,
}

/// A file's class, property and type definitions.
#[derive(Debug, Clone, Default)]
pub struct MetaDictionary {
    classes: HashMap<Auid, ClassDef>,
    types: HashMap<Auid, TypeDef>,
    classes_by_name: HashMap<String, Auid>,
    types_by_name: HashMap<String, Auid>,
}

impl MetaDictionary {
    /// Reads a file's meta dictionary.
    ///
    /// # Errors
    ///
    /// Returns an error if the file has no meta dictionary, or if a definition
    /// in it is missing a property it cannot be read without.
    pub fn read<R: Read + Seek>(file: &mut AafFile<R>) -> Result<Self> {
        let root = file.root()?;
        let property = root
            .get(pid::ROOT_METADICT)
            .ok_or(Error::MissingDefinitionProperty {
                definition: "the root object".to_owned(),
                pid: pid::ROOT_METADICT,
            })?
            .clone();
        let metadict = file.strong_ref(&root, &property)?;

        let mut out = Self::default();

        for (_, object) in Self::definitions(file, &metadict, pid::CLASSDEFS)? {
            let class = Self::read_class(file, &object)?;
            out.classes_by_name.insert(class.name.clone(), class.auid);
            out.classes.insert(class.auid, class);
        }

        for (_, object) in Self::definitions(file, &metadict, pid::TYPEDEFS)? {
            let type_def = Self::read_type(file, &object)?;
            out.types_by_name
                .insert(type_def.name.clone(), type_def.auid);
            out.types.insert(type_def.auid, type_def);
        }

        Ok(out)
    }

    /// How many classes the dictionary defines.
    #[must_use]
    pub fn class_count(&self) -> usize {
        self.classes.len()
    }

    /// How many types the dictionary defines.
    #[must_use]
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Every class, in no particular order.
    pub fn classes(&self) -> impl Iterator<Item = &ClassDef> {
        self.classes.values()
    }

    /// Every type, in no particular order.
    pub fn types(&self) -> impl Iterator<Item = &TypeDef> {
        self.types.values()
    }

    /// The class with this identifier.
    #[must_use]
    pub fn class(&self, auid: Auid) -> Option<&ClassDef> {
        self.classes.get(&auid)
    }

    /// The class with this name.
    #[must_use]
    pub fn class_named(&self, name: &str) -> Option<&ClassDef> {
        self.classes.get(self.classes_by_name.get(name)?)
    }

    /// The type with this identifier.
    #[must_use]
    pub fn type_def(&self, auid: Auid) -> Option<&TypeDef> {
        self.types.get(&auid)
    }

    /// The type with this name.
    #[must_use]
    pub fn type_named(&self, name: &str) -> Option<&TypeDef> {
        self.types.get(self.types_by_name.get(name)?)
    }

    /// The class this one inherits from, or `None` for the root class.
    ///
    /// The root class is recorded as its own parent, which this reports as no
    /// parent rather than as a loop.
    #[must_use]
    pub fn parent(&self, class: &ClassDef) -> Option<&ClassDef> {
        if class.parent == class.auid {
            return None;
        }
        self.class(class.parent)
    }

    /// Every property an object of this class can have, its own and inherited.
    ///
    /// Ordered from the root of the inheritance chain down, so a class's own
    /// properties come last. An unknown class has none.
    #[must_use]
    pub fn all_properties(&self, class_id: Auid) -> Vec<&PropertyDef> {
        let mut chain = Vec::new();
        let mut current = self.class(class_id);
        while let Some(class) = current {
            chain.push(class);
            current = self.parent(class);
            // A class that is its own ancestor would loop forever.
            if chain.len() > self.classes.len() {
                break;
            }
        }
        chain
            .iter()
            .rev()
            .flat_map(|class| class.properties.iter())
            .collect()
    }

    /// The definition of one property of a class, its own or inherited.
    #[must_use]
    pub fn property(&self, class_id: Auid, pid: u16) -> Option<&PropertyDef> {
        let mut current = self.class(class_id);
        let mut depth = 0;
        while let Some(class) = current {
            if let Some(found) = class.properties.iter().find(|p| p.pid == pid) {
                return Some(found);
            }
            current = self.parent(class);
            depth += 1;
            if depth > self.classes.len() {
                break;
            }
        }
        None
    }

    /// The pid of the property that is a class's unique key, if it has one.
    ///
    /// Sets of objects are keyed by this property's value.
    #[must_use]
    pub fn unique_key_pid(&self, class_id: Auid) -> Option<u16> {
        self.all_properties(class_id)
            .into_iter()
            .find(|p| p.unique)
            .map(|p| p.pid)
    }

    // --- reading ------------------------------------------------------------

    /// Reads the members of one of the meta dictionary's two sets.
    fn definitions<R: Read + Seek>(
        file: &mut AafFile<R>,
        metadict: &Object,
        pid: u16,
    ) -> Result<Vec<(RefKey, Object)>> {
        let property = metadict
            .get(pid)
            .ok_or(Error::MissingDefinitionProperty {
                definition: "the meta dictionary".to_owned(),
                pid,
            })?
            .clone();
        file.strong_ref_set(metadict, &property)
    }

    fn read_class<R: Read + Seek>(file: &mut AafFile<R>, object: &Object) -> Result<ClassDef> {
        let name = string(object, pid::NAME)?;
        let auid = auid(object, pid::AUID, &name)?;

        let mut properties = Vec::new();
        if let Some(property) = object.get(pid::PROPERTIES).cloned() {
            for (_, member) in file.strong_ref_set(object, &property)? {
                properties.push(Self::read_property(&member)?);
            }
            properties.sort_by_key(|p| p.pid);
        }

        Ok(ClassDef {
            auid,
            parent: weak_ref_auid(object, pid::PARENT).unwrap_or(auid),
            concrete: boolean(object, pid::CONCRETE),
            name,
            properties,
        })
    }

    fn read_property(object: &Object) -> Result<PropertyDef> {
        let name = string(object, pid::NAME)?;
        Ok(PropertyDef {
            auid: auid(object, pid::AUID, &name)?,
            pid: u16_value(object, pid::PID).ok_or_else(|| Error::MissingDefinitionProperty {
                definition: name.clone(),
                pid: pid::PID,
            })?,
            type_id: auid(object, pid::TYPE, &name)?,
            optional: boolean(object, pid::OPTIONAL),
            unique: boolean(object, pid::UNIQUE),
            name,
        })
    }

    fn read_type<R: Read + Seek>(file: &mut AafFile<R>, object: &Object) -> Result<TypeDef> {
        let name = string(object, pid::NAME)?;
        let auid = auid(object, pid::AUID, &name)?;

        // The category is the class of the definition object itself.
        let kind = match object.class_id() {
            class::TYPE_INT => TypeKind::Int {
                size: byte(object, pid::INT_SIZE).unwrap_or(0),
                signed: boolean(object, pid::INT_SIGNED),
            },
            class::TYPE_STRONGREF => TypeKind::StrongRef {
                target: weak_ref_auid(object, pid::STRONGREF_TARGET).unwrap_or(Auid::NIL),
            },
            class::TYPE_WEAKREF => TypeKind::WeakRef {
                target: weak_ref_auid(object, pid::WEAKREF_TARGET).unwrap_or(Auid::NIL),
                target_set: auid_array(object, pid::WEAKREF_TARGET_SET),
            },
            class::TYPE_ENUM => TypeKind::Enum {
                element_type: weak_ref_auid(object, pid::ENUM_TYPE).unwrap_or(Auid::NIL),
                elements: enum_elements(object),
            },
            class::TYPE_FIXED_ARRAY => TypeKind::FixedArray {
                element_type: weak_ref_auid(object, pid::FIXED_TYPE).unwrap_or(Auid::NIL),
                count: u32_value(object, pid::FIXED_COUNT).unwrap_or(0),
            },
            class::TYPE_VAR_ARRAY => TypeKind::VarArray {
                element_type: weak_ref_auid(object, pid::VAR_TYPE).unwrap_or(Auid::NIL),
            },
            class::TYPE_SET => TypeKind::Set {
                element_type: weak_ref_auid(object, pid::SET_TYPE).unwrap_or(Auid::NIL),
            },
            class::TYPE_STRING => TypeKind::String {
                element_type: weak_ref_auid(object, pid::STRING_TYPE).unwrap_or(Auid::NIL),
            },
            class::TYPE_STREAM => TypeKind::Stream,
            class::TYPE_RECORD => TypeKind::Record {
                members: Self::read_record_members(file, object)?,
            },
            class::TYPE_RENAME => TypeKind::Rename {
                renamed: weak_ref_auid(object, pid::RENAME_TYPE).unwrap_or(Auid::NIL),
            },
            class::TYPE_EXT_ENUM => TypeKind::ExtEnum {
                elements: ext_enum_elements(object),
            },
            class::TYPE_INDIRECT => TypeKind::Indirect,
            class::TYPE_OPAQUE => TypeKind::Opaque,
            class::TYPE_CHARACTER => TypeKind::Character,
            class_id => TypeKind::Unknown { class_id },
        };

        Ok(TypeDef { auid, name, kind })
    }

    /// Reads a record type's members: names inline, types in an index stream.
    fn read_record_members<R: Read + Seek>(
        file: &mut AafFile<R>,
        object: &Object,
    ) -> Result<Vec<(String, Auid)>> {
        let names = utf16_array(object, pid::RECORD_NAMES);

        let types: Vec<Auid> = match object.get(pid::RECORD_TYPES).cloned() {
            Some(property) => match &property.value {
                PropertyValue::WeakRefArray { .. } => file
                    .weak_ref_array(object, &property)?
                    .keys
                    .into_iter()
                    .filter_map(|key| match key {
                        RefKey::Auid(id) => Some(id),
                        RefKey::MobId(_) => None,
                    })
                    .collect(),
                PropertyValue::Data(data) => parse_auid_array(data),
                _ => Vec::new(),
            },
            None => Vec::new(),
        };

        Ok(names.into_iter().zip(types).collect())
    }
}

// --- property accessors -----------------------------------------------------

/// Reads a property's inline bytes, if it has any.
fn data(object: &Object, pid: u16) -> Option<&[u8]> {
    match &object.get(pid)?.value {
        PropertyValue::Data(bytes) => Some(bytes),
        _ => None,
    }
}

/// Reads a UTF-16 string property.
fn string(object: &Object, pid: u16) -> Result<String> {
    data(object, pid)
        .map(decode_le)
        .ok_or(Error::MissingDefinitionProperty {
            definition: format!("a definition of class {}", object.class_id()),
            pid,
        })
}

/// Reads an AUID stored inline.
fn auid(object: &Object, pid: u16, definition: &str) -> Result<Auid> {
    let bytes = data(object, pid).and_then(|d| <[u8; 16]>::try_from(d).ok());
    bytes
        .map(Auid::from_bytes_le)
        .ok_or_else(|| Error::MissingDefinitionProperty {
            definition: definition.to_owned(),
            pid,
        })
}

/// Reads the target of a weak reference property.
///
/// Definitions refer to each other by weak reference, and the reference's key
/// is the target's own AUID, so the key is the answer without a lookup.
fn weak_ref_auid(object: &Object, pid: u16) -> Option<Auid> {
    match &object.get(pid)?.value {
        PropertyValue::WeakRef {
            key: RefKey::Auid(id),
            ..
        } => Some(*id),
        // Some writers store the target inline rather than as a reference.
        PropertyValue::Data(bytes) => <[u8; 16]>::try_from(bytes.as_slice())
            .ok()
            .map(Auid::from_bytes_le),
        _ => None,
    }
}

/// Reads a boolean property. A missing property reads as false.
fn boolean(object: &Object, pid: u16) -> bool {
    data(object, pid).is_some_and(|d| d.first() == Some(&1))
}

fn byte(object: &Object, pid: u16) -> Option<u8> {
    data(object, pid)?.first().copied()
}

fn u16_value(object: &Object, pid: u16) -> Option<u16> {
    let bytes = data(object, pid)?;
    Some(u16::from_le_bytes(bytes.get(..2)?.try_into().ok()?))
}

fn u32_value(object: &Object, pid: u16) -> Option<u32> {
    let bytes = data(object, pid)?;
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// Reads a run of NUL-terminated UTF-16 strings stored one after another.
fn utf16_array(object: &Object, pid: u16) -> Vec<String> {
    let Some(bytes) = data(object, pid) else {
        return Vec::new();
    };

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();

    // Each string ends at a NUL, including the last, so a trailing run with no
    // NUL is an unterminated string and is dropped, as upstream does.
    units
        .split(|unit| *unit == 0)
        .take(units.iter().filter(|unit| **unit == 0).count())
        .map(|run| {
            char::decode_utf16(run.iter().copied())
                .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect()
        })
        .collect()
}

fn parse_auid_array(bytes: &[u8]) -> Vec<Auid> {
    bytes
        .chunks_exact(16)
        .map(|chunk| Auid::from_bytes_le(chunk.try_into().expect("slice is sixteen bytes")))
        .collect()
}

fn auid_array(object: &Object, pid: u16) -> Vec<Auid> {
    data(object, pid).map(parse_auid_array).unwrap_or_default()
}

/// Reads an enumeration's elements: names inline, values as signed 64-bit.
///
/// The values are always stored 64 bits wide here, whatever the width of the
/// enumeration's underlying integer type.
fn enum_elements(object: &Object) -> Vec<(i64, String)> {
    let names = utf16_array(object, pid::ENUM_NAMES);
    let values = data(object, pid::ENUM_VALUES).unwrap_or_default();
    values
        .chunks_exact(8)
        .map(|chunk| i64::from_le_bytes(chunk.try_into().expect("slice is eight bytes")))
        .zip(names)
        .collect()
}

/// Reads an extendible enumeration's elements: names inline, values as AUIDs.
fn ext_enum_elements(object: &Object) -> Vec<(Auid, String)> {
    let names = utf16_array(object, pid::EXTENUM_NAMES);
    auid_array(object, pid::EXTENUM_VALUES)
        .into_iter()
        .zip(names)
        .collect()
}

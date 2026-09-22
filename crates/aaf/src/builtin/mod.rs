//! The class, property and type definitions AAF takes as given.
//!
//! A file's meta dictionary describes the classes and types that file
//! *stores*, which is not every class and type it uses. AAF treats a large
//! body of definitions as common knowledge: the `Root` class that holds the
//! header and the meta dictionary, every standard class from `Mob` down, and
//! the types their properties are. A file that uses only those need not carry
//! any of them, and files in the wild leave out different subsets of them.
//!
//! [`tables`] holds that body, translated from `pyaaf2`'s own model. This
//! module turns it into a [`MetaDictionary`] that [`MetaDictionary::read`]
//! then lays a file's own definitions over.
//!
//! # Properties with no fixed identifier
//!
//! Sixty-eight of the properties AAF defines have no identifier of their own.
//! They are optional extensions, and a file that uses one assigns it an
//! identifier in its own dictionary, counting down from `0xffff`. Which
//! identifier that is, is the file's business, so those properties are carried
//! here for the write path but left out of the dictionary this builds: reading
//! loses nothing by it, because a file that uses one defines it.

mod tables;
mod write_tables;

use std::sync::OnceLock;

use crate::Auid;
use crate::metadict::{ClassDef, MetaDictionary, PropertyDef, TypeDef, TypeKind};

/// One class, with the properties it adds to its parent's.
pub(crate) struct Class {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    /// The class it inherits from. Only the two roots of the tree have none.
    pub(crate) parent: Option<Auid>,
    pub(crate) concrete: bool,
    pub(crate) properties: &'static [Prop],
}

/// One property a class adds.
pub(crate) struct Prop {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    /// The identifier, for the properties that have one of their own.
    pub(crate) pid: Option<u16>,
    pub(crate) type_id: Auid,
    pub(crate) optional: bool,
    pub(crate) unique: bool,
}

/// An integer type.
pub(crate) struct IntType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) size: u8,
    pub(crate) signed: bool,
}

/// An enumeration, with a name for each value.
pub(crate) struct EnumType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) element_type: Auid,
    pub(crate) elements: &'static [(i64, &'static str)],
}

/// An extendible enumeration, whose values are identifiers.
pub(crate) struct ExtEnumType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) elements: &'static [(Auid, &'static str)],
}

/// A record, with its members in storage order.
pub(crate) struct RecordType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) members: &'static [(&'static str, Auid)],
}

/// An array of a fixed length.
pub(crate) struct FixedArrayType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) element_type: Auid,
    pub(crate) count: u32,
}

/// A type defined entirely by one other type it points at.
///
/// Arrays, sets, renames, strings and strong references all have this shape;
/// what `other` means depends on which table the row is in.
pub(crate) struct PairType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) other: Auid,
}

/// A type that needs nothing but its own identity.
pub(crate) struct SoloType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
}

/// A weak reference, and the path to where its objects are owned.
pub(crate) struct WeakRefType {
    pub(crate) name: &'static str,
    pub(crate) auid: Auid,
    pub(crate) target: Auid,
    pub(crate) target_set: &'static [Auid],
}

/// One type of the extension model, tagged with its category.
///
/// The built-in tables keep one table per category, because the reader only
/// looks types up. The writer needs the order pyaaf2 registered them in as
/// well, and for the extension model that is one sequence across categories.
///
/// Every category pyaaf2 has is here, though its extension model uses only
/// some of them, so that the generator can emit whatever a future model holds.
#[allow(dead_code)]
pub(crate) enum ExtType {
    Int(IntType),
    Enum(EnumType),
    Record(RecordType),
    FixedArray(FixedArrayType),
    VarArray(PairType),
    Rename(PairType),
    String(PairType),
    Stream(SoloType),
    Opaque(SoloType),
    ExtEnum(ExtEnumType),
    Character(SoloType),
    Indirect(SoloType),
    Set(PairType),
    StrongRef(PairType),
    WeakRef(WeakRefType),
}

/// A definition object a new file's dictionary starts with.
pub(crate) struct Definition {
    pub(crate) auid: Auid,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
}

/// A codec definition a new file's dictionary starts with.
pub(crate) struct CodecDefinition {
    pub(crate) auid: Auid,
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    /// The name of the descriptor class the codec's files are described by.
    pub(crate) file_descriptor_class: &'static str,
    /// The names of the data definitions the codec carries.
    pub(crate) data_definitions: &'static [&'static str],
}

/// The raw tables, for the write path, which registers them one by one in
/// pyaaf2's order rather than reading them as a finished dictionary.
pub(crate) mod raw {
    pub(crate) use super::tables::{
        CHARACTERS, CLASS_ALIASES, CLASSES, ENUMS, EXT_ENUMS, FIXED_ARRAYS, GENERIC_CHARACTERS,
        INDIRECTS, INTS, OPAQUES, RECORDS, RENAMES, ROOT_STRONG_REFS, SETS, STREAMS, STRINGS,
        STRONG_REFS, VAR_ARRAYS, WEAK_REFS,
    };
    pub(crate) use super::write_tables::{
        CODEC_DEFS, CONTAINER_DEFS, DATA_DEFS, EXT_CLASS_ALIASES, EXT_CLASSES, EXT_TYPES,
        GENERIC_CHARACTER_SIZES,
    };
}

/// Parses an AUID written the way the tables write it.
///
/// This runs while the program is being compiled, so a malformed identifier in
/// a table is a build failure rather than anything that can happen at runtime.
///
/// # Panics
///
/// If `text` is not sixteen hyphen-separated bytes in the usual UUID grouping.
pub(crate) const fn auid(text: &str) -> Auid {
    let text = text.as_bytes();
    assert!(text.len() == 36, "an AUID is 36 characters");

    let mut bytes = [0u8; 16];
    let mut at = 0; // index into the text
    let mut byte = 0; // index into the output
    while byte < 16 {
        if text[at] == b'-' {
            at += 1;
        }
        bytes[byte] = nibble(text[at]) << 4 | nibble(text[at + 1]);
        at += 2;
        byte += 1;
    }
    Auid::from_bytes_be(bytes)
}

/// One hexadecimal digit.
const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("an AUID holds only hexadecimal digits and hyphens"),
    }
}

/// The definitions every AAF file takes as given.
///
/// Built once and shared, since nothing in it depends on the file being read.
pub(crate) fn dictionary() -> &'static MetaDictionary {
    static BUILT: OnceLock<MetaDictionary> = OnceLock::new();
    BUILT.get_or_init(build)
}

/// Turns the tables into a dictionary.
fn build() -> MetaDictionary {
    let mut dict = MetaDictionary::default();

    for class in tables::CLASSES {
        // The tables keep AAF's own declaration order, which is not quite pid
        // order; a class's properties are held in pid order everywhere else.
        let mut properties: Vec<PropertyDef> = class
            .properties
            .iter()
            .filter_map(|prop| {
                Some(PropertyDef {
                    auid: prop.auid,
                    name: prop.name.to_owned(),
                    pid: prop.pid?,
                    type_id: prop.type_id,
                    optional: prop.optional,
                    unique: prop.unique,
                })
            })
            .collect();
        properties.sort_by_key(|prop| prop.pid);

        dict.define_class(ClassDef {
            auid: class.auid,
            name: class.name.to_owned(),
            // The two roots of the inheritance tree are their own parent,
            // which is the convention the rest of the crate walks on.
            parent: class.parent.unwrap_or(class.auid),
            concrete: class.concrete,
            properties,
        });
    }

    for (alias, name) in tables::CLASS_ALIASES {
        if let Some(class) = dict.class_named(name) {
            let auid = class.auid;
            dict.alias_class(alias, auid);
        }
    }

    let mut define = |name: &str, auid: Auid, kind: TypeKind| {
        dict.define_type(TypeDef {
            auid,
            name: name.to_owned(),
            kind,
        });
    };

    for t in tables::INTS {
        define(
            t.name,
            t.auid,
            TypeKind::Int {
                size: t.size,
                signed: t.signed,
            },
        );
    }
    for t in tables::ENUMS {
        define(
            t.name,
            t.auid,
            TypeKind::Enum {
                element_type: t.element_type,
                elements: t
                    .elements
                    .iter()
                    .map(|(value, name)| (*value, (*name).to_owned()))
                    .collect(),
            },
        );
    }
    for t in tables::EXT_ENUMS {
        define(
            t.name,
            t.auid,
            TypeKind::ExtEnum {
                elements: t
                    .elements
                    .iter()
                    .map(|(value, name)| (*value, (*name).to_owned()))
                    .collect(),
            },
        );
    }
    for t in tables::RECORDS {
        define(
            t.name,
            t.auid,
            TypeKind::Record {
                members: t
                    .members
                    .iter()
                    .map(|(name, type_id)| ((*name).to_owned(), *type_id))
                    .collect(),
            },
        );
    }
    for t in tables::FIXED_ARRAYS {
        define(
            t.name,
            t.auid,
            TypeKind::FixedArray {
                element_type: t.element_type,
                count: t.count,
            },
        );
    }
    for t in tables::VAR_ARRAYS {
        define(
            t.name,
            t.auid,
            TypeKind::VarArray {
                element_type: t.other,
            },
        );
    }
    for t in tables::SETS {
        define(
            t.name,
            t.auid,
            TypeKind::Set {
                element_type: t.other,
            },
        );
    }
    for t in tables::RENAMES {
        define(t.name, t.auid, TypeKind::Rename { renamed: t.other });
    }
    for t in tables::STRINGS {
        define(
            t.name,
            t.auid,
            TypeKind::String {
                element_type: t.other,
            },
        );
    }
    for t in tables::ROOT_STRONG_REFS.iter().chain(tables::STRONG_REFS) {
        define(t.name, t.auid, TypeKind::StrongRef { target: t.other });
    }
    for t in tables::WEAK_REFS {
        define(
            t.name,
            t.auid,
            TypeKind::WeakRef {
                target: t.target,
                target_set: t.target_set.to_vec(),
            },
        );
    }
    for t in tables::STREAMS {
        define(t.name, t.auid, TypeKind::Stream);
    }
    for t in tables::OPAQUES {
        define(t.name, t.auid, TypeKind::Opaque);
    }
    for t in tables::CHARACTERS {
        define(t.name, t.auid, TypeKind::Character);
    }
    for t in tables::INDIRECTS {
        define(t.name, t.auid, TypeKind::Indirect);
    }
    // A generic character's width is told to the reader by the file rather
    // than being part of the type, which is the one category this crate does
    // not model, so it is carried as the unmodelled kind rather than guessed.
    for t in tables::GENERIC_CHARACTERS {
        define(
            t.name,
            t.auid,
            TypeKind::Unknown {
                class_id: GENERIC_CHARACTER_CLASS,
            },
        );
    }

    dict
}

/// The class of a generic character definition.
const GENERIC_CHARACTER_CLASS: Auid = auid("0e040101-0000-0000-060e-2b3402060101");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_auid_at_compile_time() {
        const ID: Auid = auid("0d010101-0101-0100-060e-2b3402060101");
        assert_eq!(ID.to_string(), "0d010101-0101-0100-060e-2b3402060101");
    }

    #[test]
    fn carries_every_definition_in_the_tables() {
        let dict = dictionary();
        assert_eq!(dict.class_count(), tables::CLASSES.len());
        assert_eq!(dict.class_count(), 116);
        assert_eq!(dict.type_count(), 164);
    }

    /// The class no file stores, which is why these tables exist.
    #[test]
    fn knows_the_root_class() {
        let dict = dictionary();
        let root = dict
            .class_named("Root")
            .expect("the root class is built in");
        assert!(root.concrete);
        let metadict = dict.property(root.auid, 1).expect("property 1 is defined");
        assert_eq!(metadict.name, "MetaDictionary");
        let header = dict.property(root.auid, 2).expect("property 2 is defined");
        assert_eq!(header.name, "Header");
    }

    #[test]
    fn resolves_inherited_properties_through_the_chain() {
        let dict = dictionary();
        let timeline = dict
            .class_named("TimelineMobSlot")
            .expect("TimelineMobSlot is built in");
        // EditRate is its own; SlotID comes from MobSlot two levels up.
        assert_eq!(
            dict.property(timeline.auid, 0x4b01)
                .map(|p| p.name.as_str()),
            Some("EditRate")
        );
        assert_eq!(
            dict.property(timeline.auid, 0x4801)
                .map(|p| p.name.as_str()),
            Some("SlotID")
        );
    }

    #[test]
    fn an_alias_finds_the_same_class() {
        let dict = dictionary();
        assert_eq!(
            dict.class_named("ClassDef").map(|c| c.auid),
            dict.class_named("ClassDefinition").map(|c| c.auid)
        );
    }

    /// The type the older fixture uses without defining.
    #[test]
    fn knows_the_types_a_file_may_use_without_defining() {
        let dict = dictionary();
        let array = dict
            .type_named("aafInt64Array")
            .expect("aafInt64Array is built in");
        let TypeKind::VarArray { element_type } = array.kind else {
            panic!("aafInt64Array is a variable array");
        };
        assert_eq!(
            dict.type_def(element_type).map(|t| t.name.as_str()),
            Some("aafInt64")
        );
    }

    /// Optional extension properties have no identifier until a file gives
    /// them one, so they are carried but not registered.
    #[test]
    fn properties_without_an_identifier_are_left_out() {
        let carried: usize = tables::CLASSES
            .iter()
            .map(|class| class.properties.iter().filter(|p| p.pid.is_none()).count())
            .sum();
        assert_eq!(carried, 68);

        let dict = dictionary();
        let descriptor = dict
            .class_named("EssenceDescriptor")
            .expect("EssenceDescriptor is built in");
        assert!(
            !descriptor
                .properties
                .iter()
                .any(|p| p.name == "SubDescriptors"),
            "SubDescriptors has no identifier of its own"
        );
    }
}

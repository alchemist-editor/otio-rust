//! Turning an AAF object's properties into the `AAF` metadata dictionary.
//!
//! Every OTIO object this crate produces carries the AAF object it came from
//! under `metadata["AAF"]`, so nothing in the file is lost on the way across
//! even where OTIO has no field for it. That is upstream's behaviour and the
//! reason a round trip through this adapter can keep a file's bookkeeping.
//!
//! # What is kept, and what is not
//!
//! The rules are upstream's, and two of them are worth stating because they
//! are what a reader of the output will notice:
//!
//! - **A collection of objects comes out empty** unless its members are the
//!   kind that carry a name and a value, which in practice means tagged
//!   values and parameters. A mob's `Slots` is therefore `{}` rather than a
//!   copy of the tracks that are already in the timeline itself.
//! - **A weak reference is followed once.** The property that holds it is
//!   replaced by the object it names, and inside *that* object a further weak
//!   reference is followed only when it names a data definition. Without that
//!   stop, an operation definition would drag its whole dictionary along.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use aaf::property::{Property, PropertyValue};
use aaf::{Aaf, Object, Value};
use otio_core::{Any, AnyDictionary};

use crate::Transcriber;
use crate::error::Result;

/// How deep a chain of references is followed before giving up.
///
/// Weak references stop on their own after one step, but a strong reference
/// can nest as deeply as the file likes. Nothing in these files comes close,
/// so a limit this size only ever catches a file built to loop.
const MAX_DEPTH: u32 = 16;

/// The enumeration AAF uses where another format would have a boolean.
const BOOLEAN_TYPE: &str = "Boolean";

/// The classes AAF spells out and everybody else abbreviates.
///
/// AAF names a definition class `DataDefinition`; pyaaf2 calls it `DataDef`,
/// which is the alias AAF itself records for it and what the metadata of a
/// file read upstream says. The set is closed — every `…Definition` class and
/// every `TypeDefinition…` one — so it is written out rather than guessed at
/// from the shape of the name.
const SHORT_CLASS_NAMES: [(&str, &str); 24] = [
    ("ClassDefinition", "ClassDef"),
    ("CodecDefinition", "CodecDef"),
    ("ContainerDefinition", "ContainerDef"),
    ("DataDefinition", "DataDef"),
    ("InterpolationDefinition", "InterpolationDef"),
    ("OperationDefinition", "OperationDef"),
    ("ParameterDefinition", "ParameterDef"),
    ("PluginDefinition", "PluginDef"),
    ("PropertyDefinition", "PropertyDef"),
    ("TaggedValueDefinition", "TaggedValueDef"),
    ("TypeDefinition", "TypeDef"),
    ("TypeDefinitionCharacter", "TypeDefCharacter"),
    ("TypeDefinitionEnumeration", "TypeDefEnum"),
    ("TypeDefinitionExtendibleEnumeration", "TypeDefExtEnum"),
    ("TypeDefinitionFixedArray", "TypeDefFixedArray"),
    ("TypeDefinitionGenericCharacter", "TypeDefGenericCharacter"),
    ("TypeDefinitionIndirect", "TypeDefIndirect"),
    ("TypeDefinitionInteger", "TypeDefInt"),
    ("TypeDefinitionOpaque", "TypeDefOpaque"),
    ("TypeDefinitionRecord", "TypeDefRecord"),
    ("TypeDefinitionRename", "TypeDefRename"),
    ("TypeDefinitionSet", "TypeDefSet"),
    ("TypeDefinitionStream", "TypeDefStream"),
    ("TypeDefinitionString", "TypeDefString"),
];

/// What the metadata calls an object's class.
pub(crate) fn class_name<R: Read + Seek>(aaf: &Aaf<R>, object: &Object) -> String {
    let name = aaf.class_name(object).unwrap_or_default();
    SHORT_CLASS_NAMES
        .iter()
        .find(|(long, _)| *long == name)
        .map_or_else(|| name.to_owned(), |(_, short)| (*short).to_owned())
}

/// Whether a weak reference at this depth is followed.
///
/// At the top level every weak reference is followed, because that is how a
/// component reaches its data definition and an operation group its
/// operation. Below that only data definitions are, which is what keeps an
/// object from dragging the file's dictionary in behind it.
fn follows_weak_refs<R: Read + Seek>(aaf: &Aaf<R>, target: &Object, depth: u32) -> bool {
    depth == 0 || aaf.is_a(target, "DataDef")
}

/// An AAF object's properties, as the `AAF` metadata dictionary.
///
/// The class name comes first as `ClassName`, and then every property the
/// object carries that its class gives a name to.
///
/// # Errors
///
/// Returns an error if one of the object's properties cannot be read.
pub fn object_properties<R: Read + Seek>(
    state: &mut Transcriber<R>,
    object: &Object,
) -> Result<AnyDictionary> {
    named(state, object, 0)
}

/// An object as its class name and every named property it carries.
fn named<R: Read + Seek>(
    state: &mut Transcriber<R>,
    object: &Object,
    depth: u32,
) -> Result<AnyDictionary> {
    let mut out = BTreeMap::new();
    out.insert(
        "ClassName".to_owned(),
        Any::String(class_name(&state.aaf, object)),
    );
    if depth > MAX_DEPTH {
        return Ok(out);
    }
    // The property table is read into a list first: resolving a reference
    // borrows the file, which the object itself is borrowed from.
    let properties: Vec<(String, Property)> = object
        .properties()
        .iter()
        .filter_map(|property| {
            let def = state
                .aaf
                .metadict()
                .property(object.class_id(), property.pid)?;
            Some((def.name.clone(), property.clone()))
        })
        .collect();

    for (name, property) in properties {
        if let Some(value) = property_value(state, object, &property, depth)? {
            out.insert(name, value);
        }
    }
    Ok(out)
}

/// One property, as the metadata records it.
fn property_value<R: Read + Seek>(
    state: &mut Transcriber<R>,
    owner: &Object,
    property: &Property,
    depth: u32,
) -> Result<Option<Any>> {
    match &property.value {
        PropertyValue::Data(_) => {
            let Some(def) = state
                .aaf
                .metadict()
                .property(owner.class_id(), property.pid)
            else {
                return Ok(None);
            };
            let type_id = def.type_id;
            // AAF has no boolean type: it has an enumeration named `Boolean`
            // whose two elements are `True` and `False`. Everything reading
            // AAF treats it as the boolean it stands for, and recording it as
            // the string "False" would be a value no reader expects.
            let boolean = state
                .aaf
                .metadict()
                .type_def(type_id)
                .is_some_and(|def| def.name == BOOLEAN_TYPE);
            Ok(state
                .aaf
                .metadict()
                .decode(type_id, property)
                .ok()
                .map(|value| {
                    if boolean {
                        as_bool(&value)
                    } else {
                        any_of(value)
                    }
                }))
        }
        PropertyValue::StrongRef { .. } => {
            let child = state.aaf.file().strong_ref(owner, property)?;
            Ok(Some(Any::Dictionary(named(state, &child, depth + 1)?)))
        }
        PropertyValue::StrongRefVector { .. } => {
            let members = state.aaf.file().strong_ref_vector(owner, property)?;
            Ok(Some(Any::Dictionary(collection(state, members, depth)?)))
        }
        PropertyValue::StrongRefSet { .. } => {
            let members = state
                .aaf
                .file()
                .strong_ref_set(owner, property)?
                .into_iter()
                .map(|(_, member)| member)
                .collect();
            Ok(Some(Any::Dictionary(collection(state, members, depth)?)))
        }
        PropertyValue::WeakRef { key, .. } => {
            let Some(target) = state.resolve(*key)? else {
                return Ok(None);
            };
            if !follows_weak_refs(&state.aaf, &target, depth) {
                return Ok(None);
            }
            Ok(Some(Any::Dictionary(named(state, &target, depth + 1)?)))
        }
        // A stream is essence rather than a value, and the collections of
        // weak references in these files name definitions the dictionary
        // already carries.
        _ => Ok(None),
    }
}

/// A collection of objects, keyed by each member's name.
///
/// Only members that carry both a name and a value contribute, which is what
/// makes a mob's `Slots` come out empty while its `UserComments` does not:
/// a tagged value is a name and a value, and a mob slot is neither.
fn collection<R: Read + Seek>(
    state: &mut Transcriber<R>,
    members: Vec<Object>,
    depth: u32,
) -> Result<AnyDictionary> {
    let mut out = BTreeMap::new();
    if depth > MAX_DEPTH {
        return Ok(out);
    }
    for member in members {
        let Ok(Some(name)) = state.aaf.name(&member) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let Ok(pid) = state.aaf.pid(&member, "Value") else {
            continue;
        };
        let Some(property) = member.get(pid).cloned() else {
            continue;
        };
        if let Some(value) = property_value(state, &member, &property, depth + 1)? {
            out.insert(name, value);
        }
    }
    Ok(out)
}

/// A decoded AAF value, as OTIO metadata holds it.
///
/// Numbers and strings come across as themselves and an enumeration keeps its
/// name rather than its number. Everything else becomes text, because that is
/// where upstream ends up: pyaaf2 packages the records AAF wears as single
/// values into Python objects — a rational into a `Fraction`, a timestamp into
/// a `datetime` — and the metadata records how those print. [`render`] writes
/// the same text without the detour.
fn any_of(value: Value) -> Any {
    match value {
        Value::Int(number) => Any::Int(number),
        Value::UInt(number) => i64::try_from(number).map_or(Any::UInt(number), Any::Int),
        Value::String(text) => Any::String(text),
        Value::Enum { name, value } => name.map_or_else(|| Any::Int(value), Any::String),
        Value::ExtEnum { name, value } => {
            name.map_or_else(|| Any::String(value.to_string()), Any::String)
        }
        // A collection is recorded by its members' names, and a collection of
        // plain values has none, so nothing of it is kept. That is upstream's
        // behaviour, and it is why a descriptor's `Summary` — the bytes of a
        // WAVE header — comes out empty rather than as a list of numbers.
        Value::Array(_) | Value::Set(_) => Any::Dictionary(AnyDictionary::new()),
        other => Any::String(render(&other)),
    }
}

/// A value of AAF's boolean enumeration, as a boolean.
fn as_bool(value: &Value) -> Any {
    match value {
        Value::Enum { value, .. } => Any::Bool(*value != 0),
        other => any_of(other.clone()),
    }
}

/// A value as text, the way Python prints what pyaaf2 packaged it into.
///
/// Three records get this treatment, and they are the ones a file is full of:
/// a rational prints as a fraction, and a date, a time or a whole timestamp
/// print the way a `datetime` does. A record this does not recognise falls
/// back to the crate's own rendering, which names its members.
fn render(value: &Value) -> String {
    let Value::Record(members) = value else {
        return value.to_string();
    };
    let member = |name: &str| {
        members
            .iter()
            .find(|(found, _)| found == name)
            .map(|(_, value)| value)
    };
    let number = |name: &str| member(name).and_then(Value::as_i64).unwrap_or_default();

    match (member("Numerator"), member("date"), member("hour")) {
        // A fraction over one prints as a whole number, which is how Python
        // prints the `Fraction` pyaaf2 packages a rational into.
        (Some(_), _, _) => match (number("Numerator"), number("Denominator")) {
            (numerator, 1) => numerator.to_string(),
            (numerator, denominator) => format!("{numerator}/{denominator}"),
        },
        (_, Some(_), _) => {
            let (Some(date), Some(time)) = (member("date"), member("time")) else {
                return value.to_string();
            };
            format!("{} {}", render(date), render(time))
        }
        (_, _, Some(_)) => format!(
            "{:02}:{:02}:{:02}",
            number("hour"),
            number("minute"),
            number("second")
        ),
        _ if member("year").is_some() => format!(
            "{:04}-{:02}-{:02}",
            number("year"),
            number("month"),
            number("day")
        ),
        _ => value.to_string(),
    }
}

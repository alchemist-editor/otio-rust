//! Reading an AAF file by name rather than by number.
//!
//! Everything below this works in identifiers: a property is a `u16`, a class
//! is a 16-byte AUID, and the two are joined by the meta dictionary. That is
//! the file's own vocabulary, but it is nobody's. [`Aaf`] holds a file and its
//! dictionary together so the same work can be written the way people talk
//! about AAF: the `Mobs` of the content storage, the `Slots` of a mob, the
//! `Segment` of a slot.
//!
//! # Example
//!
//! ```no_run
//! use std::fs::File;
//! use aaf::Aaf;
//!
//! let mut aaf = Aaf::open(File::open("example.aaf")?)?;
//!
//! for mob in aaf.mobs()? {
//!     let name = aaf.name(&mob)?.unwrap_or_else(|| "<unnamed>".to_owned());
//!     println!("{name} is a {}", aaf.class_name(&mob).unwrap_or("?"));
//!
//!     for slot in aaf.slots(&mob)? {
//!         let segment = aaf.child(&slot, "Segment")?;
//!         println!("    slot holds a {}", segment
//!             .as_ref()
//!             .and_then(|s| aaf.class_name(s))
//!             .unwrap_or("nothing"));
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Classes, not types
//!
//! AAF's classes form an inheritance tree, and much of reading a file is
//! asking what kind of thing an object is: a `SourceClip` is a
//! `SourceReference` is a `Segment` is a `Component`. [`Aaf::is_a`] answers
//! that the way the format means it, by walking the chain, so a reader can ask
//! for `Segment` and be told yes about all of its kinds.

use std::io::{Read, Seek};

use crate::error::{Error, Result};
use crate::metadict::MetaDictionary;
use crate::object::{AafFile, Object};
use crate::property::{PropertyValue, RefKey};
use crate::value::Value;
use crate::{Auid, MobId};

/// An AAF file and the definitions it is read with.
///
/// Everything below this works in identifiers: a property is a `u16`, a class
/// is a 16-byte AUID, and the meta dictionary joins the two. That is the
/// file's own vocabulary, but it is nobody's. This holds a file and its
/// dictionary together so the same work can be written the way people talk
/// about AAF: the `Mobs` of the content storage, the `Slots` of a mob, the
/// `Segment` of a slot.
///
/// # Example
///
/// ```no_run
/// use std::fs::File;
/// use aaf::Aaf;
///
/// let mut aaf = Aaf::open(File::open("example.aaf")?)?;
///
/// for mob in aaf.top_level_mobs()? {
///     println!("{:?}", aaf.name(&mob)?);
///     for slot in aaf.slots(&mob)? {
///         let segment = aaf.child(&slot, "Segment")?;
///         println!("    {:?} holds a {}",
///             aaf.name(&slot)?,
///             segment.as_ref().and_then(|s| aaf.class_name(s)).unwrap_or("nothing"));
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Classes, not types
///
/// AAF's classes form an inheritance tree, and much of reading a file is
/// asking what kind of thing an object is: a `SourceClip` is a
/// `SourceReference` is a `Segment` is a `Component`. [`is_a`](Self::is_a)
/// answers that the way the format means it, by walking the chain, so a
/// reader can ask for `Segment` and be told yes about all of its kinds.
#[derive(Debug)]
pub struct Aaf<R> {
    file: AafFile<R>,
    metadict: MetaDictionary,
}

impl<R: Read + Seek> Aaf<R> {
    /// Opens a file and reads the definitions that apply to it.
    ///
    /// # Errors
    ///
    /// Returns an error if the source is not an AAF file, or if its meta
    /// dictionary cannot be read.
    pub fn open(reader: R) -> Result<Self> {
        let mut file = AafFile::open(reader)?;
        let metadict = MetaDictionary::read(&mut file)?;
        Ok(Self { file, metadict })
    }

    /// The definitions this file is read with.
    #[must_use]
    pub const fn metadict(&self) -> &MetaDictionary {
        &self.metadict
    }

    /// The file underneath, for reading by identifier.
    pub const fn file(&mut self) -> &mut AafFile<R> {
        &mut self.file
    }

    /// The name of an object's class, if the dictionary defines it.
    #[must_use]
    pub fn class_name(&self, object: &Object) -> Option<&str> {
        self.metadict
            .class(object.class_id())
            .map(|class| class.name.as_str())
    }

    /// Whether an object is of this class, or of one that inherits from it.
    ///
    /// This is what AAF means by an object's kind: a `SourceClip` is a
    /// `Segment`, because `Segment` is somewhere up its inheritance chain.
    #[must_use]
    pub fn is_a(&self, object: &Object, class_name: &str) -> bool {
        let Some(wanted) = self.metadict.class_named(class_name) else {
            return false;
        };
        let mut current = self.metadict.class(object.class_id());
        let mut depth = 0;
        while let Some(class) = current {
            if class.auid == wanted.auid {
                return true;
            }
            // The root of the tree is its own parent, so stop there.
            let parent = self.metadict.parent(class);
            if parent.map(|p| p.auid) == Some(class.auid) {
                return false;
            }
            current = parent;
            depth += 1;
            if depth > self.metadict.class_count() {
                return false;
            }
        }
        false
    }

    /// The identifier a named property has on this object's class.
    ///
    /// # Errors
    ///
    /// Returns an error if the class does not define a property of that name,
    /// which means the name is wrong or the object is not the kind expected.
    pub fn pid(&self, object: &Object, name: &str) -> Result<u16> {
        self.metadict
            .all_properties(object.class_id())
            .into_iter()
            .find(|def| def.name == name)
            .map(|def| def.pid)
            .ok_or_else(|| Error::UndefinedProperty {
                class: self.class_name(object).unwrap_or("?").to_owned(),
                property: name.to_owned(),
            })
    }

    /// A named property's decoded value, if the object carries it.
    ///
    /// Most of AAF's properties are optional, so an object of the right class
    /// may still not have one; that is `None` rather than an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the class does not define a property of that name,
    /// or if the stored bytes do not decode as the type it declares.
    pub fn value(&mut self, object: &Object, name: &str) -> Result<Option<Value>> {
        let pid = self.pid(object, name)?;
        let Some(property) = object.get(pid) else {
            return Ok(None);
        };
        let type_id = self
            .metadict
            .property(object.class_id(), pid)
            .map(|def| def.type_id)
            .ok_or_else(|| Error::UndefinedProperty {
                class: self.class_name(object).unwrap_or("?").to_owned(),
                property: name.to_owned(),
            })?;
        self.metadict.decode(type_id, property).map(Some)
    }

    /// The object a named strong reference points at, if the object has one.
    ///
    /// # Errors
    ///
    /// Returns an error if the class does not define a property of that name,
    /// if the property is not a single strong reference, or if the object it
    /// names cannot be read.
    pub fn child(&mut self, object: &Object, name: &str) -> Result<Option<Object>> {
        let pid = self.pid(object, name)?;
        let Some(property) = object.get(pid).cloned() else {
            return Ok(None);
        };
        self.file.strong_ref(object, &property).map(Some)
    }

    /// The objects a named collection holds, in the order the file keeps them.
    ///
    /// Vectors are ordered and sets are not, but both read the same way here,
    /// because which one a property is, is the format's business rather than
    /// the reader's. A property the object does not carry is an empty list,
    /// since an absent collection and an empty one mean the same thing.
    ///
    /// # Errors
    ///
    /// Returns an error if the class does not define a property of that name,
    /// if the property is not a collection of strong references, or if one of
    /// the objects cannot be read.
    pub fn children(&mut self, object: &Object, name: &str) -> Result<Vec<Object>> {
        let pid = self.pid(object, name)?;
        let Some(property) = object.get(pid).cloned() else {
            return Ok(Vec::new());
        };
        match property.value {
            PropertyValue::StrongRefVector { .. } => self.file.strong_ref_vector(object, &property),
            PropertyValue::StrongRefSet { .. } => Ok(self
                .file
                .strong_ref_set(object, &property)?
                .into_iter()
                .map(|(_, object)| object)
                .collect()),
            PropertyValue::StrongRef { .. } => Ok(vec![self.file.strong_ref(object, &property)?]),
            _ => Err(Error::NotAReference {
                pid,
                expected: "a collection of strong references",
            }),
        }
    }

    /// The key a named weak reference carries.
    ///
    /// A weak reference names an object owned elsewhere in the file rather
    /// than pointing at it, so this gives the key it names — the `DataDef` a
    /// component is sound or picture by, for instance. Resolving that key to
    /// the object is a separate lookup in the file's dictionary.
    ///
    /// # Errors
    ///
    /// Returns an error if the class does not define a property of that name,
    /// or if the property is not a weak reference.
    pub fn weak_key(&mut self, object: &Object, name: &str) -> Result<Option<RefKey>> {
        let pid = self.pid(object, name)?;
        let Some(property) = object.get(pid) else {
            return Ok(None);
        };
        match &property.value {
            PropertyValue::WeakRef { key, .. } => Ok(Some(*key)),
            _ => Err(Error::NotAReference {
                pid,
                expected: "a weak reference",
            }),
        }
    }

    /// What an object is called.
    ///
    /// Most of AAF's classes call this `Name`; a mob slot calls it
    /// `SlotName`. Which one is read is decided by what the object *is*, not
    /// by which name its class happens to define: a vendor extension is free
    /// to add a `Name` to a slot or a `SlotName` to something that is not
    /// one, and either would otherwise be picked over the property the format
    /// actually keeps the slot's name in. Both are optional, so an object may
    /// have neither.
    ///
    /// # Errors
    ///
    /// Returns an error if the object's class does not define the property
    /// its kind keeps its name in, or if the stored value does not decode.
    pub fn name(&mut self, object: &Object) -> Result<Option<String>> {
        let property = if self.is_a(object, "MobSlot") {
            "SlotName"
        } else {
            "Name"
        };
        Ok(self
            .value(object, property)?
            .and_then(|value| value.as_str().map(ToOwned::to_owned)))
    }

    /// A mob's identifier, which is how everything else refers to it.
    ///
    /// # Errors
    ///
    /// Returns an error if the object's class has no `MobID` property, or if
    /// the stored value does not decode.
    pub fn mob_id(&mut self, object: &Object) -> Result<Option<MobId>> {
        Ok(match self.value(object, "MobID")? {
            Some(Value::MobId(id)) => Some(id),
            _ => None,
        })
    }

    /// A named child the file is not readable without.
    fn required(&mut self, object: &Object, name: &str) -> Result<Object> {
        match self.child(object, name)? {
            Some(child) => Ok(child),
            None => Err(Error::MissingProperty {
                class: self.class_name(object).unwrap_or("?").to_owned(),
                property: name.to_owned(),
            }),
        }
    }

    /// The file's root object, which owns everything else.
    ///
    /// # Errors
    ///
    /// Returns an error if the root storage cannot be read as an object.
    pub fn root(&mut self) -> Result<Object> {
        self.file.root()
    }

    /// The file's `Header`, which holds the content and the dictionary.
    ///
    /// # Errors
    ///
    /// Returns an error if the file has no header, which no valid AAF does.
    pub fn header(&mut self) -> Result<Object> {
        let root = self.root()?;
        self.required(&root, "Header")
    }

    /// The `ContentStorage`, which holds every mob in the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the header has no content storage.
    pub fn content(&mut self) -> Result<Object> {
        let header = self.header()?;
        self.required(&header, "Content")
    }

    /// Every mob in the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the content storage or one of the mobs cannot be
    /// read.
    pub fn mobs(&mut self) -> Result<Vec<Object>> {
        let content = self.content()?;
        self.children(&content, "Mobs")
    }

    /// The mobs of one kind, such as `CompositionMob` or `MasterMob`.
    ///
    /// Kinds inherit, so asking for `Mob` gives every mob in the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the content storage or one of the mobs cannot be
    /// read.
    pub fn mobs_of(&mut self, class_name: &str) -> Result<Vec<Object>> {
        Ok(self
            .mobs()?
            .into_iter()
            .filter(|mob| self.is_a(mob, class_name))
            .collect())
    }

    /// The composition mobs an application would open the file on.
    ///
    /// A file holds compositions that are whole sequences and compositions
    /// that are pieces of one; `Usage_TopLevel` is how the format tells them
    /// apart, and only the top-level ones are what somebody opened the file
    /// to see.
    ///
    /// # Errors
    ///
    /// Returns an error if the content storage or one of the mobs cannot be
    /// read.
    pub fn top_level_mobs(&mut self) -> Result<Vec<Object>> {
        let mut out = Vec::new();
        for mob in self.mobs_of("CompositionMob")? {
            if self
                .value(&mob, "UsageCode")?
                .as_ref()
                .and_then(Value::as_str)
                == Some("Usage_TopLevel")
            {
                out.push(mob);
            }
        }
        Ok(out)
    }

    /// A mob's slots, which are its tracks.
    ///
    /// # Errors
    ///
    /// Returns an error if the object is not a mob, or if a slot cannot be
    /// read.
    pub fn slots(&mut self, mob: &Object) -> Result<Vec<Object>> {
        self.children(mob, "Slots")
    }

    /// The file's `Dictionary`, which holds its definitions.
    ///
    /// Not to be confused with the meta dictionary: that one describes the
    /// format, and this one describes the content — what a `DataDef` called
    /// `Sound` is, which codecs the file uses, which operations its effects
    /// are.
    ///
    /// # Errors
    ///
    /// Returns an error if the header has no dictionary.
    pub fn dictionary(&mut self) -> Result<Object> {
        let header = self.header()?;
        self.required(&header, "Dictionary")
    }

    /// One of the dictionary's definition collections, keyed as the file keys
    /// it.
    ///
    /// `kind` is the collection's property name, such as `DataDefinitions` or
    /// `OperationDefinitions`.
    ///
    /// # Errors
    ///
    /// Returns an error if the dictionary has no collection of that name, or
    /// if one of the definitions cannot be read.
    pub fn definitions(&mut self, kind: &str) -> Result<Vec<(RefKey, Object)>> {
        let dictionary = self.dictionary()?;
        let pid = self.pid(&dictionary, kind)?;
        let Some(property) = dictionary.get(pid).cloned() else {
            return Ok(Vec::new());
        };
        self.file.strong_ref_set(&dictionary, &property)
    }

    /// The definition a key names, if the dictionary holds one.
    ///
    /// # Errors
    ///
    /// Returns an error if the dictionary has no collection of that name, or
    /// if one of the definitions cannot be read.
    pub fn definition(&mut self, kind: &str, key: Auid) -> Result<Option<Object>> {
        Ok(self
            .definitions(kind)?
            .into_iter()
            .find(|(found, _)| *found == RefKey::Auid(key))
            .map(|(_, object)| object))
    }

    /// What a component carries: `Picture`, `Sound`, `Timecode` and so on.
    ///
    /// A component says what it carries by pointing at a `DataDef` in the
    /// file's dictionary, and that definition's name is what the industry
    /// calls it. Applications disagree about how to spell it: the same sound
    /// definition is `Sound` in one fixture here and `DataDef_LegacySound` in
    /// the other. The `DataDef_` and `ContainerDef_` markers are taken out,
    /// as upstream takes them out, so that the two files answer alike.
    ///
    /// # Errors
    ///
    /// Returns an error if the object's class does not define
    /// `DataDefinition`, or if the dictionary cannot be read.
    pub fn media_kind(&mut self, component: &Object) -> Result<Option<String>> {
        let Some(RefKey::Auid(key)) = self.weak_key(component, "DataDefinition")? else {
            return Ok(None);
        };
        let Some(definition) = self.definition("DataDefinitions", key)? else {
            return Ok(None);
        };
        Ok(self.name(&definition)?.as_deref().map(short_name))
    }

    /// The mob an identifier names, if the file holds one.
    ///
    /// Mobs refer to each other by `MobID` rather than by reference, so
    /// following a source clip to the media it uses is this lookup.
    ///
    /// # Errors
    ///
    /// Returns an error if the content storage or one of the mobs cannot be
    /// read.
    pub fn mob(&mut self, id: MobId) -> Result<Option<Object>> {
        let content = self.content()?;
        let pid = self.pid(&content, "Mobs")?;
        let Some(property) = content.get(pid).cloned() else {
            return Ok(None);
        };
        Ok(self
            .file
            .strong_ref_set(&content, &property)?
            .into_iter()
            .find(|(key, _)| *key == RefKey::MobId(id))
            .map(|(_, mob)| mob))
    }

    /// The components of a sequence, in order.
    ///
    /// # Errors
    ///
    /// Returns an error if the object is not a sequence, or if a component
    /// cannot be read.
    pub fn components(&mut self, sequence: &Object) -> Result<Vec<Object>> {
        self.children(sequence, "Components")
    }
}

/// A definition's name with the markers AAF spells its names with taken out.
///
/// `DataDef_Sound` and `Sound` name the same thing, and files in the wild
/// carry both. Upstream removes `DataDef_` and `ContainerDef_` wherever they
/// appear rather than only at the front, and this does the same so that the
/// two agree on any name either would be given.
fn short_name(name: &str) -> String {
    name.replace("DataDef_", "").replace("ContainerDef_", "")
}

#[cfg(test)]
mod tests {
    use super::short_name;

    /// The two spellings the fixtures carry for one definition.
    ///
    /// `sector_size_512.aaf` names the sound definition `Sound` and
    /// `empty.aaf` names the same AUID `DataDef_LegacySound`. Both have to
    /// answer `media_kind` alike, which is what taking the marker out is for.
    #[test]
    fn a_definitions_short_name_drops_the_markers_aaf_spells_it_with() {
        assert_eq!(short_name("Sound"), "Sound");
        assert_eq!(short_name("DataDef_LegacySound"), "LegacySound");
        assert_eq!(short_name("DataDef_Picture"), "Picture");
        assert_eq!(short_name("ContainerDef_AAF"), "AAF");
        // A name carrying neither marker is its own short name, and one that
        // is nothing but a marker shortens to nothing rather than to itself.
        assert_eq!(short_name("Timecode"), "Timecode");
        assert_eq!(short_name("DataDef_"), "");
    }
}

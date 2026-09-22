//! Writing AAF files, byte for byte as pyaaf2 writes them.
//!
//! [`AafWriter`] builds a new AAF file in memory and hands back its bytes.
//! It is a port of pyaaf2's write path, and its promise is exact: the same
//! operations, in the same order, with the same times and identifiers,
//! produce the same file pyaaf2 produces, down to the byte. The tests hold
//! it to that against files pyaaf2 wrote.
//!
//! That is a stronger promise than a valid file, and it is the one that
//! matters for interchange. Editing applications are not all equally
//! forgiving of AAF, and the files they are known to accept are the ones
//! the tools already in use write. A file identical to pyaaf2's is one the
//! OpenTimelineIO AAF adapter's users have already been sending them.
//!
//! # The shape of the API
//!
//! pyaaf2 is an object API in a dynamic language: `f.create.SourceClip()`,
//! `clip['StartTime'].value = 10`, `mob.slots.append(slot)`. The same code
//! here is calls on the writer with an [`ObjRef`] naming the object:
//!
//! | pyaaf2 | here |
//! |---|---|
//! | `f.create.Filler('picture', 24)` | `w.create_filler("picture", 24)?` |
//! | `f.create.NetworkLocator()` | `w.create("NetworkLocator")?` |
//! | `obj['Name'].value = 'A'` | `w.set(obj, "Name", "A")?` |
//! | `obj['Slots'].append(slot)` | `w.append(obj, "Slots", slot)?` |
//! | `del obj['Name']` | `w.remove(obj, "Name")?` |
//! | `f.content.mobs.append(mob)` | `w.add_mob(mob)?` |
//! | `mob.create_timeline_slot(24)` | `w.create_timeline_slot(mob, 24, None)?` |
//! | `mob.comments['Scene'] = '12A'` | `w.set_tagged_value(mob, "UserComments", "Scene", "12A")?` |
//! | `mob.import_dnxhd_essence(path, 24)` | `w.import_dnxhd_essence(mob, path, EssenceImport { edit_rate: Some(24.into()), ..Default::default() })?` |
//! | `mob.import_audio_essence(path)` | `w.import_audio_essence(mob, path, EssenceImport::default())?` |
//! | `obj.copy(root=f)` | `w.copy_from(&mut source, &obj)?` |
//!
//! Property names are pyaaf2's, which are the AAF model's. Values are
//! [`WriteValue`]s, which convert from the Rust values they correspond to
//! and are encoded against the type the property declares, as pyaaf2
//! encodes them.
//!
//! Where pyaaf2's helper classes do work in their constructors — a new mob
//! gets a name, a fresh `MobID` and its creation time; a new component a
//! data definition and a length — the matching `create_*` method does the
//! same work, in the same order. [`create`](AafWriter::create) runs the
//! constructor pyaaf2 would run for a class, with its default arguments.
//!
//! # Essence
//!
//! Essence is a stream property, and pyaaf2 writes the stream of an object
//! that is not in the file yet under `/tmp`, moving it beside the object when
//! the object is attached and removing what is left of `/tmp` when the file
//! is closed. That leaves its mark on the directory, so this does the same.
//! The imports read the media in the same pieces pyaaf2 does, since each is
//! one write to the stream, and fail with pyaaf2's messages on media they
//! cannot read, as [`Error::InvalidMedia`].
//!
//! # Times and identifiers
//!
//! pyaaf2 reads the clock and draws random UUIDs as it goes. Here those come
//! from a [`Clock`] and an [`IdSource`] in the [`WriteOptions`], so that a
//! file can be reproduced exactly; see [`sources`].
//!
//! # Example
//!
//! ```
//! use aaf::write::{AafWriter, WriteOptions};
//!
//! let mut w = AafWriter::new()?;
//! let comp = w.create_mob("CompositionMob", Some("Edit"))?;
//! w.set(comp, "UsageCode", "Usage_TopLevel")?;
//! w.add_mob(comp)?;
//!
//! let slot = w.create_timeline_slot(comp, 24, None)?;
//! w.set(slot, "SlotName", "V1")?;
//! let sequence = w.create_sequence("picture")?;
//! w.set(slot, "Segment", sequence)?;
//! let filler = w.create_filler("picture", 48)?;
//! w.append(sequence, "Components", filler)?;
//! w.set(sequence, "Length", 48)?;
//!
//! let bytes = w.finish()?;
//! let aaf = aaf::Aaf::open(std::io::Cursor::new(bytes))?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod api;
mod copy;
mod dnx;
mod encode;
mod essence;
mod model;
mod object;
#[doc(hidden)]
pub mod replay;
pub mod sources;
mod value;
mod wave;

use std::path::Path;

pub use self::api::DefKey;
pub use self::essence::EssenceImport;
pub use self::sources::{
    Clock, FixedClock, IdSource, RandomIds, SequentialIds, SteppingClock, SystemClock,
};
pub use self::value::{ParseRationalError, Rational, Timestamp, WriteValue};

use self::model::{METADICT_CLASS, Model};
use self::object::{
    Body, Modified, Obj, SF_DATA, SF_DATA_STREAM, SF_STRONG, SF_STRONG_SET, SF_STRONG_VECTOR,
    SF_WEAK, SF_WEAK_SET, SF_WEAK_VECTOR,
};
use crate::cfb::{CompoundFileWriter, ROOT_ID};
use crate::error::{Error, Result};
use crate::{Auid, MobId};

/// An object in a file being written.
///
/// A handle, valid only with the [`AafWriter`] that made it. Objects are
/// made detached — in no property of anything in the file — and join the
/// file when they are assigned to a property of an object that is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef(pub(crate) u32);

/// How to write a new file.
pub struct WriteOptions {
    /// The container's sector size: 4096, pyaaf2's default, or 512.
    pub sector_size: u32,
    /// What the file's identification records as the platform it was
    /// written on. pyaaf2 records Python's `sys.platform`.
    pub platform: String,
    /// Whether to register Avid's extension classes and types, as pyaaf2
    /// does unless told not to. Files for Avid Media Composer need them.
    pub extensions: bool,
    /// Where times come from.
    pub clock: Box<dyn Clock + Send>,
    /// Where identifiers come from.
    pub ids: Box<dyn IdSource + Send>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            sector_size: 4096,
            platform: default_platform().to_owned(),
            extensions: true,
            clock: Box::new(SystemClock),
            ids: Box::new(RandomIds::new()),
        }
    }
}

impl std::fmt::Debug for WriteOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteOptions")
            .field("sector_size", &self.sector_size)
            .field("platform", &self.platform)
            .field("extensions", &self.extensions)
            .finish_non_exhaustive()
    }
}

/// The name Python's `sys.platform` gives this platform.
fn default_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        "linux" => "linux",
        "freebsd" => "freebsd",
        other => other,
    }
}

/// A new AAF file, being built.
///
/// Made with the header, content storage, dictionary and meta dictionary
/// every new file has, exactly as `aaf2.open(path, 'w')` makes them; see
/// the [module documentation](self) for how the rest of pyaaf2's API maps
/// onto this one.
pub struct AafWriter {
    cfb: CompoundFileWriter,
    objs: Vec<Obj>,
    modified: Modified,
    weakref_table: Vec<Vec<u16>>,
    model: Model,
    root: ObjRef,
    metadict: ObjRef,
    header: ObjRef,
    clock: Box<dyn Clock + Send>,
    ids: Box<dyn IdSource + Send>,
}

impl std::fmt::Debug for AafWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AafWriter")
            .field("objects", &self.objs.len())
            .finish_non_exhaustive()
    }
}

impl AafWriter {
    /// Starts a new file with pyaaf2's defaults: 4096-byte sectors, the
    /// extension model, the system clock and random identifiers.
    ///
    /// # Errors
    ///
    /// Only if the built-in model is inconsistent, which the tests rule out.
    pub fn new() -> Result<Self> {
        Self::with_options(WriteOptions::default())
    }

    /// Starts a new file.
    ///
    /// # Errors
    ///
    /// Returns an error for a sector size other than 512 or 4096.
    pub fn with_options(options: WriteOptions) -> Result<Self> {
        let mut w = Self {
            cfb: CompoundFileWriter::new(options.sector_size)?,
            objs: Vec::new(),
            modified: Modified::default(),
            weakref_table: Vec::new(),
            model: Model::new(),
            root: ObjRef(0),
            metadict: ObjRef(0),
            header: ObjRef(0),
            clock: options.clock,
            ids: options.ids,
        };
        w.setup_empty(&options.platform)?;
        if options.extensions {
            w.register_extensions()?;
        }
        Ok(w)
    }

    /// pyaaf2's `AAFFile.setup_empty`.
    fn setup_empty(&mut self, platform: &str) -> Result<()> {
        let now = self.clock.now();
        self.metadict = self.new_obj(METADICT_CLASS);
        self.build_base_model()?;

        let root_class = self
            .model
            .class_named("Root")
            .map(|c| self.model.classes[c].auid)
            .ok_or_else(|| Error::UndefinedClass {
                name: "Root".to_owned(),
            })?;
        self.root = self.new_obj(root_class);
        self.attach(self.root, ROOT_ID)?;
        self.set(self.root, "MetaDictionary", self.metadict)?;
        self.header = self.create("Header")?;
        self.set(self.root, "Header", self.header)?;

        let dictionary = self.create_dictionary()?;
        self.set(self.header, "Dictionary", dictionary)?;
        self.setup_defaults(dictionary)?;

        let content = self.create("ContentStorage")?;
        self.set(self.header, "Content", content)?;
        self.set(
            self.header,
            "OperationalPattern",
            crate::builtin::auid("0d011201-0100-0000-060e-2b3404010105"),
        )?;
        self.set(self.header, "ObjectModelVersion", 1)?;
        self.set(
            self.header,
            "Version",
            WriteValue::record([("major", 1), ("minor", 2)]),
        )?;

        let identification = self.create("Identification")?;
        self.set(identification, "ProductName", "PyAAF")?;
        self.set(identification, "CompanyName", "CompanyName")?;
        self.set(identification, "ProductVersionString", "2.0.0")?;
        self.set(
            identification,
            "ProductID",
            crate::builtin::auid("97e04c67-dbe6-4d11-bcd7-3a3a4253a2ef"),
        )?;
        self.set(identification, "Date", now)?;
        self.set(identification, "Platform", platform)?;
        let generation = self.ids.uuid4();
        self.set(identification, "GenerationAUID", generation)?;

        self.set(self.header, "IdentificationList", vec![identification])?;
        self.set(self.header, "LastModified", now)?;
        self.set(self.header, "ByteOrder", 0x4949)?;
        self.set(content, "Mobs", Vec::<ObjRef>::new())?;
        Ok(())
    }

    /// Finishes the file and returns its bytes: pyaaf2's `save` and
    /// `close`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MissingRequired`] if an object in the file lacks a
    /// property its class requires; pyaaf2 refuses to save that too.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.write_reference_properties()?;
        self.write_objects()?;
        self.remove_temp()?;
        Ok(self.cfb.finish()?)
    }

    /// Finishes the file and writes it to `path`.
    ///
    /// # Errors
    ///
    /// As for [`finish`](Self::finish), and if the file cannot be written.
    pub fn save(self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = self.finish()?;
        std::fs::write(path, bytes).map_err(|e| Error::Cfb(e.into()))
    }

    // --- the fixed objects ------------------------------------------------

    /// The root object, which holds the header and the meta dictionary.
    #[must_use]
    pub const fn root(&self) -> ObjRef {
        self.root
    }

    /// The meta dictionary, which holds the class and type definitions.
    #[must_use]
    pub const fn metadict(&self) -> ObjRef {
        self.metadict
    }

    /// The header: pyaaf2's `f.header`.
    #[must_use]
    pub const fn header(&self) -> ObjRef {
        self.header
    }

    /// The content storage, which holds the mobs: pyaaf2's `f.content`.
    ///
    /// # Errors
    ///
    /// Only if the header's `Content` has been removed.
    pub fn content(&self) -> Result<ObjRef> {
        self.required_object(self.header, "Content")
    }

    /// The dictionary, which holds the definitions: pyaaf2's `f.dictionary`.
    ///
    /// # Errors
    ///
    /// Only if the header's `Dictionary` has been removed.
    pub fn dictionary(&self) -> Result<ObjRef> {
        self.required_object(self.header, "Dictionary")
    }

    fn required_object(&self, obj: ObjRef, name: &str) -> Result<ObjRef> {
        self.get_object(obj, name)?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(obj),
                property: name.to_owned(),
            })
    }

    // --- properties -------------------------------------------------------

    /// Sets a property: pyaaf2's `obj[name].value = value`.
    ///
    /// The value is encoded against the type the property declares. A
    /// single reference takes an [`ObjRef`]; a collection of references a
    /// `Vec<ObjRef>`, which replaces what the collection held. An object
    /// assigned to an object in the file joins the file, with everything it
    /// owns; an object it replaces leaves it.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property, if the value
    /// cannot be stored as the property's type, or if an object is of the
    /// wrong class or already in the file.
    pub fn set(&mut self, obj: ObjRef, name: &str, value: impl Into<WriteValue>) -> Result<()> {
        self.check(obj)?;
        let value = value.into();
        let spec = self.find(obj, name)?;
        match spec.format {
            SF_DATA => {
                if spec.unique && self.obj(obj).dir.is_some() && self.prop(obj, spec.pid).is_some()
                {
                    self.swap_unique_key(obj, &spec, &value)?;
                }
                let data = self.encode(spec.type_id, &value)?;
                self.put_data(obj, spec.pid, data);
                Ok(())
            }
            SF_STRONG => match value {
                WriteValue::Object(child) => self.set_strong(obj, &spec, child),
                other => Err(Error::InvalidValue {
                    type_name: spec.name,
                    reason: format!("a strong reference takes an object, not {}", other.kind()),
                }),
            },
            SF_WEAK => match value {
                WriteValue::Object(target) => self.set_weak(obj, &spec, target),
                other => Err(Error::InvalidValue {
                    type_name: spec.name,
                    reason: format!("a weak reference takes an object, not {}", other.kind()),
                }),
            },
            SF_STRONG_VECTOR | SF_STRONG_SET | SF_WEAK_VECTOR | SF_WEAK_SET => match value {
                WriteValue::Objects(children) => {
                    self.clear_collection(obj, &spec)?;
                    self.extend_collection(obj, &spec, &children)
                }
                // pyaaf2 takes any empty iterable, a `dict` included, as a
                // collection of no objects.
                WriteValue::Array(items) if items.is_empty() => {
                    self.clear_collection(obj, &spec)?;
                    self.extend_collection(obj, &spec, &[])
                }
                WriteValue::Record(members) if members.is_empty() => {
                    self.clear_collection(obj, &spec)?;
                    self.extend_collection(obj, &spec, &[])
                }
                other => Err(Error::InvalidValue {
                    type_name: spec.name,
                    reason: format!("a collection takes a list of objects, not {}", other.kind()),
                }),
            },
            _ => Err(Error::WrongPropertyKind {
                property: spec.name,
                expected: "a value; write a stream with write_stream",
            }),
        }
    }

    /// Changing the key of an object filed in the content storage refiles
    /// it, as pyaaf2 does for a mob's or essence's `MobID`.
    fn swap_unique_key(
        &mut self,
        obj: ObjRef,
        spec: &object::Spec,
        value: &WriteValue,
    ) -> Result<()> {
        const MOB_MOBID: Auid = crate::builtin::auid("01011510-0000-0000-060e-2b3401010101");
        const ESSENCE_MOBID: Auid = crate::builtin::auid("06010106-0100-0000-060e-2b3401010102");
        let collection = match spec.auid {
            MOB_MOBID => "Mobs",
            ESSENCE_MOBID => "EssenceData",
            _ => {
                return Err(Error::Unsupported {
                    what: "changing the unique key of an object in the file",
                });
            }
        };
        let old = self
            .prop(obj, spec.pid)
            .map(|p| p.data.clone())
            .unwrap_or_default();
        let new = self.encode(spec.type_id, value)?;
        let content = self.content()?;
        let set = self.find(content, collection)?;
        let index = self.prop_pos(content, set.pid).ok_or(Error::Unsupported {
            what: "changing the key of an object not in the content storage",
        })?;
        if let Body::Set { entries, .. } = &mut self.obj_mut(content).props[index].body {
            let at = entries
                .iter()
                .position(|e| e.key == old)
                .ok_or(Error::Unsupported {
                    what: "changing the key of an object not in the content storage",
                })?;
            let mut entry = entries.remove(at);
            entry.key = new;
            entries.push(entry);
        }
        Ok(())
    }

    /// Removes a property: pyaaf2's `del obj[name]`, or setting it to
    /// `None`.
    ///
    /// As in pyaaf2, removing a reference does not take the object it
    /// referred to out of the file: an owned object stays where it was, is
    /// written, and is no longer referred to by anything.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn remove(&mut self, obj: ObjRef, name: &str) -> Result<()> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        self.remove_prop(obj, spec.pid);
        Ok(())
    }

    /// Adds an object to a collection: pyaaf2's `obj[name].append(child)`.
    ///
    /// Works on vectors, sets and arrays of weak references alike. A set
    /// files the object under its unique key, replacing any object already
    /// filed there.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a collection, or the object is
    /// of the wrong class or already in the file.
    pub fn append(&mut self, obj: ObjRef, name: &str, child: ObjRef) -> Result<()> {
        self.extend(obj, name, &[child])
    }

    /// Adds objects to a collection: pyaaf2's `obj[name].extend(children)`.
    ///
    /// # Errors
    ///
    /// As for [`append`](Self::append).
    pub fn extend(&mut self, obj: ObjRef, name: &str, children: &[ObjRef]) -> Result<()> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        self.extend_collection(obj, &spec, children)
    }

    /// Inserts an object into a vector: pyaaf2's
    /// `obj[name].insert(index, child)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a vector the object already
    /// has.
    pub fn insert(&mut self, obj: ObjRef, name: &str, index: usize, child: ObjRef) -> Result<()> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        if spec.format != SF_STRONG_VECTOR {
            return Err(Error::WrongPropertyKind {
                property: spec.name,
                expected: "a vector of owned objects",
            });
        }
        self.vector_insert(obj, &spec, index, child)
    }

    /// Empties a collection: pyaaf2's `obj[name].clear()`. Owned objects
    /// leave the file.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a collection.
    pub fn clear(&mut self, obj: ObjRef, name: &str) -> Result<()> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        self.clear_collection(obj, &spec)
    }

    /// Writes a stream property, such as the essence of an `EssenceData`:
    /// pyaaf2's `obj[name].open('w').write(data)`.
    ///
    /// Each call replaces what the stream held. The stream of an object not
    /// yet in the file is parked until the object is attached, as pyaaf2
    /// parks it, which draws an identifier from the writer's source.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a stream.
    pub fn write_stream(&mut self, obj: ObjRef, name: &str, data: &[u8]) -> Result<()> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        if spec.format != SF_DATA_STREAM {
            return Err(Error::WrongPropertyKind {
                property: spec.name,
                expected: "a stream",
            });
        }
        self.write_stream_prop(obj, &spec, data)
    }

    // --- the writer's sources -----------------------------------------------

    /// Reads the writer's clock: pyaaf2's `datetime.now()`.
    ///
    /// For code built on the writer that reads the time itself, as the
    /// OpenTimelineIO adapter does to date a new marker. Taking the time from
    /// here rather than from the system keeps a file written with a
    /// [`SteppingClock`], or a replayed one, reproducible, and keeps the
    /// order of readings the order pyaaf2 and its caller made them in.
    pub fn now(&mut self) -> Timestamp {
        self.clock.now()
    }

    // --- reading back ------------------------------------------------------

    /// The name of the type a property of an object's class declares:
    /// pyaaf2's `obj[name].typedef.type_name`.
    ///
    /// The object need not have the property yet.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UndefinedProperty`] if the class has no such
    /// property, which is where pyaaf2 raises `KeyError`.
    pub fn property_type_name(&self, obj: ObjRef, name: &str) -> Result<String> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        self.model
            .type_def(spec.type_id)
            .map(|t| t.name.clone())
            .ok_or(Error::UndefinedType {
                type_id: spec.type_id,
            })
    }

    /// The name of an object's class.
    #[must_use]
    pub fn class_name(&self, obj: ObjRef) -> String {
        self.class_name_of(obj)
    }

    /// Whether an object is of the named class or one derived from it:
    /// Python's `isinstance` on the object pyaaf2 makes of it.
    #[must_use]
    pub fn is_a(&self, obj: ObjRef, class: &str) -> bool {
        match (self.class_of(obj), self.model.class_named(class)) {
            (Ok(c), Some(wanted)) => self.model.derives_from(c, self.model.classes[wanted].auid),
            _ => false,
        }
    }

    /// Whether an object is in the file yet.
    #[must_use]
    pub fn is_attached(&self, obj: ObjRef) -> bool {
        self.objs
            .get(obj.0 as usize)
            .is_some_and(|o| o.dir.is_some())
    }

    /// Whether an object has a property: pyaaf2's `name in obj`.
    #[must_use]
    pub fn has(&self, obj: ObjRef, name: &str) -> bool {
        self.find(obj, name)
            .is_ok_and(|spec| self.prop(obj, spec.pid).is_some())
    }

    /// The bytes of a property, as they will be stored.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_bytes(&self, obj: ObjRef, name: &str) -> Result<Option<&[u8]>> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        Ok(self.prop(obj, spec.pid).map(|p| p.data.as_slice()))
    }

    /// The object a strong reference holds.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_object(&self, obj: ObjRef, name: &str) -> Result<Option<ObjRef>> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        Ok(match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::Strong { obj, .. }) => Some(*obj),
            _ => None,
        })
    }

    /// The objects an owned vector or set holds, in order.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_objects(&self, obj: ObjRef, name: &str) -> Result<Vec<ObjRef>> {
        self.check(obj)?;
        let spec = self.find(obj, name)?;
        Ok(match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::Vector { objs, .. }) => objs.clone(),
            Some(Body::Set { entries, .. }) => entries.iter().map(|e| e.obj).collect(),
            _ => Vec::new(),
        })
    }

    /// An integer property, or an enumeration's value.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_int(&self, obj: ObjRef, name: &str) -> Result<Option<i64>> {
        let Some(data) = self.get_bytes(obj, name)? else {
            return Ok(None);
        };
        let spec = self.find(obj, name)?;
        Ok(self.decode_int(spec.type_id, data))
    }

    /// A string property.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_string(&self, obj: ObjRef, name: &str) -> Result<Option<String>> {
        Ok(self.get_bytes(obj, name)?.map(Self::data_string))
    }

    /// An identifier property.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_auid(&self, obj: ObjRef, name: &str) -> Result<Option<Auid>> {
        Ok(self
            .get_bytes(obj, name)?
            .and_then(|d| <[u8; 16]>::try_from(d).ok())
            .map(Auid::from_bytes_le))
    }

    /// A MobID property.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn get_mob_id(&self, obj: ObjRef, name: &str) -> Result<Option<MobId>> {
        Ok(self
            .get_bytes(obj, name)?
            .and_then(|d| <[u8; 32]>::try_from(d).ok())
            .map(MobId::from_bytes))
    }

    /// Decodes an integer of the given type, following renames and
    /// enumerations to the integer underneath.
    fn decode_int(&self, type_id: Auid, data: &[u8]) -> Option<i64> {
        use self::model::Kind;
        match &self.model.type_def(type_id)?.kind {
            Kind::Int { size, signed } => {
                let size = usize::from(*size);
                if data.len() != size {
                    return None;
                }
                let mut buf = [0u8; 8];
                buf[..size].copy_from_slice(data);
                let raw = u64::from_le_bytes(buf);
                #[allow(clippy::cast_possible_wrap)]
                Some(if *signed && size < 8 {
                    let shift = 64 - 8 * size as u32;
                    ((raw << shift) as i64) >> shift
                } else {
                    raw as i64
                })
            }
            Kind::Enum { element, .. } => self.decode_int(*element, data),
            Kind::Rename { renamed } => self.decode_int(*renamed, data),
            _ => None,
        }
    }
}

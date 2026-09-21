//! Reading an AAF file as a tree of objects.
//!
//! Every AAF object is a storage in the compound file. The storage's class id
//! says what the object is, and a stream inside it named `properties` holds
//! its property values. Properties that own other objects name the storages
//! those objects live in, so the file is a tree and [`AafFile::walk`] is a
//! walk of it.
//!
//! What this module does *not* do is interpret property values. A property
//! whose format is [`PropertyFormat::Data`] is bytes until the file's own type
//! definitions say what they mean, and those live in the meta dictionary,
//! which is the layer above this one.
//!
//! # Example
//!
//! ```no_run
//! use std::fs::File;
//! use aaf::AafFile;
//!
//! let mut file = AafFile::open(File::open("example.aaf")?)?;
//! for (path, object) in file.walk()? {
//!     println!("{path} is a {}", object.class_id());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::io::{Read, Seek};

use crate::Auid;
use crate::cfb::{self, CompoundFile, DirId, ROOT_ID, cmp_names};
use crate::error::{Error, Result};
use crate::property::{
    Property, PropertyFormat, PropertyStream, PropertyValue, RefKey, SetIndex, VectorIndex,
    WeakRefArrayIndex, member_storage_name,
};

/// The stream inside every object's storage that holds its property values.
const PROPERTIES_STREAM: &str = "properties";

/// One AAF object: a class, and the properties stored against it.
#[derive(Debug, Clone)]
pub struct Object {
    storage: DirId,
    class_id: Auid,
    properties: PropertyStream,
}

impl Object {
    /// The storage this object lives in.
    #[must_use]
    pub const fn storage(&self) -> DirId {
        self.storage
    }

    /// The object's class.
    ///
    /// Which class this identifies is a question for the file's meta
    /// dictionary; the id itself is on the storage entry.
    #[must_use]
    pub const fn class_id(&self) -> Auid {
        self.class_id
    }

    /// The object's properties, in the order they were stored.
    #[must_use]
    pub fn properties(&self) -> &[Property] {
        &self.properties.properties
    }

    /// The format version the object was written with.
    #[must_use]
    pub const fn version(&self) -> u8 {
        self.properties.version
    }

    /// The property with this identifier, if the object has one.
    #[must_use]
    pub fn get(&self, pid: u16) -> Option<&Property> {
        self.properties.get(pid)
    }
}

/// An AAF file, opened for reading.
///
/// This is a compound file read as AAF: [`cfb`] gives the storages and
/// streams, and this gives the objects stored in them.
#[derive(Debug)]
pub struct AafFile<R> {
    cfb: CompoundFile<R>,
}

impl<R: Read + Seek> AafFile<R> {
    /// Opens an AAF file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is not a readable compound file. The AAF
    /// content is read on demand, so a file whose container is sound opens
    /// even if its objects are not.
    pub fn open(reader: R) -> Result<Self> {
        Ok(Self {
            cfb: CompoundFile::open(reader)?,
        })
    }

    /// The compound file underneath.
    #[must_use]
    pub const fn cfb(&self) -> &CompoundFile<R> {
        &self.cfb
    }

    /// The compound file underneath, mutably, for reading streams directly.
    pub const fn cfb_mut(&mut self) -> &mut CompoundFile<R> {
        &mut self.cfb
    }

    /// Reads the root object, which owns everything else in the file.
    ///
    /// The root object is the compound file's own root storage. Its two
    /// properties are the header, which holds the content, and the meta
    /// dictionary, which holds the definitions that give the content meaning.
    ///
    /// # Errors
    ///
    /// Returns an error if the root object's properties cannot be read.
    pub fn root(&mut self) -> Result<Object> {
        self.read_object(ROOT_ID)
    }

    /// Reads the object stored in `storage`.
    ///
    /// A storage with no `properties` stream is not an error: it reads as an
    /// object with no properties, which is what upstream `pyaaf2` does.
    ///
    /// # Errors
    ///
    /// Returns an error if `storage` is not in the file, or if its
    /// `properties` stream is malformed.
    pub fn read_object(&mut self, storage: DirId) -> Result<Object> {
        let entry = self.cfb.entry(storage)?;
        let class_id = entry.class_id().unwrap_or(Auid::NIL);

        let properties = match self.child(storage, PROPERTIES_STREAM)? {
            Some(stream) => PropertyStream::parse(&self.cfb.read_stream(stream)?)?,
            None => PropertyStream {
                version: 0,
                properties: Vec::new(),
            },
        };

        Ok(Object {
            storage,
            class_id,
            properties,
        })
    }

    /// Reads the index of a [`PropertyValue::StrongRefVector`] without its members.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a strong reference vector, or
    /// if its index stream is missing or malformed.
    pub fn vector_index(&mut self, owner: &Object, property: &Property) -> Result<VectorIndex> {
        if !matches!(property.value, PropertyValue::StrongRefVector { .. }) {
            return Err(Error::NotAReference {
                pid: property.pid,
                expected: "a strong reference vector",
            });
        }
        VectorIndex::parse(&self.read_index(owner, property)?)
    }

    /// Reads the index of a [`PropertyValue::StrongRefSet`] without its members.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a strong reference set, or if
    /// its index stream is missing or malformed.
    pub fn set_index(&mut self, owner: &Object, property: &Property) -> Result<SetIndex> {
        if !matches!(property.value, PropertyValue::StrongRefSet { .. }) {
            return Err(Error::NotAReference {
                pid: property.pid,
                expected: "a strong reference set",
            });
        }
        SetIndex::parse(&self.read_index(owner, property)?)
    }

    /// Resolves a [`PropertyValue::StrongRef`] to the object it owns.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a strong reference, or if the
    /// storage it names is not there.
    pub fn strong_ref(&mut self, owner: &Object, property: &Property) -> Result<Object> {
        let PropertyValue::StrongRef { name } = &property.value else {
            return Err(Error::NotAReference {
                pid: property.pid,
                expected: "a strong reference",
            });
        };
        let storage = self.require_child(owner.storage, name)?;
        self.read_object(storage)
    }

    /// Resolves a [`PropertyValue::StrongRefVector`] to the objects it owns, in order.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a strong reference vector, or
    /// if its index stream or any member it names is missing.
    pub fn strong_ref_vector(
        &mut self,
        owner: &Object,
        property: &Property,
    ) -> Result<Vec<Object>> {
        let PropertyValue::StrongRefVector { index_name } = &property.value else {
            return Err(Error::NotAReference {
                pid: property.pid,
                expected: "a strong reference vector",
            });
        };
        let index_name = index_name.clone();
        let index = self.vector_index(owner, property)?;

        let mut members = Vec::with_capacity(index.local_keys.len());
        for local_key in index.local_keys {
            let name = member_storage_name(&index_name, local_key);
            let storage = self.require_child(owner.storage, &name)?;
            members.push(self.read_object(storage)?);
        }
        Ok(members)
    }

    /// Resolves a [`PropertyValue::StrongRefSet`] to the objects it owns, with their keys.
    ///
    /// The order is the index's own, which is the order the writer happened to
    /// store them in rather than anything meaningful.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a strong reference set, or if
    /// its index stream or any member it names is missing.
    pub fn strong_ref_set(
        &mut self,
        owner: &Object,
        property: &Property,
    ) -> Result<Vec<(RefKey, Object)>> {
        let PropertyValue::StrongRefSet { index_name } = &property.value else {
            return Err(Error::NotAReference {
                pid: property.pid,
                expected: "a strong reference set",
            });
        };
        let index_name = index_name.clone();
        let index = self.set_index(owner, property)?;

        let mut members = Vec::with_capacity(index.entries.len());
        for entry in index.entries {
            let name = member_storage_name(&index_name, entry.local_key);
            let storage = self.require_child(owner.storage, &name)?;
            members.push((entry.key, self.read_object(storage)?));
        }
        Ok(members)
    }

    /// Reads the index of a [`PropertyValue::WeakRefArray`].
    ///
    /// The keys name objects owned elsewhere in the file. Turning a key into
    /// an object needs the file's table of reference targets, which is the
    /// layer above this one.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a weak reference collection, or
    /// if its index stream is missing.
    pub fn weak_ref_array(
        &mut self,
        owner: &Object,
        property: &Property,
    ) -> Result<WeakRefArrayIndex> {
        if !matches!(property.value, PropertyValue::WeakRefArray { .. }) {
            return Err(Error::NotAReference {
                pid: property.pid,
                expected: "a weak reference collection",
            });
        }
        WeakRefArrayIndex::parse(&self.read_index(owner, property)?)
    }

    /// Every object `owner` owns directly, with the property that owns it.
    ///
    /// Weak references are not followed: they point at objects some other part
    /// of the file owns, and following them here would visit those twice.
    ///
    /// # Errors
    ///
    /// Returns an error if any owned object cannot be read.
    pub fn children(&mut self, owner: &Object) -> Result<Vec<(u16, Object)>> {
        let mut out = Vec::new();
        for property in owner.properties().to_vec() {
            match property.format {
                PropertyFormat::StrongRef => {
                    out.push((property.pid, self.strong_ref(owner, &property)?));
                }
                PropertyFormat::StrongRefVector => {
                    out.extend(
                        self.strong_ref_vector(owner, &property)?
                            .into_iter()
                            .map(|object| (property.pid, object)),
                    );
                }
                PropertyFormat::StrongRefSet => {
                    out.extend(
                        self.strong_ref_set(owner, &property)?
                            .into_iter()
                            .map(|(_, object)| (property.pid, object)),
                    );
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Every object in the file, depth first from the root, with its path.
    ///
    /// The path is the storage path, so it matches what [`cfb`] reports and
    /// can be looked up with [`CompoundFile::find`].
    ///
    /// # Errors
    ///
    /// Returns an error if any object in the file cannot be read.
    pub fn walk(&mut self) -> Result<Vec<(String, Object)>> {
        let root = self.root()?;
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(object) = stack.pop() {
            let path = self.cfb.path(object.storage)?;
            for (_, child) in self.children(&object)?.into_iter().rev() {
                stack.push(child);
            }
            out.push((path, object));
        }
        Ok(out)
    }

    // --- lookups ------------------------------------------------------------

    /// Reads the index stream of a collection property.
    fn read_index(&mut self, owner: &Object, property: &Property) -> Result<Vec<u8>> {
        let name = property.index_stream_name().ok_or(Error::NotAReference {
            pid: property.pid,
            expected: "a collection",
        })?;
        let stream = self
            .child(owner.storage, &name)?
            .ok_or_else(|| Error::MissingIndex {
                name: name.clone(),
                parent: self.path_or_id(owner.storage),
            })?;
        Ok(self.cfb.read_stream(stream)?)
    }

    /// Finds an entry directly inside `parent`, by name.
    fn child(&self, parent: DirId, name: &str) -> Result<Option<DirId>> {
        Ok(self
            .cfb
            .children(parent)?
            .into_iter()
            .find(|entry| cmp_names(entry.name(), name).is_eq())
            .map(cfb::DirEntry::id))
    }

    /// Finds an entry directly inside `parent`, failing if it is not there.
    fn require_child(&self, parent: DirId, name: &str) -> Result<DirId> {
        self.child(parent, name)?
            .ok_or_else(|| Error::MissingEntry {
                name: name.to_owned(),
                parent: self.path_or_id(parent),
            })
    }

    /// A storage's path for an error message, falling back to its number.
    fn path_or_id(&self, storage: DirId) -> String {
        self.cfb
            .path(storage)
            .unwrap_or_else(|_| format!("directory entry {storage}"))
    }
}

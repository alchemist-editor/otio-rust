//! Changing an existing file: pyaaf2's `'r+'` and `'rw'` modes.
//!
//! pyaaf2 opens a file to change it by reading its container's tables into
//! the same state its writer keeps for a new file, and its objects into the
//! same object store. What it then writes back is only what changed: the
//! objects marked modified, the `properties` stream and indexes of each
//! rewritten in place, the directory entries touched, and the container's
//! tables. Everything else stays byte for byte as it was, stale sectors
//! included.
//!
//! [`AafWriter::open`] does the same, in the same order, so that the same
//! edits leave the same bytes:
//!
//! 1. The container is opened with [`CompoundFileWriter::open`], which reads
//!    its FAT, mini FAT, DIFAT and directory into the writer's state and
//!    keeps the file's bytes as the buffer later writes land in.
//! 2. The weak reference table, `/referenced properties`, is read.
//! 3. The standard model is built as pyaaf2's `MetaDictionary.__init__`
//!    builds it, with none of it in the file.
//! 4. The file's objects are read, root first. pyaaf2 reads objects lazily,
//!    as they are asked for; reading has no effect on the file, so here they
//!    are all read up front, and each is written back only if it is marked
//!    modified, as pyaaf2's are.
//! 5. The file's own class and type definitions replace the standard ones of
//!    the same name and identifier, and every standard definition the file
//!    lacks is added to it, as pyaaf2's `MetaDictionary.read_properties`
//!    does for a file opened for writing.
//! 6. Unless told not to, the Avid extensions are registered, as they are on
//!    a new file. On a file that already has them this still rewrites the
//!    meta dictionary and replaces each extension type that is not an
//!    enumeration with a new object in the same storage, because that is
//!    what pyaaf2's registration does.
//!
//! Saving is then what it is for a new file: the weak reference table is
//! rewritten, every modified object is written in the order it was first
//! marked, streams parked under `/tmp` are removed, and the container is
//! closed. pyaaf2 leaves the header's `LastModified` and identification
//! list alone when it saves an existing file, and so does this.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use super::model::{ClassInfo, Kind, METADICT_CLASS, Model, PropInfo, TypeInfo};
use super::object::{
    Body, Prop, SF_DATA_STREAM, SF_STRONG, SF_STRONG_SET, SF_STRONG_VECTOR, SF_WEAK, SF_WEAK_SET,
    SF_WEAK_VECTOR, SetEntry,
};
use super::sources::{Clock, IdSource, RandomIds, SystemClock};
use super::{AafWriter, ObjRef};
use crate::cfb::{CompoundFileWriter, DirId, ROOT_ID};
use crate::error::{Error, Result};
use crate::{Auid, utf16};

/// How to open an existing file for changing.
pub struct OpenOptions {
    /// Whether to register Avid's extension classes and types, as pyaaf2
    /// does unless told not to. On a file that has them already, this still
    /// rewrites the meta dictionary and the extension types; see
    /// [`AafWriter::open`].
    pub extensions: bool,
    /// Where times come from, for the mobs and markers made while the file
    /// is open. pyaaf2 does not date the file itself when it saves it.
    pub clock: Box<dyn Clock + Send>,
    /// Where identifiers come from: new mobs' `MobID`s, and the storages
    /// streams are parked in while their object is out of the file.
    pub ids: Box<dyn IdSource + Send>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            extensions: true,
            clock: Box::new(SystemClock),
            ids: Box::new(RandomIds::new()),
        }
    }
}

impl std::fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenOptions")
            .field("extensions", &self.extensions)
            .finish_non_exhaustive()
    }
}

/// The class of each kind of type definition, as `Kind::class_id` names it.
const TYPEDEF_CLASSES: [(&str, Auid); 16] = [
    (
        "int",
        crate::builtin::auid("0d010101-0204-0000-060e-2b3402060101"),
    ),
    (
        "strongref",
        crate::builtin::auid("0d010101-0205-0000-060e-2b3402060101"),
    ),
    (
        "weakref",
        crate::builtin::auid("0d010101-0206-0000-060e-2b3402060101"),
    ),
    (
        "enum",
        crate::builtin::auid("0d010101-0207-0000-060e-2b3402060101"),
    ),
    (
        "fixed",
        crate::builtin::auid("0d010101-0208-0000-060e-2b3402060101"),
    ),
    (
        "vararray",
        crate::builtin::auid("0d010101-0209-0000-060e-2b3402060101"),
    ),
    (
        "set",
        crate::builtin::auid("0d010101-020a-0000-060e-2b3402060101"),
    ),
    (
        "string",
        crate::builtin::auid("0d010101-020b-0000-060e-2b3402060101"),
    ),
    (
        "stream",
        crate::builtin::auid("0d010101-020c-0000-060e-2b3402060101"),
    ),
    (
        "record",
        crate::builtin::auid("0d010101-020d-0000-060e-2b3402060101"),
    ),
    (
        "rename",
        crate::builtin::auid("0d010101-020e-0000-060e-2b3402060101"),
    ),
    (
        "extenum",
        crate::builtin::auid("0d010101-0220-0000-060e-2b3402060101"),
    ),
    (
        "indirect",
        crate::builtin::auid("0d010101-0221-0000-060e-2b3402060101"),
    ),
    (
        "opaque",
        crate::builtin::auid("0d010101-0222-0000-060e-2b3402060101"),
    ),
    (
        "character",
        crate::builtin::auid("0d010101-0223-0000-060e-2b3402060101"),
    ),
    (
        "genericchar",
        crate::builtin::auid("0e040101-0000-0000-060e-2b3402060101"),
    ),
];

/// The root class, which pyaaf2 keeps out of every file's meta dictionary.
const ROOT_CLASS: Auid = crate::builtin::auid("b3b398a5-1c90-11d4-8053-080036210804");

/// The two root types pyaaf2 does not add to a file that lacks them.
const ROOT_TYPES: [Auid; 2] = [
    crate::builtin::auid("05022800-0000-0000-060e-2b3401040101"),
    crate::builtin::auid("05022700-0000-0000-060e-2b3401040101"),
];

const PID_CLASSDEFS: u16 = 0x0003;
const PID_TYPEDEFS: u16 = 0x0004;

/// One property as a `properties` stream stores it.
struct RawProp {
    pid: u16,
    format: u8,
    data: Vec<u8>,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn truncated(wanted: usize, found: usize) -> Error {
    Error::TruncatedIndex { wanted, found }
}

/// pyaaf2's `iter_utf16_array`: the NUL-terminated strings in `data`, and
/// nothing after the last terminator.
fn utf16_array(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in (0..data.len().saturating_sub(1)).step_by(2) {
        if data[i] == 0 && data[i + 1] == 0 {
            out.push(utf16::decode_le(&data[start..i]));
            start = i + 2;
        }
    }
    out
}

/// A run of 16-byte identifiers.
fn auid_array(data: &[u8]) -> Vec<Auid> {
    data.chunks_exact(16)
        .map(|c| Auid::from_bytes_le(c.try_into().expect("sixteen bytes")))
        .collect()
}

/// The key a weak reference's bytes hold, as an identifier.
fn weak_target(data: &[u8]) -> Option<Auid> {
    let key = data.get(5..21)?;
    Some(Auid::from_bytes_le(key.try_into().ok()?))
}

impl AafWriter {
    /// Opens an existing file to change it, with pyaaf2's defaults: the
    /// extension model, the system clock and random identifiers. This is
    /// `aaf2.open(path, 'r+')`, which pyaaf2 also spells `'rw'`.
    ///
    /// The whole file is read into memory. Nothing is written to `path`
    /// until [`save`](Self::save) is called with it. pyaaf2 changes the file
    /// where it lies, and cuts it after the last sector in use; saving
    /// writes the same bytes, whole.
    ///
    /// pyaaf2 reads objects as they are asked for, and this reads them all
    /// up front. Reading changes nothing, so the saved file is the same, but
    /// a file with an object that cannot be read fails here even if the
    /// edits would never have reached it.
    ///
    /// Any standard class or type definition the file lacks is added to it
    /// when it is saved, as pyaaf2 adds it. So are Avid's extensions, unless
    /// [`extensions`](OpenOptions::extensions) is off; in a file that has
    /// them already, registering them writes the meta dictionary and the
    /// extension types again, as they were.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, is not a compound file
    /// pyaaf2 would open, or holds objects that cannot be read.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(path, OpenOptions::default())
    }

    /// Opens an existing file to change it.
    ///
    /// # Errors
    ///
    /// As for [`open`](Self::open).
    pub fn open_with_options(path: impl AsRef<Path>, options: OpenOptions) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| Error::Cfb(e.into()))?;
        Self::open_bytes(bytes, options)
    }

    /// Opens a file read from `reader` to change it. Get the changed file
    /// back with [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// As for [`open`](Self::open).
    pub fn open_reader(mut reader: impl Read, options: OpenOptions) -> Result<Self> {
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Cfb(e.into()))?;
        Self::open_bytes(bytes, options)
    }

    /// Opens a file held in memory to change it. Get the changed file back
    /// with [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// As for [`open`](Self::open).
    pub fn open_bytes(bytes: Vec<u8>, options: OpenOptions) -> Result<Self> {
        let mut w = Self {
            cfb: CompoundFileWriter::open(bytes)?,
            objs: Vec::new(),
            modified: super::object::Modified::default(),
            weakref_table: Vec::new(),
            model: Model::new(),
            root: ObjRef(0),
            metadict: ObjRef(0),
            header: ObjRef(0),
            clock: options.clock,
            ids: options.ids,
            last_free_keys: HashMap::new(),
        };
        w.read_reference_properties()?;

        // pyaaf2's `MetaDictionary(self)`: the standard model, not in the
        // file, then pointed at the file's meta dictionary.
        w.metadict = w.new_obj(METADICT_CLASS);
        w.build_base_model()?;
        let builtin_classes = w.model.class_order();
        let builtin_types = w.model.type_order();
        let metadict_dir = w.cfb.find("/MetaDictionary-1").ok_or(Error::MissingEntry {
            name: "MetaDictionary-1".to_owned(),
            parent: "/".to_owned(),
        })?;
        w.obj_mut(w.metadict).dir = Some(metadict_dir);

        w.root = w.read_objects(metadict_dir)?;
        w.header = w
            .get_object(w.root, "Header")?
            .ok_or_else(|| Error::MissingProperty {
                class: "Root".to_owned(),
                property: "Header".to_owned(),
            })?;

        w.read_metadict()?;
        w.add_missing_definitions(&builtin_types, &builtin_classes)?;
        if options.extensions {
            w.register_extensions()?;
        }
        Ok(w)
    }

    // --- reading ------------------------------------------------------------

    /// pyaaf2's `read_reference_properties`.
    fn read_reference_properties(&mut self) -> Result<()> {
        let id = self
            .cfb
            .find("/referenced properties")
            .ok_or(Error::MissingEntry {
                name: "referenced properties".to_owned(),
                parent: "/".to_owned(),
            })?;
        let data = self.cfb.read_stream(id)?;
        if data.first() != Some(&0x4c) {
            return Err(Error::UnsupportedByteOrder {
                mark: data.first().copied().unwrap_or(0),
            });
        }
        let path_count = u16_at(&data, 1).ok_or_else(|| truncated(3, data.len()))?;
        let pid_count = u32_at(&data, 3).ok_or_else(|| truncated(7, data.len()))? as usize;
        let mut table = Vec::new();
        let mut path = Vec::new();
        for i in 0..pid_count {
            let pid = u16_at(&data, 7 + 2 * i).ok_or_else(|| truncated(9 + 2 * i, data.len()))?;
            if pid == 0 {
                table.push(std::mem::take(&mut path));
            } else {
                path.push(pid);
            }
        }
        if table.len() != usize::from(path_count) {
            return Err(Error::Unsupported {
                what: "a weak reference table whose count disagrees with its paths",
            });
        }
        self.weakref_table = table;
        Ok(())
    }

    /// Reads the file's objects, from the root down, and returns the root.
    ///
    /// The storage `metadict_dir` is the meta dictionary pyaaf2 has already
    /// made; its properties from the file replace the ones it was made with,
    /// each in its place, and new ones join the end, as reading into an
    /// existing object's property map does.
    fn read_objects(&mut self, metadict_dir: DirId) -> Result<ObjRef> {
        let root_class = self.cfb.class_id(ROOT_ID).ok_or(Error::Unsupported {
            what: "a root storage with no class",
        })?;
        let root = self.new_obj(root_class);
        self.obj_mut(root).dir = Some(ROOT_ID);
        let mut pending = vec![root];
        while let Some(obj) = pending.pop() {
            let dir = self.obj(obj).dir.expect("read objects are in the file");
            for raw in self.read_props(dir)? {
                let (prop, children) = self.read_prop(obj, dir, raw, metadict_dir)?;
                pending.extend(children);
                self.put_prop(obj, prop);
            }
        }
        Ok(root)
    }

    /// The properties in a storage's `properties` stream, in order. A storage
    /// with no such stream has none, as pyaaf2 reads it.
    fn read_props(&self, dir: DirId) -> Result<Vec<RawProp>> {
        let Some(stream) = self.cfb.get(dir, "properties") else {
            return Ok(Vec::new());
        };
        let data = self.cfb.read_stream(stream)?;
        if data.len() < 4 {
            return Err(Error::TruncatedProperty {
                wanted: 4,
                found: data.len(),
            });
        }
        if data[0] != 0x4c {
            return Err(Error::UnsupportedByteOrder { mark: data[0] });
        }
        let count = usize::from(u16::from_le_bytes([data[2], data[3]]));
        let mut at = 4 + 6 * count;
        if data.len() < at {
            return Err(Error::TruncatedProperty {
                wanted: at,
                found: data.len(),
            });
        }
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let entry = &data[4 + 6 * i..10 + 6 * i];
            let pid = u16::from_le_bytes([entry[0], entry[1]]);
            let format = u16::from_le_bytes([entry[2], entry[3]]);
            let len = usize::from(u16::from_le_bytes([entry[4], entry[5]]));
            let bytes = data.get(at..at + len).ok_or(Error::TruncatedProperty {
                wanted: at + len,
                found: data.len(),
            })?;
            let format = u8::try_from(format).map_err(|_| Error::Unsupported {
                what: "a property stored in a format no AAF writer uses",
            })?;
            out.push(RawProp {
                pid,
                format,
                data: bytes.to_vec(),
            });
            at += len;
        }
        Ok(out)
    }

    /// A stream in a storage, read whole, or an error naming it.
    fn read_named(&self, dir: DirId, name: &str) -> Result<Vec<u8>> {
        let id = self.cfb.get(dir, name).ok_or_else(|| Error::MissingIndex {
            name: name.to_owned(),
            parent: self.cfb.path(dir),
        })?;
        Ok(self.cfb.read_stream(id)?)
    }

    /// The object in storage `name` inside `dir`, made but not yet read.
    fn child_object(&mut self, dir: DirId, name: &str, metadict_dir: DirId) -> Result<ObjRef> {
        let child = self.cfb.get(dir, name).ok_or_else(|| Error::MissingEntry {
            name: name.to_owned(),
            parent: self.cfb.path(dir),
        })?;
        if child == metadict_dir {
            return Ok(self.metadict);
        }
        let class_id = self.cfb.class_id(child).ok_or(Error::Unsupported {
            what: "an object whose storage records no class",
        })?;
        let obj = self.new_obj(class_id);
        self.obj_mut(obj).dir = Some(child);
        Ok(obj)
    }

    /// One property read back into the shape the writer holds it in, with
    /// the objects it owns, which are still to be read.
    fn read_prop(
        &mut self,
        owner: ObjRef,
        dir: DirId,
        raw: RawProp,
        metadict_dir: DirId,
    ) -> Result<(Prop, Vec<ObjRef>)> {
        let RawProp { pid, format, data } = raw;
        let name = utf16::decode_le(&data);
        let mut children = Vec::new();
        let body = match format {
            SF_DATA_STREAM => {
                if data.first() != Some(&0x55) {
                    return Err(Error::UnsupportedByteOrder {
                        mark: data.first().copied().unwrap_or(0),
                    });
                }
                Body::Stream {
                    name: utf16::decode_le(&data[1..]),
                    parked: None,
                }
            }
            SF_STRONG => {
                let obj = self.child_object(dir, &name, metadict_dir)?;
                children.push(obj);
                Body::Strong { name, obj }
            }
            SF_STRONG_VECTOR => {
                let index = self.read_named(dir, &format!("{name} index"))?;
                let count = u32_at(&index, 0).ok_or_else(|| truncated(12, index.len()))? as usize;
                let next_free_key = u32_at(&index, 4).ok_or_else(|| truncated(12, index.len()))?;
                let last_free_key = u32_at(&index, 8).ok_or_else(|| truncated(12, index.len()))?;
                let mut keys = Vec::with_capacity(count);
                let mut objs = Vec::with_capacity(count);
                for i in 0..count {
                    let key = u32_at(&index, 12 + 4 * i)
                        .ok_or_else(|| truncated(16 + 4 * i, index.len()))?;
                    let obj =
                        self.child_object(dir, &format!("{name}{{{key:x}}}"), metadict_dir)?;
                    keys.push(key);
                    objs.push(obj);
                }
                children.extend(&objs);
                self.note_last_free_key(owner, pid, last_free_key);
                Body::Vector {
                    index_name: name,
                    keys,
                    objs,
                    next_free_key,
                }
            }
            SF_STRONG_SET => {
                let index = self.read_named(dir, &format!("{name} index"))?;
                let count = u32_at(&index, 0).ok_or_else(|| truncated(15, index.len()))? as usize;
                let next_free_key = u32_at(&index, 4).ok_or_else(|| truncated(15, index.len()))?;
                let last_free_key = u32_at(&index, 8).ok_or_else(|| truncated(15, index.len()))?;
                let key_pid = u16_at(&index, 12).ok_or_else(|| truncated(15, index.len()))?;
                let key_size = *index.get(14).ok_or_else(|| truncated(15, index.len()))?;
                if key_size != 16 && key_size != 32 {
                    return Err(Error::BadKeySize { size: key_size });
                }
                let entry_len = 8 + usize::from(key_size);
                let mut entries = Vec::with_capacity(count);
                for i in 0..count {
                    let at = 15 + entry_len * i;
                    let entry = index
                        .get(at..at + entry_len)
                        .ok_or_else(|| truncated(at + entry_len, index.len()))?;
                    let local_key = u32_at(entry, 0).expect("in range");
                    let obj =
                        self.child_object(dir, &format!("{name}{{{local_key:x}}}"), metadict_dir)?;
                    children.push(obj);
                    // pyaaf2 files by key, so a key listed twice keeps its
                    // first place and its last member.
                    let key = entry[8..].to_vec();
                    match entries.iter_mut().find(|e: &&mut SetEntry| e.key == key) {
                        Some(e) => {
                            e.local_key = local_key;
                            e.obj = obj;
                        }
                        None => entries.push(SetEntry {
                            key,
                            local_key,
                            obj,
                        }),
                    }
                }
                self.note_last_free_key(owner, pid, last_free_key);
                Body::Set {
                    index_name: name,
                    entries,
                    next_free_key,
                    key_pid,
                    key_size,
                }
            }
            SF_WEAK => Body::Weak,
            SF_WEAK_VECTOR | SF_WEAK_SET => {
                let index = self.read_named(dir, &format!("{name} index"))?;
                let count = u32_at(&index, 0).ok_or_else(|| truncated(9, index.len()))? as usize;
                let weakref_index = u16_at(&index, 4).ok_or_else(|| truncated(9, index.len()))?;
                let key_pid = u16_at(&index, 6).ok_or_else(|| truncated(9, index.len()))?;
                let key_size = *index.get(8).ok_or_else(|| truncated(9, index.len()))?;
                if key_size != 16 && key_size != 32 {
                    return Err(Error::BadKeySize { size: key_size });
                }
                let size = usize::from(key_size);
                let keys = (0..count)
                    .map(|i| {
                        let at = 9 + size * i;
                        index
                            .get(at..at + size)
                            .map(<[u8]>::to_vec)
                            .ok_or_else(|| truncated(at + size, index.len()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Body::WeakArray {
                    index_name: name,
                    weakref_index,
                    key_pid,
                    key_size,
                    keys,
                }
            }
            _ => Body::Data,
        };
        Ok((
            Prop {
                pid,
                format,
                data,
                body,
            },
            children,
        ))
    }

    /// Remembers a collection's last free key where it is not the one pyaaf2
    /// writes for a new collection, so that its index is written back with
    /// it.
    fn note_last_free_key(&mut self, obj: ObjRef, pid: u16, last_free_key: u32) {
        if last_free_key != u32::MAX {
            self.last_free_keys.insert((obj, pid), last_free_key);
        }
    }
}

// --- the file's own definitions ---------------------------------------------------

impl AafWriter {
    /// The bytes of a property an object has.
    fn data_of(&self, obj: ObjRef, pid: u16) -> Option<&[u8]> {
        self.prop(obj, pid).map(|p| p.data.as_slice())
    }

    /// A property a definition cannot do without.
    fn required_data(&self, obj: ObjRef, pid: u16) -> Result<&[u8]> {
        self.data_of(obj, pid)
            .ok_or_else(|| Error::MissingDefinitionProperty {
                definition: self
                    .data_of(obj, 0x0006)
                    .map_or_else(|| self.class_name_of(obj), utf16::decode_le),
                pid,
            })
    }

    fn required_auid(&self, obj: ObjRef, pid: u16) -> Result<Auid> {
        let data = self.required_data(obj, pid)?;
        <[u8; 16]>::try_from(data)
            .map(Auid::from_bytes_le)
            .map_err(|_| Error::TruncatedProperty {
                wanted: 16,
                found: data.len(),
            })
    }

    fn required_target(&self, obj: ObjRef, pid: u16) -> Result<Auid> {
        let data = self.required_data(obj, pid)?;
        weak_target(data).ok_or(Error::TruncatedProperty {
            wanted: 21,
            found: data.len(),
        })
    }

    /// The members of an owned set, in the order its index lists them.
    fn set_members(&self, obj: ObjRef, pid: u16) -> Vec<ObjRef> {
        match self.prop(obj, pid).map(|p| &p.body) {
            Some(Body::Set { entries, .. }) => entries.iter().map(|e| e.obj).collect(),
            _ => Vec::new(),
        }
    }

    /// Whether an owned set files something under `key`: pyaaf2's
    /// `key in obj[name]`.
    fn set_has(&self, obj: ObjRef, pid: u16, key: Auid) -> bool {
        let key = key.to_bytes_le();
        match self.prop(obj, pid).map(|p| &p.body) {
            Some(Body::Set { entries, .. }) => entries.iter().any(|e| e.key == key),
            _ => false,
        }
    }

    /// A type definition read from the file, as the model holds one.
    fn read_typedef(&self, obj: ObjRef) -> Result<TypeInfo> {
        let class_id = self.obj(obj).class_id;
        let kind_name = TYPEDEF_CLASSES
            .iter()
            .find(|(_, auid)| *auid == class_id)
            .map(|(name, _)| *name)
            .ok_or(Error::Unsupported {
                what: "a type definition of a class pyaaf2 does not know",
            })?;
        let name = utf16::decode_le(self.required_data(obj, 0x0006)?);
        let auid = self.required_auid(obj, 0x0005)?;
        let kind = match kind_name {
            "int" => Kind::Int {
                size: *self.required_data(obj, 0x000f)?.first().unwrap_or(&0),
                signed: self.required_data(obj, 0x0010)? == [1],
            },
            "strongref" => Kind::StrongRef {
                class: self.required_target(obj, 0x0011)?,
            },
            "weakref" => Kind::WeakRef {
                class: self.required_target(obj, 0x0012)?,
                target_set: auid_array(self.required_data(obj, 0x0013)?),
            },
            "enum" => {
                let names = utf16_array(self.required_data(obj, 0x0015)?);
                let values = self
                    .required_data(obj, 0x0016)?
                    .chunks_exact(8)
                    .take(names.len())
                    .map(|c| i64::from_le_bytes(c.try_into().expect("eight bytes")))
                    .collect();
                Kind::Enum {
                    element: self.required_target(obj, 0x0014)?,
                    names,
                    values,
                }
            }
            "fixed" => {
                let count = self.required_data(obj, 0x0018)?;
                Kind::FixedArray {
                    element: self.required_target(obj, 0x0017)?,
                    count: u32_at(count, 0).ok_or(Error::TruncatedProperty {
                        wanted: 4,
                        found: count.len(),
                    })?,
                }
            }
            "vararray" => Kind::VarArray {
                element: self.required_target(obj, 0x0019)?,
            },
            "set" => Kind::Set {
                element: self.required_target(obj, 0x001a)?,
            },
            "string" => Kind::String {
                element: self.required_target(obj, 0x001b)?,
            },
            "rename" => Kind::Rename {
                renamed: self.required_target(obj, 0x001e)?,
            },
            "record" => {
                let names = utf16_array(self.required_data(obj, 0x001d)?);
                let types = match self.prop(obj, 0x001c).map(|p| &p.body) {
                    Some(Body::WeakArray { keys, .. }) => keys
                        .iter()
                        .filter_map(|k| <[u8; 16]>::try_from(k.as_slice()).ok())
                        .map(Auid::from_bytes_le)
                        .collect::<Vec<_>>(),
                    _ => {
                        return Err(Error::MissingDefinitionProperty {
                            definition: name,
                            pid: 0x001c,
                        });
                    }
                };
                Kind::Record {
                    members: names.into_iter().zip(types).collect(),
                }
            }
            "extenum" => Kind::ExtEnum {
                names: utf16_array(self.required_data(obj, 0x001f)?),
                values: auid_array(self.required_data(obj, 0x0020)?),
            },
            "stream" => Kind::Stream,
            "indirect" => Kind::Indirect,
            "opaque" => Kind::Opaque,
            "character" => Kind::Character,
            _ => Kind::GenericCharacter,
        };
        Ok(TypeInfo {
            name,
            auid,
            kind,
            obj,
        })
    }

    /// A class definition read from the file, and its own properties'
    /// definitions in the order its `Properties` set lists them.
    fn read_classdef(&self, obj: ObjRef) -> Result<(ClassInfo, Vec<PropInfo>)> {
        let name = utf16::decode_le(self.required_data(obj, 0x0006)?);
        let auid = self.required_auid(obj, 0x0005)?;
        // pyaaf2 takes a class with no parent for a root class, as it takes
        // one that names itself.
        let parent = match self.data_of(obj, 0x0008) {
            Some(data) => weak_target(data).unwrap_or(auid),
            None => auid,
        };
        let concrete = self.data_of(obj, 0x000a) == Some(&[1]);
        let mut props = Vec::new();
        for def in self.set_members(obj, 0x0009) {
            let pid = self.required_data(def, 0x000d)?;
            props.push(PropInfo {
                name: utf16::decode_le(self.required_data(def, 0x0006)?),
                auid: self.required_auid(def, 0x0005)?,
                pid: u16_at(pid, 0).ok_or(Error::TruncatedProperty {
                    wanted: 2,
                    found: pid.len(),
                })?,
                type_id: self.required_auid(def, 0x000b)?,
                optional: self.data_of(def, 0x000c) == Some(&[1]),
                unique: self.data_of(def, 0x000e) == Some(&[1]),
            });
        }
        Ok((ClassInfo::new(name, auid, parent, concrete, obj), props))
    }

    /// The rest of pyaaf2's `MetaDictionary.read_properties`: the file's
    /// type definitions, then its class definitions, replace the standard
    /// ones filed under the same names and identifiers, and the dynamic
    /// property identifiers taken become the ones the file uses.
    fn read_metadict(&mut self) -> Result<()> {
        for typedef in self.set_members(self.metadict, PID_TYPEDEFS) {
            let info = self.read_typedef(typedef)?;
            self.model.file_type(info);
        }
        let mut local = Vec::new();
        for classdef in self.set_members(self.metadict, PID_CLASSDEFS) {
            let (info, props) = self.read_classdef(classdef)?;
            let class = self.model.file_class(info);
            for prop in props {
                if prop.pid >= 0x8000 {
                    local.push(prop.pid);
                }
                self.model.file_prop(class, prop);
            }
        }
        self.model.reset_local_pids(local);
        Ok(())
    }

    /// The definition object of one of a class's own properties.
    fn propertydef_object(&self, class: usize, prop: usize) -> Option<ObjRef> {
        let key = self.model.props[prop].auid.to_bytes_le();
        match self
            .prop(self.model.classes[class].obj, 0x0009)
            .map(|p| &p.body)
        {
            Some(Body::Set { entries, .. }) => entries.iter().find(|e| e.key == key).map(|e| e.obj),
            _ => None,
        }
    }

    /// Gives each of a class's own properties whose dynamic identifier the
    /// file already uses a new one, as pyaaf2 does before it adds a standard
    /// definition to a file. Returns each change, old identifier first.
    fn renumber_clashing_pids(&mut self, class: usize) -> Result<Vec<(u16, u16)>> {
        let mut changes = Vec::new();
        for prop in self.model.classes[class].props.clone() {
            let old = self.model.props[prop].pid;
            if old >= 0x8000 && self.model.is_local_pid(old) {
                let new = self.model.next_free_pid()?;
                self.model.repid_prop(class, prop, new);
                if let Some(def) = self.propertydef_object(class, prop) {
                    self.put_data(def, 0x000d, new.to_le_bytes().to_vec());
                }
                changes.push((old, new));
            }
        }
        Ok(changes)
    }

    /// The part of pyaaf2's `MetaDictionary.read_properties` that runs only
    /// for a file opened for writing: every standard type and class
    /// definition the file lacks is added to it, types first, in the order
    /// pyaaf2 first registered them. A type's class goes in with it if the
    /// file lacks that too.
    fn add_missing_definitions(&mut self, types: &[Auid], classes: &[Auid]) -> Result<()> {
        for &auid in types {
            if ROOT_TYPES.contains(&auid) || self.set_has(self.metadict, PID_TYPEDEFS, auid) {
                continue;
            }
            let typedef = self
                .model
                .type_def(auid)
                .map(|t| t.obj)
                .ok_or(Error::UndefinedType { type_id: auid })?;
            let class_id = self.obj(typedef).class_id;
            let class = self
                .model
                .class_index(class_id)
                .ok_or_else(|| Error::UndefinedClass {
                    name: class_id.to_string(),
                })?;
            for (old, new) in self.renumber_clashing_pids(class)? {
                // pyaaf2 moves the type's value to the new identifier, which
                // puts it last among the type's properties.
                let at = self.prop_pos(typedef, old).ok_or(Error::Unsupported {
                    what: "a standard type without a value for a renumbered property",
                })?;
                let mut prop = self.obj_mut(typedef).props.remove(at);
                prop.pid = new;
                self.obj_mut(typedef).props.push(prop);
            }
            self.extend(self.metadict, "TypeDefinitions", &[typedef])?;
            let class_auid = self.model.classes[class].auid;
            if !self.set_has(self.metadict, PID_CLASSDEFS, class_auid) {
                let classdef = self.model.classes[class].obj;
                self.extend(self.metadict, "ClassDefinitions", &[classdef])?;
            }
        }

        for &auid in classes {
            if auid == ROOT_CLASS || self.set_has(self.metadict, PID_CLASSDEFS, auid) {
                continue;
            }
            let class = self
                .model
                .class_index(auid)
                .ok_or_else(|| Error::UndefinedClass {
                    name: auid.to_string(),
                })?;
            self.renumber_clashing_pids(class)?;
            let classdef = self.model.classes[class].obj;
            self.extend(self.metadict, "ClassDefinitions", &[classdef])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_arrays_stop_at_the_last_terminator() {
        let mut data = super::super::object::encode_string("ab");
        data.extend(super::super::object::encode_string(""));
        data.extend([b'c', 0]);
        assert_eq!(utf16_array(&data), vec!["ab".to_owned(), String::new()]);
    }
}

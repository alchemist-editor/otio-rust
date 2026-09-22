//! The objects of a file being written, and how they reach the container.
//!
//! This is pyaaf2's object layer, from `core.py` and `properties.py`, rebuilt
//! so that the same operations leave the same bytes. Three things decide
//! where each object ends up in the compound file, and all three are kept
//! exactly as pyaaf2 keeps them:
//!
//! * **When a storage is made.** An object gets a storage the moment it is
//!   attached to the file — assigned to a property of an object already in
//!   it — and so does everything it owns, depth first, in property order.
//!   Assigning an object to an object that is not yet in the file makes no
//!   storage at all, until that one is attached in turn.
//! * **The order objects are written in.** Every object attached is recorded
//!   in an ordered map keyed by its path, and written out at the end in that
//!   order: its `properties` stream first, then an index stream for each
//!   collection it holds. An object that is detached — replaced by another,
//!   or removed with the collection it was in — leaves the map, and one
//!   attached in its place joins the end.
//! * **The order properties are held in.** An object's properties are an
//!   ordered map keyed by property identifier: a new one joins the end, and
//!   a changed one keeps its place.

use std::collections::HashMap;

use super::model::Kind;
use super::{AafWriter, ObjRef};
use crate::cfb::DirId;
use crate::error::{Error, Result};
use crate::{Auid, utf16};

/// Stored as bytes in the `properties` stream.
pub(crate) const SF_DATA: u8 = 0x82;
/// Stored as a stream of its own beside the `properties` stream.
pub(crate) const SF_DATA_STREAM: u8 = 0x42;
/// An object this one owns.
pub(crate) const SF_STRONG: u8 = 0x22;
/// Objects this one owns, in order.
pub(crate) const SF_STRONG_VECTOR: u8 = 0x32;
/// Objects this one owns, by unique key.
pub(crate) const SF_STRONG_SET: u8 = 0x3a;
/// An object owned elsewhere, by its unique key.
pub(crate) const SF_WEAK: u8 = 0x02;
/// Objects owned elsewhere, in order.
pub(crate) const SF_WEAK_VECTOR: u8 = 0x12;
/// Objects owned elsewhere, as a set.
pub(crate) const SF_WEAK_SET: u8 = 0x1a;

/// The version pyaaf2 writes in every `properties` stream header.
const PROPERTY_VERSION: u8 = 32;

/// `OperationGroup::Parameters`, which the AAF SDK stores as a set although
/// the model makes it a vector. pyaaf2 follows the SDK, and so does this.
pub(crate) const OPERATIONGROUP_PARAMETERS: Auid =
    crate::builtin::auid("06010104-060a-0000-060e-2b3401010102");

/// One object.
#[derive(Debug, Clone)]
pub(crate) struct Obj {
    /// The object's class.
    pub(crate) class_id: Auid,
    /// Its storage, once it is in the file.
    pub(crate) dir: Option<DirId>,
    /// Its properties, in the order they were first set.
    pub(crate) props: Vec<Prop>,
}

/// One property of an object.
#[derive(Debug, Clone)]
pub(crate) struct Prop {
    pub(crate) pid: u16,
    pub(crate) format: u8,
    /// The bytes in the `properties` stream. For a reference this is the
    /// name, index name or key that stands for it.
    pub(crate) data: Vec<u8>,
    pub(crate) body: Body,
}

/// What a property holds, beyond its bytes.
#[derive(Debug, Clone)]
pub(crate) enum Body {
    /// Nothing: the bytes are the value.
    Data,
    /// Bytes stored in a stream named in the property's data. Until its
    /// object is in the file the stream is parked in a storage of its own
    /// under `/tmp`, as pyaaf2 parks it, and moves beside the object's
    /// `properties` when the object is attached.
    Stream { name: String, parked: Option<DirId> },
    /// An owned object, stored in the storage `name`.
    Strong { name: String, obj: ObjRef },
    /// Owned objects in order, each stored as `index_name{key}`.
    Vector {
        index_name: String,
        keys: Vec<u32>,
        objs: Vec<ObjRef>,
        next_free_key: u32,
    },
    /// Owned objects by unique key, each stored as `index_name{local key}`.
    Set {
        index_name: String,
        entries: Vec<SetEntry>,
        next_free_key: u32,
        key_pid: u16,
        key_size: u8,
    },
    /// A reference by key; the data holds all of it.
    Weak,
    /// References by key, listed in an index stream.
    WeakArray {
        index_name: String,
        weakref_index: u16,
        key_pid: u16,
        key_size: u8,
        keys: Vec<Vec<u8>>,
    },
}

/// One member of an owned set.
#[derive(Debug, Clone)]
pub(crate) struct SetEntry {
    pub(crate) key: Vec<u8>,
    pub(crate) local_key: u32,
    pub(crate) obj: ObjRef,
}

/// A property of an object, looked up by name the way pyaaf2's `obj[name]`
/// looks it up.
#[derive(Debug, Clone)]
pub(crate) struct Spec {
    pub(crate) pid: u16,
    pub(crate) format: u8,
    pub(crate) name: String,
    pub(crate) auid: Auid,
    pub(crate) type_id: Auid,
    pub(crate) unique: bool,
}

/// The objects attached since the file was started, in the order they are
/// to be written: pyaaf2's `AAFObjectManager.modified`, an ordered dict.
#[derive(Debug, Default, Clone)]
pub(crate) struct Modified {
    order: Vec<Option<(String, ObjRef)>>,
    index: HashMap<String, usize>,
}

impl Modified {
    /// Records `obj` at `path`: in place if the path is already there, at
    /// the end otherwise.
    pub(crate) fn add(&mut self, path: String, obj: ObjRef) {
        if let Some(&i) = self.index.get(&path) {
            self.order[i] = Some((path, obj));
        } else {
            self.index.insert(path.clone(), self.order.len());
            self.order.push(Some((path, obj)));
        }
    }

    pub(crate) fn pop(&mut self, path: &str) {
        if let Some(i) = self.index.remove(path) {
            self.order[i] = None;
        }
    }

    pub(crate) fn objects(&self) -> Vec<ObjRef> {
        self.order.iter().flatten().map(|(_, obj)| *obj).collect()
    }
}

/// pyaaf2's `squeeze_name`: a name cut to `size` characters by keeping its
/// two ends and putting a hyphen between them.
fn squeeze(name: &str, size: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= size {
        return name.to_owned();
    }
    let half = size / 2;
    (0..size)
        .map(|i| match i.cmp(&half) {
            std::cmp::Ordering::Less => chars[i],
            std::cmp::Ordering::Equal => '-',
            std::cmp::Ordering::Greater => chars[chars.len() - (size - i)],
        })
        .collect()
}

/// pyaaf2's `mangle_name`: the name a property's storage or index is given,
/// `name-pid` in at most `size` characters.
pub(crate) fn mangle(name: &str, pid: u16, size: usize) -> String {
    let hex = format!("{pid:x}");
    let max = size - hex.len() - 2;
    format!("{}-{hex}", squeeze(name, max))
}

/// The objects one property owns, in order.
pub(crate) fn owned_objects(prop: &Prop) -> Vec<ObjRef> {
    AafWriter::owned(prop).into_iter().map(|(_, o)| o).collect()
}

/// A vector property's parts: its index name, local keys, objects and next
/// free key.
type VectorParts = (String, Vec<u32>, Vec<ObjRef>, u32);

/// A string as pyaaf2 stores one: UTF-16LE with a terminating NUL.
pub(crate) fn encode_string(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

impl AafWriter {
    // --- the arena ------------------------------------------------------

    pub(crate) fn new_obj(&mut self, class_id: Auid) -> ObjRef {
        let r = ObjRef(u32::try_from(self.objs.len()).expect("fewer than 2^32 objects"));
        self.objs.push(Obj {
            class_id,
            dir: None,
            props: Vec::new(),
        });
        r
    }

    pub(crate) fn obj(&self, r: ObjRef) -> &Obj {
        &self.objs[r.0 as usize]
    }

    pub(crate) fn obj_mut(&mut self, r: ObjRef) -> &mut Obj {
        &mut self.objs[r.0 as usize]
    }

    pub(crate) fn check(&self, r: ObjRef) -> Result<()> {
        if (r.0 as usize) < self.objs.len() {
            Ok(())
        } else {
            Err(Error::Unsupported {
                what: "an object from another writer",
            })
        }
    }

    pub(crate) fn prop_pos(&self, obj: ObjRef, pid: u16) -> Option<usize> {
        self.obj(obj).props.iter().position(|p| p.pid == pid)
    }

    pub(crate) fn prop(&self, obj: ObjRef, pid: u16) -> Option<&Prop> {
        self.obj(obj).props.iter().find(|p| p.pid == pid)
    }

    /// Puts a property in an object's map: in its old place if it had one,
    /// at the end if not.
    pub(crate) fn put_prop(&mut self, obj: ObjRef, prop: Prop) {
        let props = &mut self.obj_mut(obj).props;
        match props.iter_mut().find(|p| p.pid == prop.pid) {
            Some(slot) => *slot = prop,
            None => props.push(prop),
        }
    }

    pub(crate) fn put_data(&mut self, obj: ObjRef, pid: u16, data: Vec<u8>) {
        self.put_prop(
            obj,
            Prop {
                pid,
                format: SF_DATA,
                data,
                body: Body::Data,
            },
        );
    }

    pub(crate) fn remove_prop(&mut self, obj: ObjRef, pid: u16) {
        self.obj_mut(obj).props.retain(|p| p.pid != pid);
        self.last_free_keys.remove(&(obj, pid));
    }

    /// pyaaf2's `mark_modified`: an object in the file is to be written when
    /// the file is saved. The first mark puts it at the end of the modified
    /// map; later ones leave it where it is. pyaaf2 marks an object after
    /// each change made through one of its properties, so for an object read
    /// from an existing file the first change decides when it is written.
    pub(crate) fn mark_modified(&mut self, obj: ObjRef) {
        if let Some(dir) = self.obj(obj).dir {
            let path = self.cfb.path(dir);
            self.modified.add(path, obj);
        }
    }

    /// The position of a path of property identifiers in the file's table of
    /// weak reference targets, added at the end if it is new.
    pub(crate) fn weakref_index(&mut self, path: &[u16]) -> Result<u16> {
        let index = match self.weakref_table.iter().position(|p| p == path) {
            Some(i) => i,
            None => {
                self.weakref_table.push(path.to_vec());
                self.weakref_table.len() - 1
            }
        };
        u16::try_from(index).map_err(|_| Error::Unsupported {
            what: "more than 65536 weak reference targets",
        })
    }

    /// The bytes of a weak reference.
    pub(crate) fn weakref_data(index: u16, key_pid: u16, key: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(5 + key.len());
        data.extend_from_slice(&index.to_le_bytes());
        data.extend_from_slice(&key_pid.to_le_bytes());
        #[allow(clippy::cast_possible_truncation)]
        data.push(key.len() as u8);
        data.extend_from_slice(key);
        data
    }

    // --- looking properties up -----------------------------------------

    /// The class index of an object.
    pub(crate) fn class_of(&self, obj: ObjRef) -> Result<usize> {
        let class_id = self.obj(obj).class_id;
        self.model
            .class_index(class_id)
            .ok_or_else(|| Error::UndefinedClass {
                name: class_id.to_string(),
            })
    }

    pub(crate) fn class_name_of(&self, obj: ObjRef) -> String {
        self.class_of(obj).map_or_else(
            |_| self.obj(obj).class_id.to_string(),
            |c| self.model.classes[c].name.clone(),
        )
    }

    /// Looks a property up by name, as pyaaf2's `obj[name]` does: first among
    /// the properties the object has, then among those its class defines.
    pub(crate) fn find(&self, obj: ObjRef, name: &str) -> Result<Spec> {
        let class = self.class_of(obj)?;
        for p in &self.obj(obj).props {
            if let Some(def) = self.model.propdef_for_pid(class, p.pid) {
                let def = &self.model.props[def];
                if def.name == name {
                    return Ok(Spec {
                        pid: p.pid,
                        format: p.format,
                        name: def.name.clone(),
                        auid: def.auid,
                        type_id: def.type_id,
                        unique: def.unique,
                    });
                }
            }
        }
        for def in self.model.all_propertydefs(class) {
            let def = &self.model.props[def];
            if def.name == name {
                let format = if def.auid == OPERATIONGROUP_PARAMETERS {
                    SF_STRONG_SET
                } else {
                    self.model.store_format(def.type_id)?
                };
                return Ok(Spec {
                    pid: def.pid,
                    format,
                    name: def.name.clone(),
                    auid: def.auid,
                    type_id: def.type_id,
                    unique: def.unique,
                });
            }
        }
        Err(Error::UndefinedProperty {
            class: self.model.classes[class].name.clone(),
            property: name.to_owned(),
        })
    }

    fn wrong_kind(spec: &Spec, expected: &'static str) -> Error {
        Error::WrongPropertyKind {
            property: spec.name.clone(),
            expected,
        }
    }

    // --- unique keys ----------------------------------------------------

    /// The key an object is filed under in a set and referred to by: its
    /// unique property, or for a parameter its definition.
    pub(crate) fn unique_key(&self, obj: ObjRef) -> Result<Vec<u8>> {
        let class = self.class_of(obj)?;
        let pid = if self
            .model
            .derives_from(class, super::model::PARAMETER_CLASS)
        {
            self.find(obj, "Definition")?.pid
        } else {
            self.model
                .unique_key_pid(class)
                .ok_or_else(|| Error::NoUniqueKey {
                    class: self.model.classes[class].name.clone(),
                })?
        };
        self.prop(obj, pid)
            .map(|p| p.data.clone())
            .ok_or_else(|| Error::NoUniqueKey {
                class: self.model.classes[class].name.clone(),
            })
    }

    /// The class a reference type refers to, checked against `child`.
    fn check_class(&self, type_id: Auid, child: ObjRef) -> Result<usize> {
        let target = self
            .model
            .ref_classdef(type_id)
            .ok_or(Error::UndefinedType { type_id })?;
        let child_class = self.class_of(child)?;
        if self.model.is_instance(target, child_class) {
            Ok(target)
        } else {
            Err(Error::WrongClass {
                expected: self.model.classes[target].name.clone(),
                found: self.model.classes[child_class].name.clone(),
            })
        }
    }

    // --- attaching and detaching ----------------------------------------

    /// Puts an object, and everything it owns, into the storage `dir`.
    pub(crate) fn attach(&mut self, obj: ObjRef, dir: DirId) -> Result<()> {
        if self.obj(obj).dir.is_some() {
            return Err(Error::AlreadyAttached {
                class: self.class_name_of(obj),
            });
        }
        let class_id = self.obj(obj).class_id;
        self.obj_mut(obj).dir = Some(dir);
        self.cfb.set_class_id(dir, Some(class_id))?;
        let path = self.cfb.path(dir);
        self.modified.add(path, obj);
        for i in 0..self.obj(obj).props.len() {
            self.attach_prop(obj, i)?;
        }
        Ok(())
    }

    /// Moves a parked stream beside its object, now that the object is in
    /// the file: pyaaf2's `StreamProperty.attach`.
    fn attach_stream(&mut self, obj: ObjRef, index: usize, parent: DirId) -> Result<()> {
        let Body::Stream { name, parked } = &mut self.obj_mut(obj).props[index].body else {
            return Ok(());
        };
        let Some(stream) = parked.take() else {
            return Ok(());
        };
        let name = name.clone();
        if self.cfb.get(parent, &name).is_some() {
            return Err(Error::Unsupported {
                what: "attaching a stream where one of its name already is",
            });
        }
        self.cfb.move_entry(stream, parent, &name)?;
        Ok(())
    }

    /// The owned objects of one property, with the storage name of each.
    pub(crate) fn owned(prop: &Prop) -> Vec<(String, ObjRef)> {
        match &prop.body {
            Body::Strong { name, obj } => vec![(name.clone(), *obj)],
            Body::Vector {
                index_name,
                keys,
                objs,
                ..
            } => keys
                .iter()
                .zip(objs)
                .map(|(key, obj)| (format!("{index_name}{{{key:x}}}"), *obj))
                .collect(),
            Body::Set {
                index_name,
                entries,
                ..
            } => entries
                .iter()
                .map(|e| (format!("{index_name}{{{:x}}}", e.local_key), e.obj))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Attaches whatever one property owns, if its object is in the file.
    ///
    /// A stream parked under `/tmp` when its object left the file moves back
    /// into the object's storage: pyaaf2's `StreamProperty.attach`.
    fn attach_prop(&mut self, obj: ObjRef, index: usize) -> Result<()> {
        let Some(parent) = self.obj(obj).dir else {
            return Ok(());
        };
        self.attach_stream(obj, index, parent)?;
        for (name, child) in Self::owned(&self.obj(obj).props[index]) {
            self.attach_child(parent, &name, child)?;
        }
        Ok(())
    }

    fn attach_child(&mut self, parent: DirId, name: &str, child: ObjRef) -> Result<()> {
        let dir = match self.cfb.get(parent, name) {
            Some(dir) => dir,
            None => self.cfb.create_storage(parent, name, None)?,
        };
        if self.obj(child).dir != Some(dir) {
            self.attach(child, dir)?;
        }
        Ok(())
    }

    /// Takes an object, and everything it owns, out of the file.
    ///
    /// The storages stay where they are: pyaaf2 leaves them for whatever is
    /// attached in their place, and an object attached there later reuses
    /// them. A stream an object holds cannot stay behind like that, since it
    /// is the object's data: pyaaf2 parks it in a storage of its own under
    /// `/tmp`, named after a fresh UUID, until the object is attached again,
    /// and removes whatever is still parked there when the file is saved.
    pub(crate) fn detach(&mut self, obj: ObjRef) -> Result<()> {
        let children: Vec<ObjRef> = self
            .obj(obj)
            .props
            .iter()
            .flat_map(|p| Self::owned(p).into_iter().map(|(_, o)| o))
            .collect();
        for child in children {
            self.detach(child)?;
        }
        let streams: Vec<(u16, String)> = self
            .obj(obj)
            .props
            .iter()
            .filter_map(|p| match &p.body {
                Body::Stream { name, .. } => Some((p.pid, name.clone())),
                _ => None,
            })
            .collect();
        for (pid, name) in streams {
            self.park_stream(obj, pid, &name)?;
        }
        if let Some(dir) = self.obj_mut(obj).dir.take() {
            let path = self.cfb.path(dir);
            self.modified.pop(&path);
        }
        Ok(())
    }

    /// pyaaf2's `StreamProperty.detach`: moves a stream out of its object's
    /// storage into `/tmp/<uuid>`, with the UUID's hyphens for separators.
    fn park_stream(&mut self, obj: ObjRef, pid: u16, name: &str) -> Result<()> {
        let stream = self
            .obj(obj)
            .dir
            .and_then(|dir| self.cfb.get(dir, name))
            .ok_or(Error::Unsupported {
                what: "taking out an object whose stream is not in the file",
            })?;
        let id = self.ids.uuid4().to_string();
        let tmp = self
            .cfb
            .makedirs(&format!("/tmp/{}", id.replace('-', "/")))?;
        self.cfb.move_entry(stream, tmp, name)?;
        let index = self.prop_pos(obj, pid).expect("the object has the stream");
        if let Body::Stream { parked, .. } = &mut self.obj_mut(obj).props[index].body {
            *parked = Some(stream);
        }
        Ok(())
    }

    // --- single references ----------------------------------------------

    /// `obj[name].value = child` for a strong reference.
    pub(crate) fn set_strong(&mut self, obj: ObjRef, spec: &Spec, child: ObjRef) -> Result<()> {
        self.check(child)?;
        self.check_class(spec.type_id, child)?;
        let (name, old) = match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::Strong { name, obj }) => (name.clone(), Some(*obj)),
            Some(_) => return Err(Self::wrong_kind(spec, "a strong reference")),
            None => (mangle(&spec.name, spec.pid, 32), None),
        };
        if let Some(old) = old {
            self.detach(old)?;
        }
        self.put_prop(
            obj,
            Prop {
                pid: spec.pid,
                format: SF_STRONG,
                data: encode_string(&name),
                body: Body::Strong { name, obj: child },
            },
        );
        let index = self.prop_pos(obj, spec.pid).expect("just put");
        self.attach_prop(obj, index)?;
        self.mark_modified(obj);
        Ok(())
    }

    /// `obj[name].value = target` for a weak reference.
    pub(crate) fn set_weak(&mut self, obj: ObjRef, spec: &Spec, target: ObjRef) -> Result<()> {
        self.check(target)?;
        let ref_class = self.check_class(spec.type_id, target)?;
        let (key_pid, index) = match self.prop(obj, spec.pid) {
            Some(p) if matches!(p.body, Body::Weak) => {
                let index = u16::from_le_bytes([p.data[0], p.data[1]]);
                let key_pid = u16::from_le_bytes([p.data[2], p.data[3]]);
                (key_pid, index)
            }
            Some(_) => return Err(Self::wrong_kind(spec, "a weak reference")),
            None => {
                let key_pid =
                    self.model
                        .unique_key_pid(ref_class)
                        .ok_or_else(|| Error::NoUniqueKey {
                            class: self.model.classes[ref_class].name.clone(),
                        })?;
                let path = self.pid_path(spec.type_id)?;
                (key_pid, self.weakref_index(&path)?)
            }
        };
        let key = self.unique_key(target)?;
        self.put_prop(
            obj,
            Prop {
                pid: spec.pid,
                format: SF_WEAK,
                data: Self::weakref_data(index, key_pid, &key),
                body: Body::Weak,
            },
        );
        self.mark_modified(obj);
        Ok(())
    }

    /// The property identifiers leading from the root to where a weak
    /// reference type's objects are owned.
    pub(crate) fn pid_path(&self, type_id: Auid) -> Result<Vec<u16>> {
        let t = self
            .model
            .type_def(type_id)
            .ok_or(Error::UndefinedType { type_id })?;
        let target_set = match &t.kind {
            Kind::WeakRef { target_set, .. } => target_set.clone(),
            Kind::VarArray { element } | Kind::Set { element } => {
                return self.pid_path(*element);
            }
            _ => {
                return Err(Error::NotAReference {
                    pid: 0,
                    expected: "a weak reference type",
                });
            }
        };
        let mut class = self
            .model
            .class_named("Root")
            .ok_or_else(|| Error::UndefinedClass {
                name: "Root".to_owned(),
            })?;
        let mut path = Vec::with_capacity(target_set.len());
        for step in target_set {
            let def = self.model.classes[class]
                .props
                .iter()
                .map(|&p| &self.model.props[p])
                .find(|p| p.auid == step)
                .ok_or(Error::Unsupported {
                    what: "a weak reference whose target path does not resolve",
                })?;
            path.push(def.pid);
            class = self
                .model
                .ref_classdef(def.type_id)
                .ok_or(Error::UndefinedType {
                    type_id: def.type_id,
                })?;
        }
        Ok(path)
    }

    // --- owned vectors --------------------------------------------------

    fn vector_parts(&self, obj: ObjRef, spec: &Spec) -> Result<Option<VectorParts>> {
        match self.prop(obj, spec.pid).map(|p| &p.body) {
            None => Ok(None),
            Some(Body::Vector {
                index_name,
                keys,
                objs,
                next_free_key,
            }) => Ok(Some((
                index_name.clone(),
                keys.clone(),
                objs.clone(),
                *next_free_key,
            ))),
            Some(_) => Err(Self::wrong_kind(spec, "a vector of owned objects")),
        }
    }

    fn put_vector(
        &mut self,
        obj: ObjRef,
        pid: u16,
        index_name: String,
        keys: Vec<u32>,
        objs: Vec<ObjRef>,
        next_free_key: u32,
    ) {
        self.put_prop(
            obj,
            Prop {
                pid,
                format: SF_STRONG_VECTOR,
                data: encode_string(&index_name),
                body: Body::Vector {
                    index_name,
                    keys,
                    objs,
                    next_free_key,
                },
            },
        );
    }

    /// `obj[name].clear()` for a vector: detaches every member and starts
    /// the keys again from zero.
    pub(crate) fn vector_clear(&mut self, obj: ObjRef, spec: &Spec) -> Result<()> {
        if let Some((index_name, _, objs, _)) = self.vector_parts(obj, spec)? {
            for child in &objs {
                self.detach(*child)?;
            }
            self.put_vector(obj, spec.pid, index_name, Vec::new(), Vec::new(), 0);
        }
        self.mark_modified(obj);
        Ok(())
    }

    /// `obj[name].extend(children)` for a vector.
    pub(crate) fn vector_extend(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        children: &[ObjRef],
    ) -> Result<()> {
        for &child in children {
            self.check(child)?;
            self.check_class(spec.type_id, child)?;
            if self.obj(child).dir.is_some() {
                return Err(Error::AlreadyAttached {
                    class: self.class_name_of(child),
                });
            }
        }
        let (index_name, mut keys, mut objs, mut next) = self
            .vector_parts(obj, spec)?
            .unwrap_or_else(|| (mangle(&spec.name, spec.pid, 22), Vec::new(), Vec::new(), 0));
        for &child in children {
            keys.push(next);
            objs.push(child);
            next += 1;
        }
        self.put_vector(obj, spec.pid, index_name, keys, objs, next);
        let index = self.prop_pos(obj, spec.pid).expect("just put");
        self.attach_prop(obj, index)?;
        self.mark_modified(obj);
        Ok(())
    }

    /// `obj[name].insert(position, child)` for a vector.
    pub(crate) fn vector_insert(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        position: usize,
        child: ObjRef,
    ) -> Result<()> {
        self.check(child)?;
        self.check_class(spec.type_id, child)?;
        let Some((index_name, mut keys, mut objs, next)) = self.vector_parts(obj, spec)? else {
            return Err(Error::Unsupported {
                what: "inserting into a vector the object does not have yet",
            });
        };
        let position = position.min(keys.len());
        keys.insert(position, next);
        objs.insert(position, child);
        self.put_vector(obj, spec.pid, index_name, keys, objs, next + 1);
        let index = self.prop_pos(obj, spec.pid).expect("just put");
        self.attach_prop(obj, index)?;
        self.mark_modified(obj);
        Ok(())
    }

    /// `obj[name].pop(position)` for a vector: takes the member out of the
    /// file and returns it.
    pub(crate) fn vector_pop(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        position: usize,
    ) -> Result<ObjRef> {
        let Some((index_name, mut keys, mut objs, next)) = self.vector_parts(obj, spec)? else {
            return Err(Error::Unsupported {
                what: "taking a member out of a vector the object does not have",
            });
        };
        if position >= objs.len() {
            return Err(Error::Unsupported {
                what: "taking out a member past the end of a vector",
            });
        }
        keys.remove(position);
        let child = objs.remove(position);
        self.put_vector(obj, spec.pid, index_name, keys, objs, next);
        self.detach(child)?;
        self.mark_modified(obj);
        Ok(child)
    }

    /// `obj[name].pop(key)` for a set: takes the member filed under `key`
    /// out of the file and returns it.
    pub(crate) fn set_pop(&mut self, obj: ObjRef, spec: &Spec, key: &[u8]) -> Result<ObjRef> {
        let index = match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::Set { .. }) => self.prop_pos(obj, spec.pid).expect("found"),
            Some(_) => return Err(Self::wrong_kind(spec, "a set of owned objects")),
            None => {
                return Err(Error::Unsupported {
                    what: "taking a member out of a set the object does not have",
                });
            }
        };
        let Body::Set { entries, .. } = &mut self.obj_mut(obj).props[index].body else {
            unreachable!("checked above")
        };
        let at = entries
            .iter()
            .position(|e| e.key == key)
            .ok_or(Error::Unsupported {
                what: "taking out a member a set does not have",
            })?;
        let child = entries.remove(at).obj;
        self.detach(child)?;
        self.mark_modified(obj);
        Ok(child)
    }

    // --- owned sets -----------------------------------------------------

    /// pyaaf2's `add2set`: files `value` under `key` in the set at `pid`,
    /// replacing whatever was there, and attaches it if the set's object is
    /// in the file.
    pub(crate) fn add2set(
        &mut self,
        obj: ObjRef,
        pid: u16,
        key: Vec<u8>,
        value: ObjRef,
    ) -> Result<()> {
        let (current, name) = {
            let Some(Prop {
                body:
                    Body::Set {
                        index_name,
                        entries,
                        ..
                    },
                ..
            }) = self.prop(obj, pid)
            else {
                return Err(Error::Unsupported {
                    what: "adding to a set the object does not have",
                });
            };
            let current = entries
                .iter()
                .find(|e| e.key == key)
                .map(|e| (e.obj, e.local_key));
            (current, index_name.clone())
        };
        if let Some((old, _)) = current {
            if old != value {
                self.detach(old)?;
            }
        }
        let local_key = {
            let index = self.prop_pos(obj, pid).expect("checked above");
            let Body::Set {
                entries,
                next_free_key,
                ..
            } = &mut self.obj_mut(obj).props[index].body
            else {
                unreachable!("checked above")
            };
            if let Some(entry) = entries.iter_mut().find(|e| e.key == key) {
                entry.obj = value;
                entry.local_key
            } else {
                let local_key = *next_free_key;
                *next_free_key += 1;
                entries.push(SetEntry {
                    key,
                    local_key,
                    obj: value,
                });
                local_key
            }
        };
        if let Some(parent) = self.obj(obj).dir {
            self.attach_child(parent, &format!("{name}{{{local_key:x}}}"), value)?;
            self.mark_modified(obj);
        }
        Ok(())
    }

    /// Adds an empty owned set to an object, as pyaaf2's
    /// `add_strongref_set_property` does.
    pub(crate) fn add_set_property(
        &mut self,
        obj: ObjRef,
        pid: u16,
        name: &str,
        key_pid: u16,
        key_size: u8,
    ) {
        let index_name = mangle(name, pid, 22);
        self.put_prop(
            obj,
            Prop {
                pid,
                format: SF_STRONG_SET,
                data: encode_string(&index_name),
                body: Body::Set {
                    index_name,
                    entries: Vec::new(),
                    next_free_key: 0,
                    key_pid,
                    key_size,
                },
            },
        );
    }

    /// `obj[name].clear()` for a set.
    pub(crate) fn set_clear(&mut self, obj: ObjRef, spec: &Spec) -> Result<()> {
        let index = match self.prop(obj, spec.pid).map(|p| &p.body) {
            None => {
                self.mark_modified(obj);
                return Ok(());
            }
            Some(Body::Set { .. }) => self.prop_pos(obj, spec.pid).expect("found"),
            Some(_) => return Err(Self::wrong_kind(spec, "a set of owned objects")),
        };
        let members: Vec<ObjRef> = match &self.obj(obj).props[index].body {
            Body::Set { entries, .. } => entries.iter().map(|e| e.obj).collect(),
            _ => unreachable!("checked above"),
        };
        for member in members {
            self.detach(member)?;
        }
        if let Body::Set {
            entries,
            next_free_key,
            ..
        } = &mut self.obj_mut(obj).props[index].body
        {
            entries.clear();
            *next_free_key = 0;
        }
        self.mark_modified(obj);
        Ok(())
    }

    /// `obj[name].extend(children)` for a set.
    pub(crate) fn set_extend(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        children: &[ObjRef],
    ) -> Result<()> {
        let mut class = None;
        for &child in children {
            self.check(child)?;
            class = Some(self.check_class(spec.type_id, child)?);
            if self.obj(child).dir.is_some() {
                return Err(Error::AlreadyAttached {
                    class: self.class_name_of(child),
                });
            }
        }
        match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::Set { .. }) => {}
            Some(_) => return Err(Self::wrong_kind(spec, "a set of owned objects")),
            None => {
                let class = match class {
                    Some(c) => c,
                    None => self
                        .model
                        .ref_classdef(spec.type_id)
                        .ok_or(Error::UndefinedType {
                            type_id: spec.type_id,
                        })?,
                };
                let key_pid =
                    self.model
                        .unique_key_pid(class)
                        .ok_or_else(|| Error::NoUniqueKey {
                            class: self.model.classes[class].name.clone(),
                        })?;
                let key_size = self.model.unique_key_size(class);
                // Not yet in the object's map: it joins only once filled.
                let index_name = mangle(&spec.name, spec.pid, 22);
                let prop = Prop {
                    pid: spec.pid,
                    format: SF_STRONG_SET,
                    data: encode_string(&index_name),
                    body: Body::Set {
                        index_name,
                        entries: Vec::new(),
                        next_free_key: 0,
                        key_pid,
                        key_size,
                    },
                };
                self.obj_mut(obj).props.push(prop);
            }
        }
        for &child in children {
            let key = self.unique_key(child)?;
            self.add2set(obj, spec.pid, key, child)?;
        }
        self.mark_modified(obj);
        Ok(())
    }

    // --- weak reference arrays ------------------------------------------

    /// `obj[name].clear()` for an array of weak references.
    pub(crate) fn weak_array_clear(&mut self, obj: ObjRef, spec: &Spec) -> Result<()> {
        let Some(index) = self.prop_pos(obj, spec.pid) else {
            self.mark_modified(obj);
            return Ok(());
        };
        match &mut self.obj_mut(obj).props[index].body {
            Body::WeakArray { keys, .. } => {
                keys.clear();
                self.mark_modified(obj);
                Ok(())
            }
            _ => Err(Self::wrong_kind(spec, "an array of weak references")),
        }
    }

    /// `obj[name].extend(targets)` for an array of weak references.
    pub(crate) fn weak_array_extend(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        targets: &[ObjRef],
    ) -> Result<()> {
        let element = match self.model.type_def(spec.type_id).map(|t| &t.kind) {
            Some(Kind::VarArray { element } | Kind::Set { element }) => *element,
            _ => {
                return Err(Error::UndefinedType {
                    type_id: spec.type_id,
                });
            }
        };
        for &target in targets {
            self.check(target)?;
            self.check_class(element, target)?;
        }
        let ref_class = self
            .model
            .ref_classdef(element)
            .ok_or(Error::UndefinedType { type_id: element })?;
        let mut keys_to_add = Vec::with_capacity(targets.len());
        for &target in targets {
            keys_to_add.push(self.unique_key(target)?);
        }
        match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::WeakArray { .. }) => {}
            Some(_) => return Err(Self::wrong_kind(spec, "an array of weak references")),
            None => {
                let index_name = mangle(&spec.name, spec.pid, 22);
                let path = self.pid_path(element)?;
                let weakref_index = self.weakref_index(&path)?;
                let key_pid =
                    self.model
                        .unique_key_pid(ref_class)
                        .ok_or_else(|| Error::NoUniqueKey {
                            class: self.model.classes[ref_class].name.clone(),
                        })?;
                let key_size = self.model.unique_key_size(ref_class);
                self.put_prop(
                    obj,
                    Prop {
                        pid: spec.pid,
                        format: spec.format,
                        data: encode_string(&index_name),
                        body: Body::WeakArray {
                            index_name,
                            weakref_index,
                            key_pid,
                            key_size,
                            keys: Vec::new(),
                        },
                    },
                );
            }
        }
        let index = self.prop_pos(obj, spec.pid).expect("present");
        if let Body::WeakArray { keys, .. } = &mut self.obj_mut(obj).props[index].body {
            keys.extend(keys_to_add);
        }
        self.mark_modified(obj);
        Ok(())
    }

    // --- dispatch by format ----------------------------------------------

    /// Empties a collection property of any kind.
    pub(crate) fn clear_collection(&mut self, obj: ObjRef, spec: &Spec) -> Result<()> {
        match spec.format {
            SF_STRONG_VECTOR => self.vector_clear(obj, spec),
            SF_STRONG_SET => self.set_clear(obj, spec),
            SF_WEAK_VECTOR | SF_WEAK_SET => self.weak_array_clear(obj, spec),
            _ => Err(Self::wrong_kind(spec, "a collection")),
        }
    }

    /// Adds to a collection property of any kind.
    pub(crate) fn extend_collection(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        children: &[ObjRef],
    ) -> Result<()> {
        match spec.format {
            SF_STRONG_VECTOR => self.vector_extend(obj, spec, children),
            SF_STRONG_SET => self.set_extend(obj, spec, children),
            SF_WEAK_VECTOR | SF_WEAK_SET => self.weak_array_extend(obj, spec, children),
            _ => Err(Self::wrong_kind(spec, "a collection")),
        }
    }

    // --- streams --------------------------------------------------------

    /// Writes a stream property's bytes, as pyaaf2's `obj[name].open('w')`
    /// followed by one `write` does.
    pub(crate) fn write_stream_prop(
        &mut self,
        obj: ObjRef,
        spec: &Spec,
        bytes: &[u8],
    ) -> Result<()> {
        let stream = self.open_stream_prop(obj, spec)?;
        self.cfb.append_stream(stream, bytes)?;
        Ok(())
    }

    /// Opens a stream property for writing, as pyaaf2's `obj[name].open('w')`
    /// does: names the stream if it has no name yet, creates it if it is not
    /// there, and empties it. Returns the stream, for writing to with the
    /// container's `append_stream`.
    ///
    /// The stream of an object not yet in the file is created in a storage of
    /// its own under `/tmp`, named after a fresh identifier, and moved beside
    /// the object when the object is attached; whatever is still parked there
    /// when the file is finished is removed with `/tmp`. That is what pyaaf2
    /// does, and it draws the identifier at the same point.
    pub(crate) fn open_stream_prop(&mut self, obj: ObjRef, spec: &Spec) -> Result<DirId> {
        let (name, parked) = match self.prop(obj, spec.pid).map(|p| &p.body) {
            Some(Body::Stream { name, parked }) => (name.clone(), *parked),
            Some(_) => return Err(Self::wrong_kind(spec, "a stream")),
            None => {
                let name = mangle(&spec.name, spec.pid, 32);
                let mut data = vec![0x55];
                data.extend(encode_string(&name));
                self.put_prop(
                    obj,
                    Prop {
                        pid: spec.pid,
                        format: SF_DATA_STREAM,
                        data,
                        body: Body::Stream {
                            name: name.clone(),
                            parked: None,
                        },
                    },
                );
                (name, None)
            }
        };
        let stream = match (self.obj(obj).dir, parked) {
            (Some(dir), _) => self.cfb.touch(dir, &name)?,
            (None, Some(stream)) => stream,
            (None, None) => {
                let id = self.ids.uuid4().to_string().replace('-', "/");
                let dir = self.cfb.makedirs(&format!("/tmp/{id}"))?;
                let stream = self.cfb.touch(dir, &name)?;
                let index = self.prop_pos(obj, spec.pid).expect("put above");
                if let Body::Stream { parked, .. } = &mut self.obj_mut(obj).props[index].body {
                    *parked = Some(stream);
                }
                stream
            }
        };
        self.cfb.reopen_stream(stream)?;
        // An empty write empties the stream now, as opening it does in
        // pyaaf2, rather than at the first write.
        self.cfb.append_stream(stream, &[])?;
        Ok(stream)
    }

    /// Removes `/tmp`, and every stream still parked in it: pyaaf2's
    /// `remove_temp`, which it runs when the file is closed, after the
    /// objects are written.
    pub(crate) fn remove_temp(&mut self) -> Result<()> {
        if let Some(tmp) = self.cfb.find("/tmp") {
            self.cfb.rmtree(tmp)?;
        }
        Ok(())
    }

    // --- saving ---------------------------------------------------------

    /// Writes the table of weak reference targets, `/referenced properties`,
    /// one field at a time as pyaaf2 does.
    pub(crate) fn write_reference_properties(&mut self) -> Result<()> {
        const PATH: &str = "/referenced properties";
        let path_count =
            u16::try_from(self.weakref_table.len()).map_err(|_| Error::Unsupported {
                what: "more than 65535 weak reference targets",
            })?;
        let pid_count: usize = self.weakref_table.iter().map(|p| p.len() + 1).sum();
        let pid_count = u32::try_from(pid_count).map_err(|_| Error::Unsupported {
            what: "a weak reference table that large",
        })?;
        self.cfb.append_path(PATH, &[0x4c])?;
        self.cfb.append_path(PATH, &path_count.to_le_bytes())?;
        self.cfb.append_path(PATH, &pid_count.to_le_bytes())?;
        let table = self.weakref_table.clone();
        for path in table {
            for pid in path {
                self.cfb.append_path(PATH, &pid.to_le_bytes())?;
            }
            self.cfb.append_path(PATH, &[0, 0])?;
        }
        Ok(())
    }

    /// Checks an object has every property its class requires.
    pub(crate) fn validate(&self, obj: ObjRef) -> Result<()> {
        let class = self.class_of(obj)?;
        let missing: Vec<String> = self
            .model
            .all_propertydefs(class)
            .into_iter()
            .map(|p| &self.model.props[p])
            .filter(|p| !p.optional && p.name != "ObjClass" && self.prop(obj, p.pid).is_none())
            .map(|p| p.name.clone())
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::MissingRequired {
                class: self.model.classes[class].name.clone(),
                path: self
                    .obj(obj)
                    .dir
                    .map_or_else(String::new, |d| self.cfb.path(d)),
                properties: missing,
            })
        }
    }

    /// Writes every attached object, in the order they were attached: its
    /// `properties` stream, then an index for each collection it holds.
    pub(crate) fn write_objects(&mut self) -> Result<()> {
        for obj in self.modified.objects() {
            self.validate(obj)?;
            let dir = self.obj(obj).dir.expect("modified objects are attached");

            let props = &self.obj(obj).props;
            let count = u16::try_from(props.len()).map_err(|_| Error::Unsupported {
                what: "an object with more than 65535 properties",
            })?;
            let mut bytes = vec![0x4c, PROPERTY_VERSION];
            bytes.extend_from_slice(&count.to_le_bytes());
            for p in props {
                let len = u16::try_from(p.data.len()).map_err(|_| Error::InvalidValue {
                    type_name: format!("property {:#06x}", p.pid),
                    reason: "a property value holds at most 65535 bytes".to_owned(),
                })?;
                bytes.extend_from_slice(&p.pid.to_le_bytes());
                bytes.extend_from_slice(&u16::from(p.format).to_le_bytes());
                bytes.extend_from_slice(&len.to_le_bytes());
            }
            for p in props {
                bytes.extend_from_slice(&p.data);
            }

            let indexes: Vec<(String, Vec<u8>)> = props
                .iter()
                .filter_map(|p| {
                    let last = self.last_free_keys.get(&(obj, p.pid));
                    Self::index_stream(p, last.copied().unwrap_or(u32::MAX))
                })
                .collect();
            let stream = self.cfb.touch(dir, "properties")?;
            self.cfb.write_stream(stream, &bytes)?;
            for (name, index) in indexes {
                let stream = self.cfb.touch(dir, &name)?;
                self.cfb.write_stream(stream, &index)?;
            }
        }
        Ok(())
    }

    /// The index stream of a collection property, and its name. A new
    /// collection's last free key is `0xffffffff`; one read from a file keeps
    /// the one its index gave.
    #[allow(clippy::cast_possible_truncation)]
    fn index_stream(prop: &Prop, last_free_key: u32) -> Option<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let name = match &prop.body {
            Body::Vector {
                index_name,
                keys,
                next_free_key,
                ..
            } => {
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                out.extend_from_slice(&next_free_key.to_le_bytes());
                out.extend_from_slice(&last_free_key.to_le_bytes());
                for key in keys {
                    out.extend_from_slice(&key.to_le_bytes());
                }
                index_name
            }
            Body::Set {
                index_name,
                entries,
                next_free_key,
                key_pid,
                key_size,
            } => {
                out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                out.extend_from_slice(&next_free_key.to_le_bytes());
                out.extend_from_slice(&last_free_key.to_le_bytes());
                out.extend_from_slice(&key_pid.to_le_bytes());
                out.push(*key_size);
                for entry in entries {
                    out.extend_from_slice(&entry.local_key.to_le_bytes());
                    out.extend_from_slice(&1u32.to_le_bytes());
                    out.extend_from_slice(&entry.key);
                }
                index_name
            }
            Body::WeakArray {
                index_name,
                weakref_index,
                key_pid,
                key_size,
                keys,
            } => {
                out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
                out.extend_from_slice(&weakref_index.to_le_bytes());
                out.extend_from_slice(&key_pid.to_le_bytes());
                out.push(*key_size);
                for key in keys {
                    out.extend_from_slice(key);
                }
                index_name
            }
            _ => return None,
        };
        Some((format!("{name} index"), out))
    }

    // --- reading back what was written ----------------------------------

    /// The string a property holds, if it holds one.
    pub(crate) fn data_string(data: &[u8]) -> String {
        utf16::decode_le(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangles_names_as_pyaaf2_does() {
        assert_eq!(mangle("MetaDictionary", 1, 32), "MetaDictionary-1");
        assert_eq!(mangle("ClassDefinitions", 3, 22), "ClassDefinitions-3");
        // Too long: the ends are kept, a hyphen marks the cut.
        assert_eq!(
            mangle("ComponentAttributeList", 0xffc9, 22),
            "Componen-uteList-ffc9"
        );
        assert_eq!(mangle("MemberTypes", 0x1c, 32), "MemberTypes-1c");
    }
}

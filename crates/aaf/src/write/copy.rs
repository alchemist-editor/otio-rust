//! Copying objects in from another file: pyaaf2's `obj.copy(root=f)`.
//!
//! The OpenTimelineIO adapter embeds essence from another AAF by copying its
//! `EssenceData`, source mob and master mob into the file it is writing, and
//! this is pyaaf2's copy of an object into a new root, property by property:
//!
//! - values are copied as bytes, and a property with a dynamic pid takes the
//!   pid its definition has in this file;
//! - owned objects are copied in turn and set, appended or filed in the copy,
//!   one at a time, as pyaaf2 does;
//! - a weak reference is made to this file's object with the same key, which
//!   for a definition this file lacks means copying that definition into this
//!   file's dictionary first;
//! - a stream is copied a sector of the source at a time into a storage of
//!   its own under `/tmp`, as pyaaf2 parks the stream of an object not yet in
//!   the file, and moves beside its object when the copy joins the file.
//!
//! The copy is detached: it joins the file when it is put somewhere in it.
//!
//! pyaaf2 also registers the copied object's class, and every property and
//! type its class chain declares, in this file's meta dictionary if they are
//! not there yet. Between files that share a model, as files pyaaf2 wrote do,
//! that registers nothing. Registering a class or type that is missing is not
//! supported here, and neither is adding an element to an enumeration, so a
//! copy that would need either is refused rather than made differently.

use std::io::{Read, Seek};

use super::model::Kind;
use super::object::{
    Body, SF_DATA_STREAM, SF_STRONG_SET, SF_STRONG_VECTOR, SF_WEAK_SET, SF_WEAK_VECTOR, Spec,
};
use super::{AafWriter, ObjRef};
use crate::error::{Error, Result};
use crate::property::{PropertyValue, RefKey};
use crate::{Aaf, Auid, Object, TypeKind};

/// The bytes of a reference key, as a set files it.
fn key_bytes(key: &RefKey) -> Vec<u8> {
    match key {
        RefKey::Auid(auid) => auid.to_bytes_le().to_vec(),
        RefKey::MobId(mob_id) => mob_id.to_bytes().to_vec(),
    }
}

fn unregistered(what: &'static str) -> Error {
    Error::Unsupported { what }
}

impl AafWriter {
    /// Copies an object, and everything it owns, out of another file:
    /// pyaaf2's `obj.copy(root=f)`.
    ///
    /// The copy is not in this file yet; put it somewhere, as
    /// [`add_mob`](Self::add_mob) does a mob, and it joins the file with its
    /// streams. Weak references are made to this file's objects with the same
    /// keys, and a definition this file lacks is copied into its dictionary.
    ///
    /// # Errors
    ///
    /// Returns an error if the object, or anything it owns or refers to,
    /// cannot be read from `source`, or if copying it would need a class,
    /// property, type or enumeration element this file does not define.
    pub fn copy_from<R: Read + Seek>(
        &mut self,
        source: &mut Aaf<R>,
        object: &Object,
    ) -> Result<ObjRef> {
        let class_id = object.class_id();
        self.register_external_classdef(source, class_id)?;
        let new = self.new_obj(class_id);
        let class = self.class_of(new)?;
        for property in object.properties() {
            let pid = if property.pid >= 0x8000 {
                // A dynamic pid is the file's own; find the property by its
                // identifier instead.
                let auid = source
                    .metadict()
                    .property(class_id, property.pid)
                    .map(|p| p.auid)
                    .ok_or(unregistered("copying a property its file does not define"))?;
                self.model
                    .all_propertydefs(class)
                    .into_iter()
                    .map(|p| &self.model.props[p])
                    .find(|p| p.auid == auid)
                    .map(|p| p.pid)
                    .ok_or(unregistered("copying a property this file does not define"))?
            } else {
                property.pid
            };
            let spec = self.spec_for_pid(new, pid)?;
            match &property.value {
                PropertyValue::Data(data) => self.put_data(new, pid, data.clone()),
                PropertyValue::Stream { name } => {
                    self.copy_stream(source, object, name, new, &spec)?;
                }
                PropertyValue::StrongRef { .. } => {
                    let child = source.file().strong_ref(object, property)?;
                    let copy = self.copy_from(source, &child)?;
                    self.set_strong(new, &spec, copy)?;
                }
                PropertyValue::StrongRefVector { .. } => {
                    let children = source.file().strong_ref_vector(object, property)?;
                    let spec = Spec {
                        format: SF_STRONG_VECTOR,
                        ..spec
                    };
                    self.vector_extend(new, &spec, &[])?;
                    for child in children {
                        let copy = self.copy_from(source, &child)?;
                        self.vector_extend(new, &spec, &[copy])?;
                    }
                }
                PropertyValue::StrongRefSet { .. } => {
                    let children = source.file().strong_ref_set(object, property)?;
                    let spec = Spec {
                        format: SF_STRONG_SET,
                        ..spec
                    };
                    self.set_extend(new, &spec, &[])?;
                    for (_, child) in children {
                        let copy = self.copy_from(source, &child)?;
                        self.set_extend(new, &spec, &[copy])?;
                    }
                }
                PropertyValue::WeakRef { key, .. } => {
                    let path = self.pid_path(spec.type_id)?;
                    self.weakref_index(&path)?;
                    let target = self.weak_target(source, &path, key, true)?;
                    self.set_weak(new, &spec, target)?;
                }
                PropertyValue::WeakRefArray { .. } => {
                    let index = source.file().weak_ref_array(object, property)?;
                    let element = match self.model.type_def(spec.type_id).map(|t| &t.kind) {
                        Some(Kind::VarArray { element } | Kind::Set { element }) => *element,
                        _ => {
                            return Err(Error::UndefinedType {
                                type_id: spec.type_id,
                            });
                        }
                    };
                    let path = self.pid_path(element)?;
                    self.weakref_index(&path)?;
                    let mut targets = Vec::with_capacity(index.keys.len());
                    for key in &index.keys {
                        targets.push(self.weak_target(source, &path, key, false)?);
                    }
                    let format = if property.format == crate::property::PropertyFormat::WeakRefSet {
                        SF_WEAK_SET
                    } else {
                        SF_WEAK_VECTOR
                    };
                    self.weak_array_extend(new, &Spec { format, ..spec }, &targets)?;
                }
                _ => {
                    return Err(unregistered(
                        "copying a property stored in an opaque format",
                    ));
                }
            }
        }
        Ok(new)
    }

    /// A property of an object's class, by pid.
    fn spec_for_pid(&self, obj: ObjRef, pid: u16) -> Result<Spec> {
        let class = self.class_of(obj)?;
        let def = self
            .model
            .propdef_for_pid(class, pid)
            .map(|p| &self.model.props[p])
            .ok_or(unregistered("copying a property this file does not define"))?;
        Ok(Spec {
            pid,
            format: self.model.store_format(def.type_id)?,
            name: def.name.clone(),
            auid: def.auid,
            type_id: def.type_id,
            unique: def.unique,
        })
    }

    /// pyaaf2's `StreamProperty.copy`: the stream, read a source sector at a
    /// time and written as it is read.
    fn copy_stream<R: Read + Seek>(
        &mut self,
        source: &mut Aaf<R>,
        object: &Object,
        name: &str,
        new: ObjRef,
        spec: &Spec,
    ) -> Result<()> {
        let spec = Spec {
            format: SF_DATA_STREAM,
            ..spec.clone()
        };
        let file = source.file();
        let entry = file
            .cfb()
            .child(object.storage(), name)?
            .map(crate::cfb::DirEntry::id)
            .ok_or_else(|| Error::MissingEntry {
                name: name.to_owned(),
                parent: file
                    .cfb()
                    .path(object.storage())
                    .unwrap_or_else(|_| "?".to_owned()),
            })?;
        let data = file.cfb_mut().read_stream(entry)?;
        let chunk = file.cfb().sector_size() as usize;
        let stream = self.open_stream_prop(new, &spec)?;
        for piece in data.chunks(chunk) {
            self.cfb.append_stream(stream, piece)?;
        }
        Ok(())
    }

    /// The object in this file a weak reference names: the member of the
    /// set at `path` filed under `key`. pyaaf2 copies one a single weak
    /// reference names from the source file if this file lacks it, which is
    /// what `copy_missing` asks for; an array of weak references it requires
    /// to be there already.
    fn weak_target<R: Read + Seek>(
        &mut self,
        source: &mut Aaf<R>,
        path: &[u16],
        key: &RefKey,
        copy_missing: bool,
    ) -> Result<ObjRef> {
        let key = key_bytes(key);
        let (owner, set_pid) = self.weak_target_set(path)?;
        if let Some(found) = self.set_member(owner, set_pid, &key) {
            return Ok(found);
        }
        if !copy_missing {
            return Err(unregistered(
                "copying a reference to an object this file does not have",
            ));
        }
        // The object as the source file has it, by the same path.
        let (last, steps) = path.split_last().ok_or(Error::NotAReference {
            pid: 0,
            expected: "a weak reference path",
        })?;
        let mut current = source.root()?;
        for pid in steps {
            let property = current.get(*pid).cloned().ok_or(Error::NotAReference {
                pid: *pid,
                expected: "a strong reference on a weak reference path",
            })?;
            current = source.file().strong_ref(&current, &property)?;
        }
        let property = current.get(*last).cloned().ok_or(Error::NotAReference {
            pid: *last,
            expected: "a set on a weak reference path",
        })?;
        let member = source
            .file()
            .strong_ref_set(&current, &property)?
            .into_iter()
            .find(|(k, _)| key_bytes(k) == key)
            .map(|(_, object)| object)
            .ok_or(unregistered(
                "copying a reference to an object neither file has",
            ))?;
        // pyaaf2 registers a class or type definition in the meta
        // dictionary rather than copying it.
        if source.is_a(&member, "MetaDefinition") {
            return Err(unregistered(
                "registering a class or type definition from another file",
            ));
        }
        let copy = self.copy_from(source, &member)?;
        let spec = self.spec_for_pid(owner, set_pid)?;
        self.set_extend(owner, &spec, &[copy])?;
        Ok(copy)
    }

    /// The object holding the set a weak reference path ends in, and the
    /// set's pid.
    fn weak_target_set(&self, path: &[u16]) -> Result<(ObjRef, u16)> {
        let (last, steps) = path.split_last().ok_or(Error::NotAReference {
            pid: 0,
            expected: "a weak reference path",
        })?;
        let mut current = self.root;
        for pid in steps {
            current = match self.prop(current, *pid).map(|p| &p.body) {
                Some(Body::Strong { obj, .. }) => *obj,
                _ => {
                    return Err(Error::NotAReference {
                        pid: *pid,
                        expected: "a strong reference on a weak reference path",
                    });
                }
            };
        }
        Ok((current, *last))
    }

    /// The member of a set filed under `key`.
    fn set_member(&self, owner: ObjRef, pid: u16, key: &[u8]) -> Option<ObjRef> {
        match self.prop(owner, pid).map(|p| &p.body) {
            Some(Body::Set { entries, .. }) => entries.iter().find(|e| e.key == key).map(|e| e.obj),
            _ => None,
        }
    }

    /// pyaaf2's `register_external_classdef`, as far as it goes between files
    /// with the same model: checks this file already defines the class, the
    /// classes it derives from, and every property and property type they
    /// declare in the source file, with every enumeration element.
    fn register_external_classdef<R: Read + Seek>(
        &self,
        source: &Aaf<R>,
        class_id: Auid,
    ) -> Result<()> {
        let metadict = source.metadict();
        let mut current = metadict.class(class_id);
        let mut depth = 0;
        while let Some(class) = current {
            let parent = metadict.parent(class).filter(|p| p.auid != class.auid);
            // pyaaf2 skips the root of the chain, `InterchangeObject`.
            if parent.is_none() {
                break;
            }
            let Some(ours) = self.model.class_index(class.auid) else {
                return Err(unregistered(
                    "copying an object of a class this file does not define",
                ));
            };
            let defined: Vec<Auid> = self.model.classes[ours]
                .props
                .iter()
                .map(|&p| self.model.props[p].auid)
                .collect();
            for property in &class.properties {
                self.register_external_typedef(source, property.type_id)?;
                if !defined.contains(&property.auid) {
                    return Err(unregistered(
                        "copying an object whose class declares a property this file does not",
                    ));
                }
            }
            current = parent;
            depth += 1;
            if depth > metadict.class_count() {
                break;
            }
        }
        Ok(())
    }

    /// pyaaf2's `register_external_typedef`, as far as it goes between files
    /// with the same model: checks this file already defines the type, and
    /// for an enumeration every element the source file gives it.
    fn register_external_typedef<R: Read + Seek>(
        &self,
        source: &Aaf<R>,
        type_id: Auid,
    ) -> Result<()> {
        let Some(theirs) = source.metadict().type_def(type_id) else {
            return Ok(());
        };
        let Some(ours) = self.model.type_def(type_id) else {
            return Err(unregistered(
                "copying an object whose properties need a type this file does not define",
            ));
        };
        let missing_element = match (&theirs.kind, &ours.kind) {
            (TypeKind::Enum { elements, .. }, Kind::Enum { values, .. }) => {
                elements.iter().any(|(value, _)| !values.contains(value))
            }
            (TypeKind::ExtEnum { elements }, Kind::ExtEnum { values, .. }) => {
                elements.iter().any(|(value, _)| !values.contains(value))
            }
            _ => false,
        };
        if missing_element {
            return Err(unregistered(
                "copying an object whose enumerations have elements this file does not",
            ));
        }
        Ok(())
    }
}

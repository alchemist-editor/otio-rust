//! pyaaf2's helper classes, as methods on the writer.
//!
//! pyaaf2 gives many AAF classes a Python class of their own, whose
//! constructor sets properties and whose methods build common structures:
//! `Mob.__init__` names the mob and gives it a `MobID`, `create_timeline_slot`
//! picks a slot identifier, `Component.__init__` looks up a data definition.
//! Each method here does what one of those does, in the same order, so that
//! code ported from pyaaf2 line by line writes the same file.

use super::value::{Rational, WriteValue};
use super::{AafWriter, ObjRef};
use crate::builtin::raw;
use crate::error::{Error, Result};
use crate::{Auid, MobId};

/// A definition to look up in the dictionary.
///
/// pyaaf2's `lookup_datadef` and its siblings take a name, an identifier or
/// the definition itself, and so do the methods that take one of these. A
/// name is matched without case and without its `DataDef_` or
/// `ContainerDef_` prefix, so `"picture"` finds `DataDef_Picture`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefKey {
    /// A definition's name.
    Name(String),
    /// A definition's identifier.
    Auid(Auid),
    /// The definition object itself.
    Object(ObjRef),
}

impl From<&str> for DefKey {
    fn from(name: &str) -> Self {
        Self::Name(name.to_owned())
    }
}

impl From<String> for DefKey {
    fn from(name: String) -> Self {
        Self::Name(name)
    }
}

impl From<Auid> for DefKey {
    fn from(id: Auid) -> Self {
        Self::Auid(id)
    }
}

impl From<ObjRef> for DefKey {
    fn from(obj: ObjRef) -> Self {
        Self::Object(obj)
    }
}

/// A component's data definition, the way pyaaf2 abbreviates it.
fn short_name(name: &str) -> String {
    name.replace("DataDef_", "").replace("ContainerDef_", "")
}

/// Which of pyaaf2's constructors runs for a class, by the name the class
/// was asked for by. pyaaf2 finds its helper class by that name, so an
/// alias of a class with a helper gets a plain object.
enum Init {
    Plain,
    Component,
    Sequence,
    SourceClip,
    Timecode,
    Event,
    Mob,
    MobSlot,
    TimelineMobSlot,
    NeedsArguments,
    Dictionary,
}

fn init_for(name: &str) -> Init {
    match name {
        "Transition" | "NestedScope" | "Filler" | "EssenceGroup" | "EdgeCode" | "Pulldown"
        | "ScopeReference" | "Selector" => Init::Component,
        "Sequence" => Init::Sequence,
        "SourceClip" => Init::SourceClip,
        "Timecode" => Init::Timecode,
        "CommentMarker" | "DescriptiveMarker" => Init::Event,
        "CompositionMob" | "MasterMob" | "SourceMob" => Init::Mob,
        "EventMobSlot" | "StaticMobSlot" => Init::MobSlot,
        "TimelineMobSlot" => Init::TimelineMobSlot,
        "OperationGroup" | "CodecDef" => Init::NeedsArguments,
        "Dictionary" => Init::Dictionary,
        _ => Init::Plain,
    }
}

/// `int(float(rate) * 60 * 60 * 12)`: twelve hours of edit units, computed
/// in floating point as pyaaf2 computes it.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn twelve_hours(rate: Rational) -> i64 {
    let rate = rate.numerator as f64 / rate.denominator as f64;
    (rate * 60.0 * 60.0 * 12.0) as i64
}

impl AafWriter {
    // --- making objects ---------------------------------------------------

    /// A new object of the named class, with no properties.
    fn instance(&mut self, class: &str) -> Result<ObjRef> {
        let index = self
            .model
            .class_named(class)
            .ok_or_else(|| Error::UndefinedClass {
                name: class.to_owned(),
            })?;
        let c = &self.model.classes[index];
        if !c.concrete {
            return Err(Error::AbstractClass {
                name: c.name.clone(),
            });
        }
        let class_id = c.auid;
        Ok(self.new_obj(class_id))
    }

    /// A new object of the named class: pyaaf2's `f.create.<class>()`.
    ///
    /// Runs the constructor pyaaf2 runs for the class, with its default
    /// arguments: a mob is named `Mob` and given a new `MobID` and its
    /// creation time, a component gets the `picture` data definition and a
    /// length of zero, a timeline slot an edit rate of 25, and so on. The
    /// `create_*` methods take the arguments those constructors take.
    /// Classes pyaaf2 has no helper for are made with no properties.
    ///
    /// # Errors
    ///
    /// Returns an error if the class is not defined or is abstract, or if
    /// pyaaf2's constructor for it needs arguments: use
    /// [`create_operation_group`](Self::create_operation_group) for an
    /// `OperationGroup`.
    pub fn create(&mut self, class: &str) -> Result<ObjRef> {
        match init_for(class) {
            Init::Plain => self.instance(class),
            Init::Component => self.create_component(class, None, None),
            Init::Sequence => self.create_sequence("picture"),
            Init::SourceClip => self.create_source_clip(0, 0, MobId::NIL, 0, "picture"),
            Init::Timecode => self.create_timecode(25, false, None),
            Init::Event => self.create_component(class, Some("DescriptiveMetadata"), None),
            Init::Mob => self.create_mob(class, None),
            Init::MobSlot => {
                let slot = self.instance(class)?;
                self.set(slot, "SlotName", "")?;
                Ok(slot)
            }
            Init::TimelineMobSlot => {
                self.create_timeline_mob_slot(None, None, None, 0, Rational::new(25, 1))
            }
            Init::Dictionary => self.create_dictionary(),
            Init::NeedsArguments => Err(Error::Unsupported {
                what: "creating this class without the arguments pyaaf2 requires",
            }),
        }
    }

    /// A new component: pyaaf2's `Component.__init__(media_kind, length)`,
    /// which sets the data definition, `picture` unless told otherwise, and
    /// the length, zero unless told otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error if the class is not a concrete component, or no data
    /// definition matches `media_kind`.
    pub fn create_component(
        &mut self,
        class: &str,
        media_kind: Option<&str>,
        length: Option<i64>,
    ) -> Result<ObjRef> {
        let obj = self.instance(class)?;
        self.set_media_kind(obj, media_kind.unwrap_or("picture"))?;
        self.set(obj, "Length", length.unwrap_or(0))?;
        Ok(obj)
    }

    /// A new, empty sequence: pyaaf2's `f.create.Sequence(media_kind)`.
    ///
    /// # Errors
    ///
    /// Returns an error if no data definition matches `media_kind`.
    pub fn create_sequence(&mut self, media_kind: &str) -> Result<ObjRef> {
        let sequence = self.create_component("Sequence", Some(media_kind), None)?;
        self.set(sequence, "Components", Vec::<ObjRef>::new())?;
        Ok(sequence)
    }

    /// A new filler: pyaaf2's `f.create.Filler(media_kind, length)`.
    ///
    /// # Errors
    ///
    /// Returns an error if no data definition matches `media_kind`.
    pub fn create_filler(&mut self, media_kind: &str, length: i64) -> Result<ObjRef> {
        self.create_component("Filler", Some(media_kind), Some(length))
    }

    /// A new transition: pyaaf2's `f.create.Transition(media_kind, length)`.
    /// Give it an `OperationGroup` and a `CutPoint`.
    ///
    /// # Errors
    ///
    /// Returns an error if no data definition matches `media_kind`.
    pub fn create_transition(&mut self, media_kind: &str, length: i64) -> Result<ObjRef> {
        self.create_component("Transition", Some(media_kind), Some(length))
    }

    /// A new source clip: pyaaf2's
    /// `f.create.SourceClip(start, length, mob_id, slot_id, media_kind)`.
    ///
    /// pyaaf2's defaults are zeros, the nil `MobID` and `picture`, which is
    /// what to pass for any argument it would be called without.
    ///
    /// # Errors
    ///
    /// Returns an error if no data definition matches `media_kind`.
    pub fn create_source_clip(
        &mut self,
        start: i64,
        length: i64,
        mob_id: MobId,
        slot_id: u32,
        media_kind: &str,
    ) -> Result<ObjRef> {
        let clip = self.create_component("SourceClip", Some(media_kind), Some(length))?;
        self.set(clip, "StartTime", start)?;
        self.set(clip, "SourceID", mob_id)?;
        self.set(clip, "SourceMobSlotID", slot_id)?;
        Ok(clip)
    }

    /// A new timecode segment: pyaaf2's `f.create.Timecode(fps, drop,
    /// length)`, starting at zero and twelve hours long unless told
    /// otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error only if the model lacks the `Timecode` data
    /// definition.
    pub fn create_timecode(&mut self, fps: u16, drop: bool, length: Option<i64>) -> Result<ObjRef> {
        let length = length
            .filter(|l| *l != 0)
            .unwrap_or(i64::from(fps) * 60 * 60 * 12);
        let timecode = self.create_component("Timecode", Some("Timecode"), Some(length))?;
        self.set(timecode, "Start", 0)?;
        self.set(timecode, "FPS", fps)?;
        self.set(timecode, "Drop", drop)?;
        Ok(timecode)
    }

    /// A new operation group: pyaaf2's
    /// `f.create.OperationGroup(operationdef, length, media_kind)`.
    ///
    /// pyaaf2 gives an operation group the `picture` data definition unless
    /// told otherwise, whatever its operation's is.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation definition is not in the dictionary.
    pub fn create_operation_group(
        &mut self,
        operation: impl Into<DefKey>,
        length: i64,
        media_kind: Option<&str>,
    ) -> Result<ObjRef> {
        let group = self.create_component("OperationGroup", media_kind, Some(length))?;
        let operation = self.lookup_operationdef(operation)?;
        self.set(group, "Operation", operation)?;
        Ok(group)
    }

    // --- mobs -------------------------------------------------------------

    /// A new `MobID` as pyaaf2 makes one: a SMPTE UMID whose material
    /// number is a fresh identifier.
    pub fn new_mob_id(&mut self) -> MobId {
        let mut bytes = [0u8; 32];
        bytes[..12].copy_from_slice(&[
            0x06, 0x0a, 0x2b, 0x34, 0x01, 0x01, 0x01, 0x05, 0x01, 0x01, 0x0f, 0x20,
        ]);
        bytes[12] = 0x13;
        bytes[16..].copy_from_slice(&self.ids.uuid4().to_bytes_le());
        MobId::from_bytes(bytes)
    }

    /// A new mob: pyaaf2's `Mob.__init__(name)`. It is named `Mob` unless
    /// told otherwise, and gets a new `MobID`, its creation time and an
    /// empty list of slots.
    ///
    /// # Errors
    ///
    /// Returns an error if the class is not a concrete mob.
    pub fn create_mob(&mut self, class: &str, name: Option<&str>) -> Result<ObjRef> {
        let mob = self.instance(class)?;
        self.set(mob, "Name", name.unwrap_or("Mob"))?;
        let mob_id = self.new_mob_id();
        self.set(mob, "MobID", mob_id)?;
        let now = self.clock.now();
        self.set(mob, "CreationTime", now)?;
        self.set(mob, "LastModified", now)?;
        self.set(mob, "Slots", Vec::<ObjRef>::new())?;
        Ok(mob)
    }

    /// Puts a mob in the file: pyaaf2's `f.content.mobs.append(mob)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object is not a mob or is already in the file.
    pub fn add_mob(&mut self, mob: ObjRef) -> Result<()> {
        let content = self.content()?;
        self.append(content, "Mobs", mob)
    }

    /// A new timeline slot: pyaaf2's `f.create.TimelineMobSlot(slot_id,
    /// name, segment, origin, edit_rate)`.
    ///
    /// # Errors
    ///
    /// Returns an error if `segment` is not a segment.
    pub fn create_timeline_mob_slot(
        &mut self,
        slot_id: Option<u32>,
        name: Option<&str>,
        segment: Option<ObjRef>,
        origin: i64,
        edit_rate: Rational,
    ) -> Result<ObjRef> {
        let slot = self.instance("TimelineMobSlot")?;
        if let Some(slot_id) = slot_id {
            self.set(slot, "SlotID", slot_id)?;
        }
        self.set(slot, "SlotName", name.unwrap_or(""))?;
        if let Some(segment) = segment {
            self.set(slot, "Segment", segment)?;
        }
        self.set(slot, "Origin", origin)?;
        let edit_rate = if edit_rate.numerator == 0 {
            Rational::new(25, 1)
        } else {
            edit_rate
        };
        self.set(slot, "EditRate", edit_rate)?;
        Ok(slot)
    }

    /// The slot identifiers a mob's slots have.
    fn slot_ids(&self, mob: ObjRef) -> Result<Vec<i64>> {
        let mut ids = Vec::new();
        for slot in self.get_objects(mob, "Slots")? {
            if let Some(id) = self.get_int(slot, "SlotID")? {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// Adds a new timeline slot to a mob: pyaaf2's
    /// `mob.create_timeline_slot(edit_rate, slot_id)`.
    ///
    /// Without a slot identifier the slot gets one past the number of slots
    /// the mob has — counting from one, or from zero if the mob has a slot
    /// zero — which is what pyaaf2's search for a free identifier comes to.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob already has a slot `slot_id`.
    pub fn create_timeline_slot(
        &mut self,
        mob: ObjRef,
        edit_rate: impl Into<Rational>,
        slot_id: Option<u32>,
    ) -> Result<ObjRef> {
        let mut slots = self.slot_ids(mob)?;
        slots.sort_unstable();
        let slot_id = match slot_id {
            Some(id) if slots.contains(&i64::from(id)) => {
                return Err(Error::InvalidValue {
                    type_name: "SlotID".to_owned(),
                    reason: format!("slot {id} already exists"),
                });
            }
            Some(id) => i64::from(id),
            None => {
                // pyaaf2 walks the sorted identifiers and keeps the *last*
                // position that does not match, which is the one past the end.
                let start = if slots.first() == Some(&0) { 0 } else { 1 };
                let mut chosen = start;
                for (i, expected) in (start..).zip(slots.iter().map(Some).chain([None])) {
                    if expected != Some(&i) {
                        chosen = i;
                    }
                }
                chosen
            }
        };
        let slot_id = u32::try_from(slot_id).map_err(|_| Error::InvalidValue {
            type_name: "SlotID".to_owned(),
            reason: format!("{slot_id} does not fit"),
        })?;
        let slot = self.create_timeline_mob_slot(Some(slot_id), None, None, 0, edit_rate.into())?;
        self.append(mob, "Slots", slot)?;
        Ok(slot)
    }

    /// Adds a timeline slot holding a new, empty sequence: pyaaf2's
    /// `mob.create_empty_sequence_slot(edit_rate, slot_id, media_kind)`.
    ///
    /// # Errors
    ///
    /// As for [`create_timeline_slot`](Self::create_timeline_slot).
    pub fn create_empty_sequence_slot(
        &mut self,
        mob: ObjRef,
        edit_rate: impl Into<Rational>,
        slot_id: Option<u32>,
        media_kind: Option<&str>,
    ) -> Result<ObjRef> {
        let slot = self.create_timeline_slot(mob, edit_rate, slot_id)?;
        let sequence = self.create_sequence(media_kind.unwrap_or("picture"))?;
        self.set(sequence, "Components", Vec::<ObjRef>::new())?;
        self.set(slot, "Segment", sequence)?;
        Ok(slot)
    }

    /// pyaaf2's `mob.create_picture_slot(edit_rate)`.
    ///
    /// # Errors
    ///
    /// As for [`create_timeline_slot`](Self::create_timeline_slot).
    pub fn create_picture_slot(
        &mut self,
        mob: ObjRef,
        edit_rate: impl Into<Rational>,
    ) -> Result<ObjRef> {
        self.create_empty_sequence_slot(mob, edit_rate, None, Some("picture"))
    }

    /// pyaaf2's `mob.create_sound_slot(edit_rate)`.
    ///
    /// # Errors
    ///
    /// As for [`create_timeline_slot`](Self::create_timeline_slot).
    pub fn create_sound_slot(
        &mut self,
        mob: ObjRef,
        edit_rate: impl Into<Rational>,
    ) -> Result<ObjRef> {
        self.create_empty_sequence_slot(mob, edit_rate, None, Some("sound"))
    }

    /// Adds a timeline slot holding a source clip that refers to nothing:
    /// pyaaf2's `source_mob.create_empty_slot(edit_rate, media_kind,
    /// slot_id)`.
    ///
    /// # Errors
    ///
    /// As for [`create_timeline_slot`](Self::create_timeline_slot).
    pub fn create_empty_slot(
        &mut self,
        mob: ObjRef,
        edit_rate: impl Into<Rational>,
        media_kind: Option<&str>,
        slot_id: Option<u32>,
    ) -> Result<ObjRef> {
        let slot = self.create_timeline_slot(mob, edit_rate, slot_id)?;
        let clip = self.create_source_clip(0, 0, MobId::NIL, 0, media_kind.unwrap_or("picture"))?;
        self.set(slot, "Segment", clip)?;
        Ok(slot)
    }

    /// Adds a timecode slot: pyaaf2's `source_mob.create_timecode_slot(
    /// edit_rate, timecode_fps, drop_frame, length)`.
    ///
    /// # Errors
    ///
    /// As for [`create_timeline_slot`](Self::create_timeline_slot).
    pub fn create_timecode_slot(
        &mut self,
        mob: ObjRef,
        edit_rate: impl Into<Rational>,
        fps: u16,
        drop: bool,
        length: Option<i64>,
    ) -> Result<ObjRef> {
        let slot = self.create_timeline_slot(mob, edit_rate, None)?;
        let timecode = self.create_timecode(fps, drop, length)?;
        self.set(slot, "Segment", timecode)?;
        Ok(slot)
    }

    /// Makes a source mob a tape: pyaaf2's `source_mob.create_tape_slots(
    /// tape_name, edit_rate, timecode_fps, drop_frame, media_kind, length)`.
    ///
    /// Names the mob, describes it with a new `TapeDescriptor`, and adds
    /// slot 1, twelve hours of source, and a timecode slot. Returns the two
    /// slots.
    ///
    /// # Errors
    ///
    /// As for [`create_timeline_slot`](Self::create_timeline_slot).
    // pyaaf2's signature, argument for argument.
    #[allow(clippy::too_many_arguments)]
    pub fn create_tape_slots(
        &mut self,
        mob: ObjRef,
        tape_name: &str,
        edit_rate: impl Into<Rational>,
        fps: u16,
        drop: bool,
        media_kind: Option<&str>,
        length: Option<i64>,
    ) -> Result<(ObjRef, ObjRef)> {
        let edit_rate = edit_rate.into();
        self.set(mob, "Name", tape_name)?;
        let descriptor = self.create("TapeDescriptor")?;
        self.set(mob, "EssenceDescription", descriptor)?;
        let slot = self.create_empty_slot(mob, edit_rate, media_kind, Some(1))?;
        let clip = self.required_segment(slot)?;
        self.set(clip, "Length", twelve_hours(edit_rate))?;
        let timecode = self.create_timecode_slot(mob, edit_rate, fps, drop, length)?;
        Ok((slot, timecode))
    }

    fn required_segment(&self, slot: ObjRef) -> Result<ObjRef> {
        self.get_object(slot, "Segment")?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(slot),
                property: "Segment".to_owned(),
            })
    }

    /// A mob's slot with the given identifier: pyaaf2's `mob.slot_at(id)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob has no such slot.
    pub fn slot_at(&self, mob: ObjRef, slot_id: u32) -> Result<ObjRef> {
        for slot in self.get_objects(mob, "Slots")? {
            if self.get_int(slot, "SlotID")? == Some(i64::from(slot_id)) {
                return Ok(slot);
            }
        }
        Err(Error::InvalidValue {
            type_name: "SlotID".to_owned(),
            reason: format!("no slot {slot_id}"),
        })
    }

    /// A slot's length: pyaaf2's `slot.length`. For a timeline slot that
    /// is its segment's length, a sequence's being the sum of its
    /// components' less its transitions'.
    ///
    /// # Errors
    ///
    /// Returns an error if the slot has no segment.
    pub fn slot_length(&self, slot: ObjRef) -> Result<i64> {
        let class = self.class_of(slot)?;
        let is = |name: &str, class: usize| {
            self.model
                .class_named(name)
                .is_some_and(|c| self.model.derives_from(class, self.model.classes[c].auid))
        };
        if !is("TimelineMobSlot", class) {
            return Ok(0);
        }
        let segment = self.required_segment(slot)?;
        let segment_class = self.class_of(segment)?;
        if is("Sequence", segment_class) {
            let mut length = 0;
            for component in self.get_objects(segment, "Components")? {
                let l = self.get_int(component, "Length")?.unwrap_or(0);
                if is("Transition", self.class_of(component)?) {
                    length -= l;
                } else {
                    length += l;
                }
            }
            Ok(length)
        } else if is("NestedScope", segment_class) {
            // pyaaf2 computes each nested slot's length and discards it.
            Ok(0)
        } else {
            Ok(self.get_int(segment, "Length")?.unwrap_or(0))
        }
    }

    /// Adds a clip of one of a mob's slots: pyaaf2's
    /// `mob.create_source_clip(slot_id, start, length, media_kind)`.
    ///
    /// The clip refers to the mob and the slot, starts at `start` (zero if
    /// not given) and runs to the end of the slot unless given a length.
    /// Like pyaaf2's, the new clip is not put anywhere.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob has no such slot.
    pub fn create_mob_source_clip(
        &mut self,
        mob: ObjRef,
        slot_id: u32,
        start: Option<i64>,
        length: Option<i64>,
        media_kind: Option<&str>,
    ) -> Result<ObjRef> {
        let slot = self.slot_at(mob, slot_id)?;
        let media_kind = match media_kind {
            Some(kind) => kind.to_owned(),
            None => {
                let segment = self.required_segment(slot)?;
                self.media_kind(segment)?
                    .unwrap_or_else(|| "picture".to_owned())
            }
        };
        let clip = self.create_source_clip(0, 0, MobId::NIL, 0, &media_kind)?;
        self.set_source_mob(clip, mob)?;
        self.set_source_slot(clip, slot)?;
        let start = start.unwrap_or(0);
        self.set(clip, "StartTime", start)?;
        let length = match length.filter(|l| *l != 0) {
            Some(l) => l,
            None => (self.slot_length(slot)? - start).max(0),
        };
        self.set(clip, "Length", length)?;
        Ok(clip)
    }

    /// Points a source reference at a mob: pyaaf2's `clip.mob = mob`.
    ///
    /// # Errors
    ///
    /// Returns an error if the mob has no `MobID`.
    pub fn set_source_mob(&mut self, clip: ObjRef, mob: ObjRef) -> Result<()> {
        let mob_id = self
            .get_mob_id(mob, "MobID")?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(mob),
                property: "MobID".to_owned(),
            })?;
        self.set(clip, "SourceID", mob_id)
    }

    /// Points a source reference at a slot: pyaaf2's `clip.slot = slot`.
    ///
    /// # Errors
    ///
    /// Returns an error if the slot has no `SlotID`.
    pub fn set_source_slot(&mut self, clip: ObjRef, slot: ObjRef) -> Result<()> {
        let slot_id = self
            .get_int(slot, "SlotID")?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(slot),
                property: "SlotID".to_owned(),
            })?;
        self.set(clip, "SourceMobSlotID", slot_id)
    }

    // --- data definitions -------------------------------------------------

    /// Sets a component's or operation definition's data definition by
    /// name: pyaaf2's `obj.media_kind = 'picture'`.
    ///
    /// # Errors
    ///
    /// Returns an error if no data definition matches.
    pub fn set_media_kind(&mut self, obj: ObjRef, media_kind: &str) -> Result<()> {
        let datadef = self.lookup_datadef(media_kind)?;
        self.set(obj, "DataDefinition", datadef)
    }

    /// An object's data definition, abbreviated: pyaaf2's `obj.media_kind`.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no `DataDefinition`.
    pub fn media_kind(&self, obj: ObjRef) -> Result<Option<String>> {
        let Some(data) = self.get_bytes(obj, "DataDefinition")? else {
            return Ok(None);
        };
        let key = data.get(5..).unwrap_or_default();
        let dictionary = self.dictionary()?;
        for def in self.get_objects(dictionary, "DataDefinitions")? {
            if self.unique_key(def)? == key {
                return Ok(self.get_string(def, "Name")?.map(|n| short_name(&n)));
            }
        }
        Ok(None)
    }

    // --- the dictionary ---------------------------------------------------

    /// The dictionary pyaaf2's `Dictionary.__init__` makes, with the data
    /// and container definitions every new file starts with.
    pub(crate) fn create_dictionary(&mut self) -> Result<ObjRef> {
        let dictionary = self.instance("Dictionary")?;
        for def in raw::DATA_DEFS {
            let d = self.create_definition(
                "DataDef",
                Some(def.auid),
                Some(def.name),
                Some(def.description),
            )?;
            self.append(dictionary, "DataDefinitions", d)?;
        }
        for def in raw::CONTAINER_DEFS {
            let d = self.create_definition(
                "ContainerDef",
                Some(def.auid),
                Some(def.name),
                Some(def.description),
            )?;
            self.append(dictionary, "ContainerDefinitions", d)?;
        }
        Ok(dictionary)
    }

    /// pyaaf2's `Dictionary.setup_defaults`: the codec definitions.
    pub(crate) fn setup_defaults(&mut self, dictionary: ObjRef) -> Result<()> {
        for def in raw::CODEC_DEFS {
            let codec = self.create_definition(
                "CodecDef",
                Some(def.auid),
                Some(def.name),
                Some(def.description),
            )?;
            let class = self
                .lookup_classdef(def.file_descriptor_class)
                .ok_or_else(|| Error::UndefinedClass {
                    name: def.file_descriptor_class.to_owned(),
                })?;
            self.set(codec, "FileDescriptorClass", class)?;
            for name in def.data_definitions {
                let datadef = self.lookup_datadef(*name)?;
                self.append(codec, "DataDefinitions", datadef)?;
            }
            self.append(dictionary, "CodecDefinitions", codec)?;
        }
        Ok(())
    }

    /// A new definition: pyaaf2's `DefinitionObject.__init__(auid, name,
    /// description)`, for `DataDef`, `OperationDef`, `ParameterDef`,
    /// `InterpolationDef` and the rest. Put it in the dictionary with
    /// [`register_def`](Self::register_def).
    ///
    /// # Errors
    ///
    /// Returns an error if the class is not a concrete definition class.
    pub fn create_definition(
        &mut self,
        class: &str,
        id: Option<Auid>,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<ObjRef> {
        let def = self.instance(class)?;
        if let Some(name) = name {
            self.set(def, "Name", name)?;
        }
        if let Some(description) = description {
            self.set(def, "Description", description)?;
        }
        if let Some(id) = id {
            self.set(def, "Identification", id)?;
        }
        Ok(def)
    }

    /// A new parameter definition: pyaaf2's `f.create.ParameterDef(auid,
    /// name, description, typedef)`.
    ///
    /// # Errors
    ///
    /// Returns an error if `typedef` is not a type definition.
    pub fn create_parameter_def(
        &mut self,
        id: Auid,
        name: &str,
        description: &str,
        typedef: Option<ObjRef>,
    ) -> Result<ObjRef> {
        let def =
            self.create_definition("ParameterDef", Some(id), Some(name), Some(description))?;
        if let Some(typedef) = typedef {
            self.set(def, "Type", typedef)?;
        }
        Ok(def)
    }

    /// Puts a definition in the dictionary, in the collection for its kind:
    /// pyaaf2's `f.dictionary.register_def(def)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the object is not a definition the dictionary
    /// holds, or is already in the file.
    pub fn register_def(&mut self, def: ObjRef) -> Result<()> {
        let class = self.class_of(def)?;
        let collection = [
            ("DataDefinition", "DataDefinitions"),
            ("ContainerDefinition", "ContainerDefinitions"),
            ("CodecDefinition", "CodecDefinitions"),
            ("ParameterDefinition", "ParameterDefinitions"),
            ("OperationDefinition", "OperationDefinitions"),
            ("InterpolationDefinition", "InterpolationDefinitions"),
            ("TaggedValueDefinition", "TaggedValueDefinitions"),
        ]
        .into_iter()
        .find(|(kind, _)| {
            self.model
                .class_named(kind)
                .is_some_and(|c| self.model.derives_from(class, self.model.classes[c].auid))
        })
        .map(|(_, collection)| collection)
        .ok_or_else(|| Error::WrongClass {
            expected: "a definition the dictionary holds".to_owned(),
            found: self.model.classes[class].name.clone(),
        })?;
        let dictionary = self.dictionary()?;
        self.append(dictionary, collection, def)
    }

    /// Looks a definition up in one of the dictionary's collections, as
    /// pyaaf2's `lookup_def` does.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_def(&self, collection: &'static str, key: impl Into<DefKey>) -> Result<ObjRef> {
        let key = key.into();
        if let DefKey::Object(obj) = key {
            self.check(obj)?;
            return Ok(obj);
        }
        let dictionary = self.dictionary()?;
        let members = self.get_objects(dictionary, collection)?;
        match &key {
            DefKey::Auid(id) => {
                for def in members {
                    if self.unique_key(def)? == id.to_bytes_le() {
                        return Ok(def);
                    }
                }
            }
            DefKey::Name(name) => {
                let wanted = short_name(name).to_lowercase();
                for def in members {
                    let name = self.get_string(def, "Name")?.unwrap_or_default();
                    if short_name(&name).to_lowercase() == wanted {
                        return Ok(def);
                    }
                }
            }
            DefKey::Object(_) => unreachable!("handled above"),
        }
        Err(Error::NoSuchDefinition {
            kind: collection,
            name: match key {
                DefKey::Name(name) => name,
                DefKey::Auid(id) => id.to_string(),
                DefKey::Object(_) => String::new(),
            },
        })
    }

    /// pyaaf2's `f.dictionary.lookup_datadef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_datadef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("DataDefinitions", key)
    }

    /// pyaaf2's `f.dictionary.lookup_containerdef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_containerdef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("ContainerDefinitions", key)
    }

    /// pyaaf2's `f.dictionary.lookup_codecdef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_codecdef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("CodecDefinitions", key)
    }

    /// pyaaf2's `f.dictionary.lookup_operationdef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_operationdef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("OperationDefinitions", key)
    }

    /// pyaaf2's `f.dictionary.lookup_parameterdef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_parameterdef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("ParameterDefinitions", key)
    }

    /// pyaaf2's `f.dictionary.lookup_interperlationdef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_interpolationdef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("InterpolationDefinitions", key)
    }

    /// pyaaf2's `f.dictionary.lookup_taggedvaluedef(name)`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchDefinition`] if nothing matches.
    pub fn lookup_taggedvaluedef(&self, key: impl Into<DefKey>) -> Result<ObjRef> {
        self.lookup_def("TaggedValueDefinitions", key)
    }

    // --- the meta dictionary ----------------------------------------------

    /// A class definition, by name: pyaaf2's
    /// `f.metadict.lookup_classdef(name)`. Aliases work too.
    #[must_use]
    pub fn lookup_classdef(&self, name: &str) -> Option<ObjRef> {
        self.model
            .class_named(name)
            .map(|c| self.model.classes[c].obj)
    }

    /// A type definition, by name or identifier: pyaaf2's
    /// `f.metadict.lookup_typedef(name)`, which is what a parameter
    /// definition's `Type` refers to.
    #[must_use]
    pub fn lookup_typedef(&self, name: &str) -> Option<ObjRef> {
        if let Ok(id) = name.parse::<Auid>() {
            if let Some(t) = self.model.type_def(id) {
                return Some(t.obj);
            }
        }
        self.model.type_named(name).map(|t| t.obj)
    }

    /// Defines a new class: pyaaf2's `f.metadict.register_classdef(name,
    /// auid, parent, concrete)`.
    ///
    /// The class derives from the class named `parent` and has no
    /// properties of its own until [`register_propertydef`] gives it some.
    /// A class already defined under `name` is left as it is. Objects of the
    /// class are made with [`create`](Self::create).
    ///
    /// [`register_propertydef`]: Self::register_propertydef
    ///
    /// # Errors
    ///
    /// Returns an error if `parent` is not defined, or `id` is already the
    /// identifier of a class with another name.
    pub fn register_classdef(
        &mut self,
        name: &str,
        id: Auid,
        parent: &str,
        concrete: bool,
    ) -> Result<()> {
        let parent = self
            .model
            .class_named(parent)
            .ok_or_else(|| Error::UndefinedClass {
                name: parent.to_owned(),
            })?;
        let parent = self.model.classes[parent].auid;
        let index = self.find_or_create_classdef(name, id, Some(parent), concrete)?;
        self.enter_classdef(index, name)
    }

    /// Defines a new property of a class: pyaaf2's
    /// `classdef.register_propertydef(name, auid, pid, typedef, optional,
    /// unique)`. Returns the property's identifier.
    ///
    /// Without an identifier the property gets the next free one, counting
    /// down from `0xffff`. A property the class already defines is left as
    /// it is. As in pyaaf2, an identifier given here is not checked against
    /// the ones already handed out.
    ///
    /// # Errors
    ///
    /// Returns an error if the class or the type is not defined.
    #[allow(clippy::too_many_arguments)]
    pub fn register_propertydef(
        &mut self,
        class: &str,
        name: &str,
        id: Auid,
        pid: Option<u16>,
        type_id: Auid,
        optional: bool,
        unique: bool,
    ) -> Result<u16> {
        let class = self
            .model
            .class_named(class)
            .ok_or_else(|| Error::UndefinedClass {
                name: class.to_owned(),
            })?;
        if self.model.type_def(type_id).is_none() {
            return Err(Error::UndefinedType { type_id });
        }
        let index =
            self.register_propertydef_on(class, name, id, pid, type_id, optional, unique)?;
        Ok(self.model.props[index].pid)
    }

    // --- tagged values ----------------------------------------------------

    /// A new tagged value: pyaaf2's `f.create.TaggedValue(name, value)`.
    ///
    /// The value's type is inferred as pyaaf2 infers it — `aafString`,
    /// `aafInt32` or `Rational` — unless it is a
    /// [`WriteValue::Typed`].
    ///
    /// # Errors
    ///
    /// Returns an error if the value's type cannot be inferred.
    pub fn create_tagged_value(
        &mut self,
        name: Option<&str>,
        value: Option<WriteValue>,
    ) -> Result<ObjRef> {
        let tag = self.instance("TaggedValue")?;
        if let Some(name) = name {
            self.set(tag, "Name", name)?;
        }
        if let Some(value) = value {
            self.set(tag, "Value", value)?;
        }
        Ok(tag)
    }

    /// The tagged value named `key` in a list of them, if there is one.
    ///
    /// # Errors
    ///
    /// Returns an error if the class has no such property.
    pub fn tagged_value(&self, obj: ObjRef, property: &str, key: &str) -> Result<Option<ObjRef>> {
        for tag in self.get_objects(obj, property)? {
            if self.get_string(tag, "Name")?.as_deref() == Some(key) {
                return Ok(Some(tag));
            }
        }
        Ok(None)
    }

    /// Sets a tagged value in a list of them, adding one if the key is new:
    /// pyaaf2's `TaggedValueHelper(obj[property])[key] = value`, which is
    /// what `mob.comments[key] = value` does with `UserComments`.
    ///
    /// # Errors
    ///
    /// Returns an error if the property is not a list of tagged values, or
    /// the value's type cannot be inferred.
    pub fn set_tagged_value(
        &mut self,
        obj: ObjRef,
        property: &str,
        key: &str,
        value: impl Into<WriteValue>,
    ) -> Result<ObjRef> {
        let tag = match self.tagged_value(obj, property, key)? {
            Some(tag) => tag,
            None => {
                let tag = self.instance("TaggedValue")?;
                self.set(tag, "Name", key)?;
                self.append(obj, property, tag)?;
                tag
            }
        };
        self.set(tag, "Value", value)?;
        Ok(tag)
    }

    // --- parameters -------------------------------------------------------

    /// The type a parameter definition's values are.
    fn parameter_type(&self, parameterdef: ObjRef) -> Result<Auid> {
        let data = self
            .get_bytes(parameterdef, "Type")?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(parameterdef),
                property: "Type".to_owned(),
            })?;
        data.get(5..21)
            .and_then(|d| <[u8; 16]>::try_from(d).ok())
            .map(Auid::from_bytes_le)
            .ok_or(Error::BadKeySize {
                size: u8::try_from(data.len()).unwrap_or(u8::MAX),
            })
    }

    /// Makes a parameter refer to its definition: pyaaf2's
    /// `param.parameterdef = paramdef`.
    ///
    /// # Errors
    ///
    /// Returns an error if the definition has no identifier.
    pub fn set_parameter_def(&mut self, parameter: ObjRef, parameterdef: ObjRef) -> Result<()> {
        let id = self
            .get_auid(parameterdef, "Identification")?
            .ok_or_else(|| Error::MissingProperty {
                class: self.class_name_of(parameterdef),
                property: "Identification".to_owned(),
            })?;
        self.set(parameter, "Definition", id)
    }

    /// A new constant parameter: pyaaf2's
    /// `f.create.ConstantValue(parameterdef, value)`. The value is stored as
    /// the parameter definition's type.
    ///
    /// # Errors
    ///
    /// Returns an error if the parameter definition is not in the
    /// dictionary, or the value does not fit its type.
    pub fn create_constant_value(
        &mut self,
        parameterdef: impl Into<DefKey>,
        value: Option<WriteValue>,
    ) -> Result<ObjRef> {
        let parameter = self.instance("ConstantValue")?;
        let parameterdef = self.lookup_parameterdef(parameterdef)?;
        self.set_parameter_def(parameter, parameterdef)?;
        if let Some(value) = value {
            let type_id = self.parameter_type(parameterdef)?;
            self.set(parameter, "Value", WriteValue::typed(type_id, value))?;
        }
        Ok(parameter)
    }

    /// A new varying parameter: pyaaf2's
    /// `f.create.VaryingValue(parameterdef, interpolationdef)`. Add
    /// `ControlPoint`s to its `PointList`.
    ///
    /// # Errors
    ///
    /// Returns an error if either definition is not in the dictionary.
    pub fn create_varying_value(
        &mut self,
        parameterdef: Option<DefKey>,
        interpolation: Option<DefKey>,
    ) -> Result<ObjRef> {
        let parameter = self.instance("VaryingValue")?;
        if let Some(def) = parameterdef {
            let def = self.lookup_parameterdef(def)?;
            self.set_parameter_def(parameter, def)?;
        }
        if let Some(def) = interpolation {
            let def = self.lookup_interpolationdef(def)?;
            self.set(parameter, "Interpolation", def)?;
        }
        Ok(parameter)
    }
}

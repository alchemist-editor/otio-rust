//! The walk from AAF objects to OTIO objects.
//!
//! This is upstream's `_transcribe`, one branch per kind of AAF object, asked
//! in upstream's order. The order matters: AAF's class hierarchy is deeper
//! than OTIO's, so a `SourceClip` is also a `Segment` and also a
//! `Component`, and asking the questions in the wrong order answers the wrong
//! one.
//!
//! Where upstream does something surprising the surprise is kept, because a
//! file read here has to come out the same as one read there. Each is noted
//! where it happens.

use std::io::{Read, Seek};

use aaf::{MobId, Object};
use opentime::{RationalTime, TimeRange};
use otio_core::schema::{
    Base, EffectData, Gap, ItemData, Marker, SerializableCollection, Stack, Timeline, Track,
    Transition,
};
use otio_core::{Any, AnyDictionary, Color, Node, NodeId};

use crate::error::{Error, Result};
use crate::log::bytes_repr;
use crate::py::Py;
use crate::{Transcriber, markers};

/// How deep the walk goes before giving up.
///
/// A source clip is followed to the mob it names, which can name another, and
/// a file is free to describe a cycle. Real files nest a handful deep.
const MAX_DEPTH: usize = 64;

/// The edit rate in force, which a slot sets for everything under it.
pub(crate) type EditRate = Option<f64>;

/// The name upstream gives the collection a list of mobs becomes: the name of
/// the Python type that list was, since it has no name of its own.
pub(crate) const LIST_NAME: &str = "list";

/// A time in frames at a rate.
#[expect(
    clippy::cast_precision_loss,
    reason = "AAF times are frame counts, well inside f64"
)]
pub(crate) fn frames(value: i64, rate: f64) -> RationalTime {
    RationalTime::new(value as f64, rate)
}

/// The rate in force, or one frame a second where nothing set one.
pub(crate) fn rate_of(edit_rate: EditRate) -> f64 {
    edit_rate.unwrap_or(1.0)
}

/// OTIO's name for what a track carries, from AAF's.
///
/// OTIO names two kinds and AAF more than two. The rest keep AAF's name,
/// prefixed, and a component with no data definition at all comes out as
/// `AAF_None`, which is what upstream's f-string makes of Python's `None`.
pub(crate) fn track_kind(media_kind: Option<&str>) -> String {
    match media_kind {
        Some("Picture") => "Video".to_owned(),
        Some("SoundMasterTrack" | "Sound") => "Audio".to_owned(),
        Some(other) => format!("AAF_{other}"),
        None => "AAF_None".to_owned(),
    }
}

/// An `AAF` metadata dictionary wrapped as an object's whole metadata.
pub(crate) fn wrap(aaf: AnyDictionary) -> AnyDictionary {
    let mut metadata = AnyDictionary::new();
    metadata.insert("AAF".to_owned(), Any::Dictionary(aaf));
    metadata
}

/// Item fields holding a name and metadata and nothing else.
pub(crate) fn item(name: String, metadata: AnyDictionary) -> ItemData {
    ItemData {
        base: Base { name, metadata },
        ..ItemData::new()
    }
}

impl<R: Read + Seek> Transcriber<R> {
    /// A list of mobs, as the collection upstream makes of it.
    pub(crate) fn transcribe_mobs(&mut self, mobs: &[Object]) -> Result<NodeId> {
        self.log(|| {
            format!(
                "Creating SerializableCollection for Iterable for {}",
                bytes_repr(LIST_NAME)
            )
        });
        let mut children = Vec::new();
        for mob in mobs {
            if let Some(child) = self.nested(|s| s.transcribe(mob, &[], None))? {
                children.push(child);
            }
        }
        let mut aaf = AnyDictionary::new();
        aaf.insert("Name".to_owned(), Any::String(LIST_NAME.to_owned()));
        Ok(self
            .document
            .insert(Node::SerializableCollection(SerializableCollection {
                base: Base {
                    name: LIST_NAME.to_owned(),
                    metadata: wrap(aaf),
                },
                children,
            })))
    }

    /// Turns one AAF object into one OTIO object, or into nothing.
    ///
    /// `parents` is the chain from the mob down to this object's owner,
    /// which three of the branches need to know where they are.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, or if what it describes
    /// does not add up the way upstream checks it does.
    pub(crate) fn transcribe(
        &mut self,
        item: &Object,
        parents: &[Object],
        edit_rate: EditRate,
    ) -> Result<Option<NodeId>> {
        if parents.len() > MAX_DEPTH {
            return Ok(None);
        }

        // Upstream writes a name in before reading the object's properties,
        // so an object with no `Name` of its own still records one, and one
        // with a `Name` property records that instead.
        let name = self.get_name(item)?;
        // Upstream's `_encoded_name`, for the log.
        let label = || bytes_repr(&name);
        let mut metadata = AnyDictionary::new();
        metadata.insert("Name".to_owned(), Any::String(name.clone()));
        metadata.extend(self.object_properties(item)?);
        if self.has_media_kind(item) {
            let kind = self.media_kind(item)?;
            metadata.insert(
                "MediaKind".to_owned(),
                Any::String(kind.unwrap_or_else(|| "None".to_owned())),
            );
        }
        let edit_rate = if self.has_edit_rate(item) {
            self.edit_rate_of(item)?.or(edit_rate)
        } else {
            edit_rate
        };
        let length = if self.py_is(item, "Component") {
            let length = self.length(item)?;
            metadata.insert("Length".to_owned(), length.map_or(Any::Null, Any::Int));
            length
        } else {
            None
        };

        let chain: Vec<Object> = parents.iter().chain([item]).cloned().collect();
        let rate = rate_of(edit_rate);

        // The metadata the length is checked against at the end. Upstream
        // rebinds its `metadata` variable in two branches, and the check
        // reads whatever it was last bound to.
        let mut checked = Some(metadata.clone());

        let result = if self.py_is(item, "ContentStorage") {
            self.log(|| format!("Creating SerializableCollection for {}", label()));
            let mut children = Vec::new();
            for mob in self.aaf.mobs_of("CompositionMob")? {
                self.log(|| "compositionmob traversal".to_owned());
                if let Some(child) = self.nested(|s| s.transcribe(&mob, &chain, edit_rate))? {
                    children.push(child);
                }
            }
            Some(
                self.document
                    .insert(Node::SerializableCollection(SerializableCollection {
                        base: Base::default(),
                        children,
                    })),
            )
        } else if self.py_is(item, "SourceMob") {
            return Err(Error::UnexpectedSourceMob);
        } else if self.py_is(item, "MasterMob") {
            Some(self.transcribe_master_mob_cached(item, &chain, &metadata, &label())?)
        } else if self.py_is(item, "CompositionMob") {
            Some(self.transcribe_composition_mob(item, &chain, edit_rate, &label())?)
        } else if self.py_is(item, "SourceClip") {
            Some(self.transcribe_source_clip(item, rate, &mut checked, &label())?)
        } else if self.py_is(item, "Transition") {
            self.log(|| format!("Creating Transition for {}", label()));
            Some(self.transcribe_transition(item, &chain, edit_rate, rate, &mut metadata)?)
        } else if self.py_is(item, "Filler") || self.py_is(item, "ScopeReference") {
            if self.py_is(item, "Filler") {
                self.log(|| format!("Creating Gap for {}", label()));
            } else {
                self.log(|| format!("Creating Gap for ScopedReference for {}", label()));
            }
            let length = length.unwrap_or_default();
            let mut gap = item_fields();
            gap.source_range = Some(TimeRange::new(frames(0, rate), frames(length, rate)));
            Some(self.document.insert(Node::Gap(Gap { item: gap })))
        } else if self.py_is(item, "NestedScope") {
            self.log(|| format!("Creating Stack for NestedScope for {}", label()));
            let mut children = Vec::new();
            for slot in self.aaf.children(item, "Slots")? {
                if let Some(child) = self.nested(|s| s.transcribe(&slot, &chain, edit_rate))? {
                    children.push(child);
                }
            }
            Some(self.stack_of(item_fields(), children)?)
        } else if self.py_is(item, "Sequence") {
            self.log(|| format!("Creating Track for Sequence for {}", label()));
            Some(self.transcribe_sequence(item, parents, &chain, edit_rate, &mut metadata)?)
        } else if self.py_is(item, "OperationGroup") {
            self.log(|| format!("Creating operationGroup for {}", label()));
            let result =
                self.nested(|s| s.transcribe_operation_group(item, &chain, &metadata, edit_rate))?;
            // Upstream empties its metadata here, so the stack gets an empty
            // `AAF` dictionary and no length check.
            metadata = AnyDictionary::new();
            checked = Some(metadata.clone());
            if let Some(name) = self.first_clip_name(result)? {
                self.set_name(result, name);
            }
            Some(result)
        } else if self.py_is(item, "MobSlot") {
            // A timeline slot and any other slot become the same thing: a
            // track holding whatever the segment became.
            if self.py_is(item, "TimelineMobSlot") {
                self.log(|| format!("Creating Track for TimelineMobSlot for {}", label()));
            } else {
                self.log(|| format!("Creating Track for MobSlot for {}", label()));
            }
            let mut children = Vec::new();
            if let Py::Object(segment) = self.value_of(item, "Segment")? {
                if let Some(child) = self.nested(|s| s.transcribe(&segment, &chain, edit_rate))? {
                    children.push(child);
                }
            }
            Some(self.track_of(item_fields(), children, String::new())?)
        } else if self.py_is(item, "Timecode")
            || self.py_is(item, "Pulldown")
            || self.py_is(item, "EdgeCode")
        {
            None
        } else if self.py_is(item, "DescriptiveMarker") {
            self.transcribe_marker(parents, rate, &mut metadata, &label())?
        } else if self.py_is(item, "Selector") {
            self.log(|| format!("Transcribe selector for  {}", label()));
            Some(self.transcribe_selector(item, &chain, edit_rate)?)
        } else {
            None
        };

        let Some(result) = result else {
            return Ok(None);
        };
        self.finish(result, item, metadata, checked)?;
        Ok(Some(result))
    }

    /// What every transcribed object gets once its branch is done.
    ///
    /// The name falls back to the one in metadata, the metadata is attached
    /// unless the branch attached its own, an item's length is checked
    /// against its range, and a track takes its kind from the object.
    fn finish(
        &mut self,
        result: NodeId,
        item: &Object,
        metadata: AnyDictionary,
        checked: Option<AnyDictionary>,
    ) -> Result<()> {
        let name = match metadata.get("Name") {
            Some(Any::String(name)) => Some(name.clone()),
            _ => None,
        };
        if let Some(base) = self.document.get_mut(result).and_then(Node::base_mut) {
            if base.name.is_empty() {
                if let Some(name) = name {
                    base.name = name;
                }
            }
            if !base.metadata.contains_key("AAF") {
                base.metadata
                    .insert("AAF".to_owned(), Any::Dictionary(metadata));
            }
        }

        let range = self
            .document
            .get(result)
            .and_then(Node::item)
            .and_then(|item| item.source_range);
        let length = checked
            .as_ref()
            .and_then(|checked| match checked.get("Length") {
                Some(Any::Int(length)) => Some(*length),
                _ => None,
            });
        if let (Some(length), Some(range)) = (length, range) {
            #[expect(
                clippy::cast_precision_loss,
                reason = "an AAF length is a frame count, well inside f64"
            )]
            let expected = length as f64;
            if length != 0 && range.duration().value() != expected {
                return Err(Error::WrongDuration {
                    found: range.duration().value(),
                    expected: length,
                });
            }
        }

        if matches!(self.document.get(result), Some(Node::Track(_))) && self.has_media_kind(item) {
            let kind = track_kind(self.media_kind(item)?.as_deref());
            if let Some(Node::Track(track)) = self.document.get_mut(result) {
                track.kind = kind;
            }
        }
        Ok(())
    }

    /// A master mob, as a timeline, walked once however often it is named.
    fn transcribe_master_mob_cached(
        &mut self,
        mob: &Object,
        chain: &[Object],
        metadata: &AnyDictionary,
        label: &str,
    ) -> Result<NodeId> {
        let id = self.mob_id_of(mob)?;
        if let Some(found) = id.and_then(|id| self.timelines.get(&id)) {
            let found = *found;
            self.log(|| format!("Reusing Timeline for MasterMob for {label}"));
            return Ok(found);
        }
        self.log(|| format!("Creating Timeline for MasterMob for {label}"));
        let timeline = self.nested(|s| s.transcribe_master_mob(mob, chain, metadata))?;
        if let Some(id) = id {
            self.timelines.insert(id, timeline);
        }
        Ok(timeline)
    }

    /// A composition mob, as a timeline of its slots.
    fn transcribe_composition_mob(
        &mut self,
        mob: &Object,
        chain: &[Object],
        edit_rate: EditRate,
        label: &str,
    ) -> Result<NodeId> {
        let id = self.mob_id_of(mob)?;
        if let Some(found) = id.and_then(|id| self.timelines.get(&id)) {
            let found = *found;
            self.log(|| format!("Reusing Timeline for CompositionMob for {label}"));
            return Ok(found);
        }
        self.log(|| format!("Creating Timeline for CompositionMob for {label}"));
        let mut tracks = Vec::new();
        for slot in self.aaf.slots(mob)? {
            if let Some(track) = self.nested(|s| s.transcribe(&slot, chain, edit_rate))? {
                tracks.push(track);
            }
        }
        let global_start_time = self.mob_start_timecode(mob)?.map(|tc| tc.start_time());
        let timeline = self.timeline_of(String::new(), AnyDictionary::new(), tracks)?;
        if let Some(Node::Timeline(found)) = self.document.get_mut(timeline) {
            found.global_start_time = global_start_time;
        }
        if let Some(id) = id {
            self.timelines.insert(id, timeline);
        }
        Ok(timeline)
    }

    /// A timeline holding tracks, with OTIO's own name for its stack.
    pub(crate) fn timeline_of(
        &mut self,
        name: String,
        metadata: AnyDictionary,
        tracks: Vec<NodeId>,
    ) -> Result<NodeId> {
        let stack = self.stack_of(item(String::from("tracks"), AnyDictionary::new()), tracks)?;
        let timeline = self.document.insert(Node::Timeline(Timeline {
            base: Base { name, metadata },
            tracks: Some(stack),
            global_start_time: None,
        }));
        Ok(timeline)
    }

    /// A source clip, as whatever the slot it names turns out to be.
    ///
    /// One naming a master mob is that mob's track for the slot, trimmed to
    /// this clip; one naming a composition is that composition nested here
    /// as a stack; one naming anything else, or nothing the file holds, is a
    /// gap of the right length, because the edit still takes up the time.
    fn transcribe_source_clip(
        &mut self,
        item: &Object,
        rate: f64,
        checked: &mut Option<AnyDictionary>,
        label: &str,
    ) -> Result<NodeId> {
        let length = self.length(item)?.unwrap_or_default();
        let start = self
            .value_of(item, "StartTime")?
            .as_i64()
            .unwrap_or_default();
        let duration = frames(length, rate);
        let source_range = TimeRange::new(frames(start, rate), duration);
        let slot_id = self.value_of(item, "SourceMobSlotID")?.as_i64();

        let color = self.clip_color(item)?;

        let mob = self.mob_of(item)?;
        let mut slot_track = None;
        let mut mob_timeline = None;
        if let Some(mob) = &mob {
            if self.py_is(mob, "MasterMob") || self.py_is(mob, "CompositionMob") {
                let timeline = self
                    .nested(|s| s.transcribe(mob, &[], Some(rate)))?
                    .ok_or(Error::Malformed("a mob transcribed to nothing"))?;
                mob_timeline = Some(timeline);
                for track in self.timeline_tracks(timeline)? {
                    if self.meta_int(track, "SlotID") == slot_id {
                        slot_track = Some(self.document.deep_clone(track)?);
                        break;
                    }
                }
            }
        }

        let (Some(mob), Some(slot_track), Some(mob_timeline)) =
            (mob.clone(), slot_track, mob_timeline)
        else {
            if self.log.is_some() {
                let slot = slot_id.map_or_else(|| "None".to_owned(), |id| id.to_string());
                let mob = match &mob {
                    Some(mob) => self.mob_repr(mob)?,
                    None => "None".to_owned(),
                };
                let line = format!("Unable find slot_id: {slot} in mob {mob} creating Gap");
                self.log(|| line.clone());
                // Upstream also prints this one whether or not it logs.
                self.log_at(0, || line);
            }
            let mut gap = item_fields();
            gap.source_range = Some(TimeRange::new(frames(0, rate), duration));
            return Ok(self.document.insert(Node::Gap(Gap { item: gap })));
        };

        if self.py_is(&mob, "CompositionMob") {
            self.log(|| format!("Creating Stack for {label}"));
            let (name, mut aaf) = match self.document.get(mob_timeline) {
                Some(Node::Timeline(timeline)) => (
                    timeline.base.name.clone(),
                    match timeline.base.metadata.get("AAF") {
                        Some(Any::Dictionary(aaf)) => aaf.clone(),
                        _ => AnyDictionary::new(),
                    },
                ),
                _ => (String::new(), AnyDictionary::new()),
            };
            let kind = self.media_kind(item)?;
            aaf.insert(
                "MediaKind".to_owned(),
                Any::String(kind.unwrap_or_else(|| "None".to_owned())),
            );
            let mut fields = item_fields();
            fields.base.name = name;
            fields.base.metadata = wrap(aaf);
            fields.source_range = Some(source_range);
            let mut children = vec![slot_track];

            // Markers on the composition's other tracks that point at the
            // slot this clip uses come along on a track of their own.
            let track_number = self.meta_int(slot_track, "PhysicalTrackNumber");
            let mut marker_track: Option<NodeId> = None;
            for track in self.timeline_tracks(mob_timeline)? {
                if self.meta_int(track, "SlotID") == slot_id {
                    continue;
                }
                let mut sub_tracks = vec![track];
                sub_tracks.extend(
                    self.document.find_children(track, None, false, &|node| {
                        matches!(node, Node::Track(_))
                    })?,
                );
                for current in sub_tracks {
                    for marker in self.markers_of(current) {
                        let marker_aaf = self.aaf_of(marker);
                        let attached_slot = int_of(marker_aaf.get("AttachedSlotID"));
                        let attached_track = int_of(marker_aaf.get("AttachedPhysicalTrackNumber"));
                        // Upstream reuses its `metadata` name for each
                        // marker's, and checks the length against the last.
                        *checked = Some(marker_aaf);
                        if track_number == attached_track && slot_id == attached_slot {
                            let target = match marker_track {
                                Some(target) => target,
                                None => {
                                    let target = self.marker_track_for(track, slot_track)?;
                                    marker_track = Some(target);
                                    target
                                }
                            };
                            let copy = self.document.deep_clone(marker)?;
                            if let Some(item) =
                                self.document.get_mut(target).and_then(Node::item_mut)
                            {
                                item.markers.push(copy);
                            }
                        }
                    }
                }
            }
            if let Some(marker_track) = marker_track {
                children.push(marker_track);
            }
            return self.stack_of(fields, children);
        }

        // A master mob: the slot's track, trimmed to this clip.
        self.log(|| format!("Creating Track for {label}"));
        if let Some(item) = self.document.get_mut(slot_track).and_then(Node::item_mut) {
            item.source_range = Some(source_range);
        }
        if let Some(color) = color {
            let clips = self
                .document
                .find_children(slot_track, None, false, &|node| {
                    matches!(node, Node::Clip(_))
                })?;
            if let Some(clip) = clips.first() {
                if let Some(item) = self.document.get_mut(*clip).and_then(Node::item_mut) {
                    item.color = Some(color);
                }
            }
        }
        Ok(slot_track)
    }

    /// The track a nested composition's markers travel on: named and kinded
    /// like the track they came from, and as long as the clip's own.
    fn marker_track_for(&mut self, track: NodeId, slot_track: NodeId) -> Result<NodeId> {
        let (name, kind) = match self.document.get(track) {
            Some(Node::Track(track)) => (track.item.base.name.clone(), track.kind.clone()),
            _ => (String::new(), String::new()),
        };
        let range = self.document.available_range(slot_track)?;
        let mut gap = item_fields();
        gap.source_range = Some(range);
        let gap = self.document.insert(Node::Gap(Gap { item: gap }));
        let mut fields = item_fields();
        fields.base.name = name;
        self.track_of(fields, vec![gap], kind)
    }

    /// The colour a source clip carries in its attribute list, if it does.
    fn clip_color(&mut self, item: &Object) -> Result<Option<Color>> {
        let Py::List(attributes) = self.value_of(item, "ComponentAttributeList")? else {
            return Ok(None);
        };
        let mut channels = std::collections::HashMap::new();
        for attribute in attributes {
            let Py::Object(attribute) = attribute else {
                continue;
            };
            let Some(name) = self.py_name(&attribute)? else {
                continue;
            };
            // The first of each name wins, as `TaggedValueHelper` finds it.
            if let std::collections::hash_map::Entry::Vacant(slot) = channels.entry(name) {
                slot.insert(self.value_of(&attribute, "Value")?);
            }
        }
        if !channels.contains_key("_COLOR_R") {
            return Ok(None);
        }
        let channel = |name: &str| {
            channels
                .get(name)
                .and_then(Py::as_i64)
                .ok_or(Error::Malformed("a clip colour is missing a channel"))
        };
        let components = [
            channel("_COLOR_R")?,
            channel("_COLOR_G")?,
            channel("_COLOR_B")?,
            65535,
        ];
        let color = Color::from_int_list(&components, 16)?;
        Ok(Some(markers::named_color(color)))
    }

    /// A transition, with its operation's metadata and control points.
    fn transcribe_transition(
        &mut self,
        item: &Object,
        chain: &[Object],
        edit_rate: EditRate,
        rate: f64,
        metadata: &mut AnyDictionary,
    ) -> Result<NodeId> {
        let group = match self.value_of(item, "OperationGroup")? {
            Py::Object(group) => Some(group),
            _ => None,
        };
        if let Some(group) = &group {
            if let Some(transcribed) = self.nested(|s| s.transcribe(group, chain, edit_rate))? {
                let effect = self
                    .document
                    .get(transcribed)
                    .and_then(Node::item)
                    .and_then(|item| item.effects.first().copied());
                if let Some(effect) = effect {
                    let aaf = self.aaf_of(effect);
                    metadata.insert("OperationGroup".to_owned(), Any::Dictionary(aaf));
                }
            }
        }

        // The first varying parameter's control points, as upstream records
        // them for rebuilding the transition on the way back.
        let group = group.ok_or(Error::Malformed("a transition has no operation group"))?;
        let parameters = self.aaf.children(&group, "Parameters")?;
        if let Some(varying) = parameters
            .iter()
            .find(|parameter| self.py_is(parameter, "VaryingValue"))
            .cloned()
        {
            let mut points = Vec::new();
            for point in self.control_points(&varying)? {
                let value = self.value_of(&point, "Value")?.as_f64().unwrap_or_default();
                let time = self.value_of(&point, "Time")?.as_f64().unwrap_or_default();
                let mut entry = AnyDictionary::new();
                entry.insert("Value".to_owned(), Any::Double(value));
                entry.insert("Time".to_owned(), Any::Double(time));
                points.push(Any::Dictionary(entry));
            }
            if !points.is_empty() {
                metadata.insert("PointList".to_owned(), Any::Vector(points));
            }
        }

        let in_offset = match metadata.get("CutPoint") {
            Some(Any::Int(cut)) => *cut,
            _ => 0,
        };
        let out_offset = self.length(item)?.unwrap_or_default() - in_offset;
        Ok(self.document.insert(Node::Transition(Transition {
            base: Base::default(),
            parent: None,
            in_offset: frames(in_offset, rate),
            out_offset: frames(out_offset, rate),
            transition_type: "SMPTE_Dissolve".to_owned(),
            enabled: true,
        })))
    }

    /// A sequence, as a track of its components laid end to end.
    ///
    /// A sequence directly inside another sequence or a nested scope also
    /// records which slot it is under and, in a scope, which track of it it
    /// is, so that markers can find it later.
    fn transcribe_sequence(
        &mut self,
        item: &Object,
        parents: &[Object],
        chain: &[Object],
        edit_rate: EditRate,
        metadata: &mut AnyDictionary,
    ) -> Result<NodeId> {
        if let Some(parent) = parents.last() {
            if self.py_is(parent, "Sequence") || self.py_is(parent, "NestedScope") {
                let slot = parents
                    .iter()
                    .rev()
                    .find(|p| self.py_is(p, "TimelineMobSlot"))
                    .cloned()
                    .ok_or(Error::Malformed(
                        "a nested sequence is under no timeline slot",
                    ))?;
                if self.py_is(parent, "NestedScope") {
                    let slots = self.aaf.children(parent, "Slots")?;
                    let index = slots
                        .iter()
                        .position(|slot| slot.storage() == item.storage())
                        .ok_or(Error::Malformed("a scope does not hold its own slot"))?;
                    metadata.insert(
                        "PhysicalTrackNumber".to_owned(),
                        Any::Int(i64::try_from(index).unwrap_or(i64::MAX) + 1),
                    );
                }
                let slot_id = self.value_of(&slot, "SlotID")?.as_i64().unwrap_or_default();
                metadata.insert("SlotID".to_owned(), Any::Int(slot_id));
            }
        }

        let mut children = Vec::new();
        let mut markers = Vec::new();
        for component in self.aaf.components(item)? {
            let Some(child) = self.nested(|s| s.transcribe(&component, chain, edit_rate))? else {
                continue;
            };
            if matches!(self.document.get(child), Some(Node::Marker(_))) {
                markers.push(child);
            } else {
                children.push(child);
            }
        }
        let mut fields = item_fields();
        fields.markers = markers;
        self.track_of(fields, children, String::new())
    }

    /// A descriptive marker in an event slot, as a marker.
    ///
    /// Outside an event slot there is nothing to say where it points, and
    /// upstream drops it.
    fn transcribe_marker(
        &mut self,
        parents: &[Object],
        rate: f64,
        metadata: &mut AnyDictionary,
        label: &str,
    ) -> Result<Option<NodeId>> {
        let Some(event_slot) = parents
            .iter()
            .rev()
            .find(|p| self.py_is(p, "EventMobSlot"))
            .cloned()
        else {
            // Upstream leaves the indent off this one.
            self.log_at(0, || {
                format!("Cannot attach marker item '{label}'. Missing event mob in hierarchy.")
            });
            return Ok(None);
        };
        self.log(|| format!("Create marker for '{label}'"));
        let name = match metadata.get("Comment") {
            Some(Any::String(comment)) => comment.clone(),
            _ => return Err(Error::Malformed("a marker has no comment")),
        };
        let described = match metadata.get("DescribedSlots") {
            Some(Any::Vector(slots)) => slots.first().and_then(|slot| int_of(Some(slot))),
            _ => None,
        }
        .ok_or(Error::Malformed("a marker describes no slot"))?;
        metadata.insert("AttachedSlotID".to_owned(), Any::Int(described));
        let track_number = self
            .value_of(&event_slot, "PhysicalTrackNumber")?
            .as_i64()
            .ok_or(Error::Malformed("an event slot has no track number"))?;
        metadata.insert(
            "AttachedPhysicalTrackNumber".to_owned(),
            Any::Int(track_number),
        );

        let color = markers::marker_color(metadata);
        let position = int_of(metadata.get("Position")).unwrap_or_default();
        let length = int_of(metadata.get("Length")).unwrap_or(1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "AAF positions are frame counts, well inside f64"
        )]
        let marked_range = TimeRange::new(
            RationalTime::from_frames(position as f64, rate),
            RationalTime::from_frames(length as f64, rate),
        );
        Ok(Some(self.document.insert(Node::Marker(Marker {
            base: Base {
                name,
                metadata: AnyDictionary::new(),
            },
            color: Some(color),
            marked_range,
            comment: String::new(),
        }))))
    }

    /// A selector, as whichever of its choices is in use.
    ///
    /// A selected filler means the edit was muted: the one alternate stands
    /// in, disabled. Otherwise the selection is what plays, and the
    /// alternates are walked — upstream records them and then, because the
    /// result already carries metadata, drops the record.
    fn transcribe_selector(
        &mut self,
        item: &Object,
        chain: &[Object],
        edit_rate: EditRate,
    ) -> Result<NodeId> {
        let Py::Object(selected) = self.value_of(item, "Selected")? else {
            return Err(Error::Malformed("a selector selects nothing"));
        };
        let alternates: Vec<Object> = match self.value_of(item, "Alternates")? {
            Py::List(items) => items
                .into_iter()
                .filter_map(|item| match item {
                    Py::Object(object) => Some(object),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        if self.py_is(&selected, "Filler") || self.py_is(&selected, "ScopeReference") {
            let [alternate] = alternates.as_slice() else {
                return Err(Error::Malformed(
                    "a muted selector has other than one alternate",
                ));
            };
            let result = self
                .nested(|s| s.transcribe(alternate, chain, edit_rate))?
                .ok_or(Error::Malformed(
                    "a selector's alternate transcribed to nothing",
                ))?;
            if let Some(item) = self.document.get_mut(result).and_then(Node::item_mut) {
                item.enabled = false;
            }
            return Ok(result);
        }
        let result = self
            .nested(|s| s.transcribe(&selected, chain, edit_rate))?
            .ok_or(Error::Malformed(
                "a selector's choice transcribed to nothing",
            ))?;
        if matches!(self.document.get(result), Some(Node::Gap(_))) {
            return Err(Error::Malformed("a selector chose a gap"));
        }
        for alternate in &alternates {
            if let Some(walked) = self.nested(|s| s.transcribe(alternate, chain, edit_rate))? {
                self.document.remove_recursive(walked)?;
            }
        }
        Ok(result)
    }

    /// An operation group, as a stack of what it operates on with the effect
    /// on the stack.
    fn transcribe_operation_group(
        &mut self,
        item: &Object,
        chain: &[Object],
        metadata: &AnyDictionary,
        edit_rate: EditRate,
    ) -> Result<NodeId> {
        let mut metadata = metadata.clone();
        let rate = rate_of(edit_rate);
        let operation = match metadata.get("Operation") {
            Some(Any::Dictionary(operation)) => operation.clone(),
            _ => AnyDictionary::new(),
        };
        let operation_name = match operation.get("Name") {
            Some(Any::String(name)) => Some(name.clone()),
            _ => None,
        };
        let length = int_of(metadata.get("Length")).unwrap_or_default();

        // Upstream adds the effect's identifier to the parameters it read,
        // and it lands in the metadata only because that is the same
        // dictionary.
        if let Some(effect_id) = self.effect_id(item)? {
            if let Some(Any::Dictionary(parameters)) = metadata.get_mut("Parameters") {
                parameters.insert("AvidEffectID".to_owned(), Any::String(effect_id));
            }
        }
        let parameters = match metadata.get("Parameters") {
            Some(Any::Dictionary(parameters)) => parameters.clone(),
            _ => AnyDictionary::new(),
        };

        let is_time_warp = matches!(operation.get("IsTimeWarp"), Some(Any::Bool(true)));
        let mut effect = if is_time_warp {
            if operation_name.as_deref() == Some("Motion Control") {
                if self.parameter(item, "SpeedRatio")?.is_some() {
                    self.linear_time_warp(item, &parameters)?
                } else {
                    fancy_time_warp()
                }
            } else {
                Node::TimeEffect(EffectData::new())
            }
        } else {
            Node::Effect(EffectData::new())
        };

        if let Py::Object(rendering) = self.value_of(item, "Rendering")? {
            if let Some(rendered) = self.transcribe(&rendering, chain, edit_rate)? {
                metadata.insert("Rendering".to_owned(), Any::Object(rendered));
            }
        }

        if let Some(base) = effect.base_mut() {
            base.metadata = wrap(metadata);
            base.name = operation_name.clone().unwrap_or_default();
        }
        let effect = self.document.insert(effect);

        let mut children = Vec::new();
        for segment in self.aaf.children(item, "InputSegments")? {
            let Some(child) = self.transcribe(&segment, chain, edit_rate)? else {
                continue;
            };
            // An empty composition is false in Python, so upstream drops it.
            if self.is_empty_composition(child) {
                continue;
            }
            let child = if matches!(self.document.get(child), Some(Node::Track(_))) {
                child
            } else {
                let kind = track_kind(self.media_kind(&segment)?.as_deref());
                self.track_of(item_fields(), vec![child], kind)?
            };
            if matches!(self.document.get(child), Some(Node::Marker(_))) {
                continue;
            }
            children.push(child);
        }

        let mut fields = item_fields();
        fields.base.name = operation_name.unwrap_or_else(|| "OperationGroup".to_owned());
        fields.source_range = Some(TimeRange::new(frames(0, rate), frames(length, rate)));
        fields.effects.push(effect);
        self.stack_of(fields, children)
    }

    /// Whether a node is a composition with nothing in it.
    pub(crate) fn is_empty_composition(&self, id: NodeId) -> bool {
        match self.document.get(id) {
            Some(Node::Track(track)) => track.children.is_empty(),
            Some(Node::Stack(stack)) => stack.children.is_empty(),
            _ => false,
        }
    }

    /// A speed change, as upstream reads one.
    fn linear_time_warp(&mut self, item: &Object, parameters: &AnyDictionary) -> Result<Node> {
        let length = self.length(item)?.unwrap_or_default();
        let mut points = Vec::new();
        if let Some(offset_map) = self.parameter(item, "PARAM_SPEED_OFFSET_MAP_U")? {
            if self.py_is(&offset_map, "VaryingValue") {
                points = self.control_points(&offset_map)?;
            }
        }
        let time_scalar = match points.len() {
            n if n > 2 => return Ok(fancy_time_warp()),
            2 => {
                let value = |this: &mut Self, point: &Object| -> Result<f64> {
                    Ok(this.value_of(point, "Value")?.as_f64().unwrap_or_default())
                };
                let time = |this: &mut Self, point: &Object| -> Result<f64> {
                    Ok(this.value_of(point, "Time")?.as_f64().unwrap_or_default())
                };
                (value(self, &points[1])? - value(self, &points[0])?)
                    / (time(self, &points[1])? - time(self, &points[0])?)
            }
            _ => {
                let ratio = match parameters.get("SpeedRatio") {
                    Some(Any::String(ratio)) => ratio.clone(),
                    Some(Any::Int(ratio)) => ratio.to_string(),
                    _ => return Err(Error::Malformed("a speed change has no speed ratio")),
                };
                if ratio == length.to_string() {
                    0.0
                } else if let Some((numerator, denominator)) = ratio.split_once('/') {
                    let numerator: f64 = numerator
                        .parse()
                        .map_err(|_| Error::Malformed("a speed ratio is not a number"))?;
                    let denominator: f64 = denominator
                        .parse()
                        .map_err(|_| Error::Malformed("a speed ratio is not a number"))?;
                    denominator / numerator
                } else {
                    let ratio: f64 = ratio
                        .parse()
                        .map_err(|_| Error::Malformed("a speed ratio is not a number"))?;
                    1.0 / ratio
                }
            }
        };
        if time_scalar == 0.0 {
            let mut effect = EffectData::new();
            effect.effect_name = "FreezeFrame".to_owned();
            return Ok(Node::FreezeFrame {
                effect,
                time_scalar: 0.0,
            });
        }
        let mut effect = EffectData::new();
        effect.effect_name = "LinearTimeWarp".to_owned();
        Ok(Node::LinearTimeWarp {
            effect,
            time_scalar,
        })
    }

    /// An operation group's parameter by the name of its definition.
    ///
    /// Later parameters of the same name win, as they do in the dictionary
    /// upstream builds to look them up.
    fn parameter(&mut self, item: &Object, name: &str) -> Result<Option<Object>> {
        let mut found = None;
        for parameter in self.aaf.children(item, "Parameters")? {
            if self.py_name(&parameter)?.as_deref() == Some(name) {
                found = Some(parameter);
            }
        }
        Ok(found)
    }

    /// The identifier Avid gives an effect, if the group carries one.
    fn effect_id(&mut self, item: &Object) -> Result<Option<String>> {
        let Some(parameter) = self.parameter(item, "AvidEffectID")? else {
            return Ok(None);
        };
        if !self.py_is(&parameter, "ConstantValue") {
            return Ok(None);
        }
        let bytes = match self.value_of(&parameter, "Value")? {
            Py::List(items) | Py::Tuple(items) => items
                .iter()
                .map(|item| item.as_i64().and_then(|b| u8::try_from(b).ok()))
                .collect::<Option<Vec<u8>>>(),
            _ => None,
        };
        Ok(bytes
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .filter(|text| !text.is_empty()))
    }

    /// The name of the first named clip under a node, depth first.
    fn first_clip_name(&self, root: NodeId) -> Result<Option<String>> {
        let clips = self
            .document
            .find_children(root, None, false, &|node| matches!(node, Node::Clip(_)))?;
        for clip in clips {
            if let Some(Node::Clip(clip)) = self.document.get(clip) {
                if !clip.item.base.name.is_empty() {
                    return Ok(Some(clip.item.base.name.clone()));
                }
            }
        }
        Ok(None)
    }

    /// Renames a node.
    pub(crate) fn set_name(&mut self, id: NodeId, name: String) {
        if let Some(base) = self.document.get_mut(id).and_then(Node::base_mut) {
            base.name = name;
        }
    }

    /// A track of children, with the children's parent set.
    pub(crate) fn track_of(
        &mut self,
        mut fields: ItemData,
        children: Vec<NodeId>,
        kind: String,
    ) -> Result<NodeId> {
        fields.parent = None;
        let track = self.document.insert(Node::Track(Track {
            item: fields,
            children: Vec::new(),
            kind,
        }));
        for child in children {
            self.document.append_child(track, child)?;
        }
        Ok(track)
    }

    /// A stack of children, with the children's parent set.
    pub(crate) fn stack_of(
        &mut self,
        mut fields: ItemData,
        children: Vec<NodeId>,
    ) -> Result<NodeId> {
        fields.parent = None;
        let stack = self.document.insert(Node::Stack(Stack {
            item: fields,
            children: Vec::new(),
        }));
        for child in children {
            self.document.append_child(stack, child)?;
        }
        Ok(stack)
    }

    /// A timeline's tracks, in order.
    pub(crate) fn timeline_tracks(&self, timeline: NodeId) -> Result<Vec<NodeId>> {
        let Some(Node::Timeline(timeline)) = self.document.get(timeline) else {
            return Ok(Vec::new());
        };
        match timeline.tracks {
            Some(stack) => Ok(self.document.children_of(stack)?),
            None => Ok(Vec::new()),
        }
    }

    /// The markers on an item.
    pub(crate) fn markers_of(&self, id: NodeId) -> Vec<NodeId> {
        self.document
            .get(id)
            .and_then(Node::item)
            .map(|item| item.markers.clone())
            .unwrap_or_default()
    }

    /// An object's `AAF` metadata, or nothing.
    pub(crate) fn aaf_of(&self, id: NodeId) -> AnyDictionary {
        match self
            .document
            .get(id)
            .and_then(Node::base)
            .and_then(|base| base.metadata.get("AAF"))
        {
            Some(Any::Dictionary(aaf)) => aaf.clone(),
            _ => AnyDictionary::new(),
        }
    }

    /// An integer in an object's `AAF` metadata.
    pub(crate) fn meta_int(&self, id: NodeId, key: &str) -> Option<i64> {
        let base = self.document.get(id).and_then(Node::base)?;
        let Some(Any::Dictionary(aaf)) = base.metadata.get("AAF") else {
            return None;
        };
        int_of(aaf.get(key))
    }

    /// Upstream's `_get_name`.
    ///
    /// A source clip is called after the mob it names. Anything else is
    /// called by its `name`, or failing that its class.
    pub(crate) fn get_name(&mut self, item: &Object) -> Result<String> {
        if self.py_is(item, "SourceClip") {
            return Ok(match self.mob_of(item)? {
                None => "SourceClip Missing Mob".to_owned(),
                Some(mob) => match self.py_name(&mob)? {
                    Some(name) if !name.is_empty() => name,
                    _ => "Untitled SourceClip".to_owned(),
                },
            });
        }
        if let Some(name) = self.py_name(item)? {
            if !name.is_empty() {
                return Ok(name);
            }
        }
        Ok(self.py_class(item).to_owned())
    }

    /// pyaaf2's `repr` of a mob, for the log, without the memory address
    /// Python ends it with.
    fn mob_repr(&mut self, mob: &Object) -> Result<String> {
        let name = self.py_name(mob)?.unwrap_or_default();
        let id = self
            .mob_id_of(mob)?
            .map_or_else(|| "None".to_owned(), |id| id.to_string());
        Ok(format!(
            "<aaf2.mobs.{} \"{name}\" {id}>",
            self.py_class(mob)
        ))
    }

    /// The mob a source clip names, if the file holds it.
    pub(crate) fn mob_of(&mut self, clip: &Object) -> Result<Option<Object>> {
        match self.value_of(clip, "SourceID")? {
            Py::MobId(id) if id != MobId::from_bytes([0; 32]) => Ok(self.aaf.mob(id)?),
            _ => Ok(None),
        }
    }

    /// A mob's identifier.
    pub(crate) fn mob_id_of(&mut self, mob: &Object) -> Result<Option<MobId>> {
        Ok(match self.value_of(mob, "MobID")? {
            Py::MobId(id) => Some(id),
            _ => None,
        })
    }

    /// Whether pyaaf2's object has a `media_kind`: components and slots do.
    pub(crate) fn has_media_kind(&self, item: &Object) -> bool {
        self.py_is(item, "Component") || self.py_is(item, "MobSlot")
    }

    /// Whether pyaaf2's object has an `edit_rate`: timeline and event slots.
    fn has_edit_rate(&self, item: &Object) -> bool {
        self.py_is(item, "TimelineMobSlot") || self.py_is(item, "EventMobSlot")
    }

    /// What an object carries, in AAF's vocabulary.
    ///
    /// A component says so through its data definition, and a slot through
    /// its segment's.
    pub(crate) fn media_kind(&mut self, item: &Object) -> Result<Option<String>> {
        if self.py_is(item, "MobSlot") {
            return match self.value_of(item, "Segment")? {
                Py::Object(segment) => self.media_kind(&segment),
                _ => Ok(None),
            };
        }
        let Py::Object(datadef) = self.value_of(item, "DataDefinition")? else {
            return Ok(None);
        };
        Ok(match self.value_of(&datadef, "Name")? {
            Py::Str(name) if !name.is_empty() => {
                Some(name.replace("DataDef_", "").replace("ContainerDef_", ""))
            }
            _ => None,
        })
    }

    /// A component's length, if it has one.
    pub(crate) fn length(&mut self, item: &Object) -> Result<Option<i64>> {
        Ok(self.value_of(item, "Length")?.as_i64())
    }

    /// A slot's edit rate as a number.
    pub(crate) fn edit_rate_of(&mut self, slot: &Object) -> Result<Option<f64>> {
        Ok(self.value_of(slot, "EditRate")?.as_f64())
    }

    /// Where a mob's timecode says it starts, and how long it runs.
    ///
    /// A mob can carry several timecode slots; the one on physical track 1
    /// is the one that means the mob's own start.
    pub(crate) fn mob_start_timecode(&mut self, mob: &Object) -> Result<Option<TimeRange>> {
        for slot in self.aaf.slots(mob)? {
            let Some(timecode) = self
                .slot_components(&slot)?
                .into_iter()
                .find(|component| self.py_is(component, "Timecode"))
            else {
                continue;
            };
            if self.value_of(&slot, "PhysicalTrackNumber")?.as_i64() != Some(1) {
                continue;
            }
            let rate = self.edit_rate_of(&slot)?.unwrap_or(1.0);
            let start = self
                .value_of(&timecode, "Start")?
                .as_i64()
                .unwrap_or_default();
            let length = self.length(&timecode)?.unwrap_or_default();
            return Ok(Some(TimeRange::new(
                frames(start, rate),
                frames(length, rate),
            )));
        }
        Ok(None)
    }

    /// A slot's components: a sequence's, or the segment itself.
    pub(crate) fn slot_components(&mut self, slot: &Object) -> Result<Vec<Object>> {
        let Py::Object(segment) = self.value_of(slot, "Segment")? else {
            return Ok(Vec::new());
        };
        if self.py_is(&segment, "Sequence") {
            return Ok(self.aaf.components(&segment)?);
        }
        Ok(vec![segment])
    }
}

/// Fresh item fields.
pub(crate) fn item_fields() -> ItemData {
    ItemData::new()
}

/// A time effect upstream cannot describe.
fn fancy_time_warp() -> Node {
    let mut effect = EffectData::new();
    effect.effect_name = "Unknown Time Warp Effect".to_owned();
    Node::TimeEffect(effect)
}

/// An integer, if metadata holds one there.
pub(crate) fn int_of(value: Option<&Any>) -> Option<i64> {
    match value? {
        Any::Int(v) => Some(*v),
        Any::UInt(v) => i64::try_from(*v).ok(),
        Any::Bool(v) => Some(i64::from(*v)),
        _ => None,
    }
}

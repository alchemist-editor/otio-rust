//! Upstream's `_TrackTranscriber` and its video and audio subclasses: what
//! each item on a track becomes.
//!
//! A track becomes a timeline slot on the composition holding a sequence;
//! for sound the sequence sits inside a mono pan operation. Each clip
//! becomes a source clip of a master mob, which points at a file source mob
//! describing the media, which points at a tape source mob carrying its
//! timecode, the three-mob chain Media Composer expects. Gaps become
//! fillers, dissolves transitions, a nested track a sequence and a stack an
//! operation group.

use aaf::write::ObjRef;
use otio_core::{Any, Node, NodeId, TRACK_KIND_AUDIO, TRACK_KIND_VIDEO};

use super::py::{self, aaf_metadata};
use super::{
    Fail, FileTranscriber, find_children, is_considered_gap, metadata_of, rational, unsupported,
    unwritable,
};

pub(crate) const PARAMETERDEF_PAN: &str = "e4962322-2267-11d3-8a4c-0050040ef7d2";
pub(crate) const OPERATIONDEF_MONOAUDIOPAN: &str = "9d2ea893-0968-11d3-8a38-0050040ef7d2";
pub(crate) const PARAMETERDEF_AVIDPARAMETERBYTEORDER: &str = "c0038672-a8cf-11d3-a05b-006094eb75cb";
pub(crate) const PARAMETERDEF_AVIDEFFECTID: &str = "93994bd6-a81d-11d3-a05b-006094eb75cb";
pub(crate) const PARAMETERDEF_AFX_FG_KEY_OPACITY_U: &str = "8d56813d-847e-11d5-935a-50f857c10000";
pub(crate) const PARAMETERDEF_LEVEL: &str = "e4962320-2267-11d3-8a4c-0050040ef7d2";
pub(crate) const VVAL_EXTRAPOLATION_ID: &str = "0e24dd54-66cd-4f1a-b0a0-670ac3a7a0b3";
pub(crate) const OPERATIONDEF_SUBMASTER: &str = "f1db0f3d-8d64-11d3-80df-006008143e6f";
/// pyaaf2's `aaf2.misc.LinearInterp`.
pub(crate) const LINEAR_INTERP: &str = "5b6c85a4-0ede-11d3-80a9-006008143e6f";

/// Which of upstream's two track transcribers a track gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Video,
    Audio,
}

impl Kind {
    /// `media_kind`.
    pub(crate) const fn media_kind(self) -> &'static str {
        match self {
            Self::Video => "picture",
            Self::Audio => "sound",
        }
    }

    /// `_master_mob_slot_id`: picture in slot 1 of a master mob, sound in
    /// slot 2, so that clips sharing a master mob never share a slot. Upstream
    /// calls this a little inadequate, and it is kept as it is.
    pub(crate) const fn master_mob_slot_id(self) -> u32 {
        match self {
            Self::Video => 1,
            Self::Audio => 2,
        }
    }
}

/// A track being transcribed: `_TrackTranscriber`'s state.
pub(crate) struct Track {
    pub(crate) id: NodeId,
    pub(crate) kind: Kind,
    /// The rate of the first thing on the track, which every slot and mob
    /// made for the track is given.
    pub(crate) edit_rate: f64,
    /// The composition's slot for the track.
    pub(crate) timeline_mobslot: ObjRef,
    /// The sequence the track's items go in.
    pub(crate) sequence: ObjRef,
}

/// A track's kind: `otio_track.kind`, which only a track has.
fn track_kind(node: &Node) -> Result<&str, Fail> {
    match node {
        Node::Track(track) => Ok(&track.kind),
        other => Err(unwritable(format!(
            "a {} has no kind, where a track was expected",
            other.schema_name()
        ))),
    }
}

impl FileTranscriber<'_> {
    /// `track_transcriber` and `_TrackTranscriber.__init__`.
    pub(crate) fn track_transcriber(&mut self, track: NodeId) -> Result<Track, Fail> {
        let kind = match track_kind(self.document.try_get(track)?)? {
            TRACK_KIND_VIDEO => Kind::Video,
            TRACK_KIND_AUDIO => Kind::Audio,
            other => {
                return Err(unsupported(format!("Unsupported track kind: {other}")));
            }
        };
        let first = *find_children(self.document, track)?
            .first()
            .ok_or_else(|| unwritable("a track with nothing on it".to_owned()))?;
        let edit_rate = self.document.duration(first)?.rate();

        let (timeline_mobslot, sequence) = match kind {
            Kind::Video => self.create_video_mobslot(edit_rate)?,
            Kind::Audio => self.create_audio_mobslot(track, edit_rate)?,
        };
        let t = Track {
            id: track,
            kind,
            edit_rate,
            timeline_mobslot,
            sequence,
        };
        self.f.set(
            timeline_mobslot,
            "SlotName",
            self.document.try_get(track)?.name(),
        )?;
        let number = self.physical_track_number(&t)?;
        self.f
            .set(timeline_mobslot, "PhysicalTrackNumber", number)?;
        Ok(t)
    }

    /// `_aaf_physical_track_number`: where the track comes among the tracks
    /// of its kind, from one.
    ///
    /// Markers find their track again on reading by this number, so the
    /// event slot holding a track's markers carries it too.
    pub(crate) fn physical_track_number(&self, t: &Track) -> Result<i64, Fail> {
        let parent = self.document.parent_of(t.id)?;
        let mut number = 0;
        for track in self.document.children_of(parent)? {
            if track_kind(self.document.try_get(track)?)? == t.kind_name() {
                number += 1;
                if track == t.id {
                    return Ok(number);
                }
            }
        }
        Err(unwritable("a track that is not in its parent".to_owned()))
    }

    /// `VideoTrackTranscriber._create_timeline_mobslot`: a timeline slot on
    /// the composition, holding a sequence.
    fn create_video_mobslot(&mut self, edit_rate: f64) -> Result<(ObjRef, ObjRef), Fail> {
        let slot = self
            .f
            .create_timeline_slot(self.composition_mob, rational(edit_rate)?, None)?;
        let sequence = self.f.create_sequence(Kind::Video.media_kind())?;
        self.f.set(sequence, "Components", Vec::<ObjRef>::new())?;
        self.f.set(slot, "Segment", sequence)?;
        Ok((slot, sequence))
    }

    /// `AudioTrackTranscriber._create_timeline_mobslot`: a sound slot on the
    /// composition, holding a mono pan operation around a sequence.
    fn create_audio_mobslot(
        &mut self,
        track: NodeId,
        edit_rate: f64,
    ) -> Result<(ObjRef, ObjRef), Fail> {
        let media_kind = Kind::Audio.media_kind();
        let slot = self
            .f
            .create_sound_slot(self.composition_mob, rational(edit_rate)?)?;
        let opdef = self.f.create_definition(
            "OperationDef",
            Some(crate::py::auid(OPERATIONDEF_MONOAUDIOPAN)),
            Some("Audio Pan"),
            None,
        )?;
        self.f.set_media_kind(opdef, media_kind)?;
        self.f.set(opdef, "NumberInputs", 1)?;
        self.f.register_def(opdef)?;

        // Transitions count here, at their full length.
        let mut total = 0.0;
        for child in self.document.children_of(track)? {
            total += self.document.duration(child)?.value();
        }
        let total_length = py::float_int(total)?;

        let opgroup = self.f.create_operation_group(opdef, 0, None)?;
        self.f.set_media_kind(opgroup, media_kind)?;
        self.f.set(opgroup, "Length", total_length)?;
        self.f.set(slot, "Segment", opgroup)?;
        let sequence = self.f.create_sequence(media_kind)?;
        self.f.set(sequence, "Components", Vec::<ObjRef>::new())?;
        self.f.set(sequence, "Length", total_length)?;
        self.f.append(opgroup, "InputSegments", sequence)?;
        Ok((slot, sequence))
    }

    /// `transcribe`: the AAF component for one item, or `None` for a
    /// transition the writer skips.
    pub(crate) fn transcribe(&mut self, t: &Track, child: NodeId) -> Result<Option<ObjRef>, Fail> {
        if is_considered_gap(self.document, child)? {
            return self.aaf_filler(t, child).map(Some);
        }
        match self.document.try_get(child)? {
            Node::Transition(_) => self.aaf_transition(t, child),
            Node::Clip(_) => self.aaf_sourceclip(t, child).map(Some),
            Node::Track(_) => self.aaf_sequence(t, child).map(Some),
            Node::Stack(_) => self.aaf_operation_group(t, child).map(Some),
            other => Err(unsupported(format!(
                "Unsupported otio child type: {}",
                other.schema_name()
            ))),
        }
    }

    /// `aaf_filler`.
    fn aaf_filler(&mut self, t: &Track, gap: NodeId) -> Result<ObjRef, Fail> {
        let length = py::float_int(self.document.visible_range(gap)?.duration().value())?;
        Ok(self.f.create_filler(t.kind.media_kind(), length)?)
    }

    /// `aaf_sourceclip`, with `AudioTrackTranscriber`'s pan in front of it
    /// for sound.
    fn aaf_sourceclip(&mut self, t: &Track, clip: NodeId) -> Result<ObjRef, Fail> {
        if t.kind == Kind::Audio {
            self.audio_pan(t, clip)?;
        }

        // Embedded media gives the clip its master mob; otherwise it is made
        // behind a file mob and a tape mob that only point at the media.
        let embedded = if self.options.embed_essence {
            self.embedded_mastermob(t, clip)?
        } else {
            None
        };
        let (mastermob, mastermob_slot) = if let Some(embedded) = embedded {
            embedded
        } else {
            let (tapemob, tapemob_slot) = self.create_tapemob(t, clip)?;
            let (filemob, filemob_slot) = self.create_filemob(t, clip, tapemob, tapemob_slot)?;
            self.create_mastermob(t, clip, filemob, filemob_slot)?
        };

        // The start is the offset of what is seen into what is available.
        let visible = self.document.visible_range(clip)?;
        let available = self.document.available_range(clip)?;
        let offset = visible.start_time() - available.start_time();
        let start = py::float_int(offset.value())?;
        let length = py::float_int(visible.duration().value())?;

        let slot_id = self.slot_id(t.timeline_mobslot)?;
        let compmob_clip = self.f.create_mob_source_clip(
            self.composition_mob,
            slot_id,
            Some(start),
            Some(length),
            Some(t.kind.media_kind()),
        )?;
        self.f.set_source_mob(compmob_clip, mastermob)?;
        self.f.set_source_slot(compmob_clip, mastermob_slot)?;
        let mastermob_slot_id = self.slot_id(mastermob_slot)?;
        self.f
            .set(compmob_clip, "SourceMobSlotID", mastermob_slot_id)?;

        // The clip's colour, as 16-bit channels in the clip's attributes.
        let color = self
            .document
            .try_get(clip)?
            .item()
            .and_then(|item| item.color.clone());
        if let Some(color) = color {
            let [red, green, blue, _] = color.to_rgba_int_list(16);
            for (key, value) in [("_COLOR_R", red), ("_COLOR_G", green), ("_COLOR_B", blue)] {
                self.f
                    .set_tagged_value(compmob_clip, "ComponentAttributeList", key, value)?;
            }
        }

        // Avid's Frame Count Start and End.
        if self.options.create_edgecode {
            let ec_slot = self.create_edgecode_timeline_slot(
                t.edit_rate,
                py::float_int(available.start_time().value())?,
                py::float_int(available.duration().value())?,
            )?;
            self.f.append(mastermob, "Slots", ec_slot)?;
        }

        // Mark in and out when the clip uses less than all of its media.
        if visible != available {
            self.f.set(
                mastermob_slot,
                "MarkIn",
                py::float_int(visible.start_time().value())?,
            )?;
            self.f.set(
                mastermob_slot,
                "MarkOut",
                py::float_int(visible.end_time_exclusive().value())?,
            )?;
        }
        Ok(compmob_clip)
    }

    /// The start of `AudioTrackTranscriber.aaf_sourceclip`: a pan parameter,
    /// keyframed from the clip's metadata or centred, on the track's pan
    /// operation.
    ///
    /// The parameter replaces the one the previous clip put there, since an
    /// operation group files its parameters by definition; upstream writes it
    /// per clip all the same, and so does this.
    fn audio_pan(&mut self, t: &Track, clip: NodeId) -> Result<(), Fail> {
        let rational_type = self.f.lookup_typedef("Rational");
        let param_def = self.f.create_parameter_def(
            crate::py::auid(PARAMETERDEF_PAN),
            "Pan",
            "Pan",
            rational_type,
        )?;
        self.f.register_def(param_def)?;
        let interp_def = self.f.create_definition(
            "InterpolationDef",
            Some(crate::py::auid(LINEAR_INTERP)),
            Some("LinearInterp"),
            Some("LinearInterp"),
        )?;
        self.f.register_def(interp_def)?;

        let varying_value = self.f.create_varying_value(None, None)?;
        self.f.set_parameter_def(varying_value, param_def)?;
        self.f.set(varying_value, "Interpolation", interp_def)?;

        let length = py::float_int(self.document.duration(clip)?.value())?;
        // Mid pan unless the clip says otherwise.
        let default_points = Any::Vector(
            [format!("0/{length}"), format!("{}/{length}", length - 1)]
                .into_iter()
                .map(|time| {
                    Any::Dictionary(
                        [
                            ("ControlPointSource".to_owned(), Any::Int(2)),
                            ("Time".to_owned(), Any::String(time)),
                            ("Value".to_owned(), Any::String("1/2".to_owned())),
                        ]
                        .into_iter()
                        .collect(),
                    )
                })
                .collect(),
        );
        let pan = aaf_metadata(metadata_of(self.document, clip)?)?.get_dict("Pan")?;
        let points = pan.get("ControlPoints").cloned().unwrap_or(default_points);

        for cp in py::py_iter(&points)? {
            let get = |key: &str| -> Result<Any, Fail> {
                match &cp {
                    Any::Dictionary(d) => d
                        .get(key)
                        .cloned()
                        .ok_or_else(|| unwritable(format!("a control point without '{key}'"))),
                    other => Err(unwritable(format!(
                        "a {} where a control point was expected",
                        other.type_name()
                    ))),
                }
            };
            let point = self.f.create("ControlPoint")?;
            self.f.set(point, "Time", py::py_rational(&get("Time")?)?)?;
            self.f
                .set(point, "Value", py::py_rational(&get("Value")?)?)?;
            self.set_py(point, "ControlPointSource", &get("ControlPointSource")?)?;
            self.f.append(varying_value, "PointList", point)?;
        }

        let opgroup = self.segment(t.timeline_mobslot)?;
        self.f.append(opgroup, "Parameters", varying_value)?;
        Ok(())
    }

    /// `obj[name].value = value`, for a value from metadata: set as pyaaf2
    /// would encode it, or removed for `None`.
    pub(crate) fn set_py(&mut self, obj: ObjRef, name: &str, value: &Any) -> Result<(), Fail> {
        match py::to_write_value(value)? {
            Some(value) => self.f.set(obj, name, value)?,
            None => {
                if self.f.has(obj, name) {
                    self.f.remove(obj, name)?;
                } else {
                    // `del` of a property that is not there does nothing,
                    // but the property has to exist.
                    self.f.property_type_name(obj, name)?;
                }
            }
        }
        Ok(())
    }

    /// `_create_tapemob`: the clip's tape mob, and a new slot on it for this
    /// use of it.
    fn create_tapemob(&mut self, t: &Track, clip: NodeId) -> Result<(ObjRef, ObjRef), Fail> {
        let tapemob = self.unique_tapemob(clip)?;
        let tapemob_slot = self.f.create_empty_slot(
            tapemob,
            rational(t.edit_rate)?,
            Some(t.kind.media_kind()),
            None,
        )?;
        let available = self.media_available_range(clip)?;
        let segment = self.segment(tapemob_slot)?;
        self.f.set(
            segment,
            "Length",
            py::float_int(available.duration().value())?,
        )?;
        self.f.set(
            segment,
            "StartTime",
            py::float_int(available.start_time().value())?,
        )?;
        Ok((tapemob, tapemob_slot))
    }

    /// `_create_filemob`: a file source mob describing the clip's media, one
    /// per use, pointing at the tape.
    fn create_filemob(
        &mut self,
        t: &Track,
        clip: NodeId,
        tapemob: ObjRef,
        tapemob_slot: ObjRef,
    ) -> Result<(ObjRef, ObjRef), Fail> {
        let filemob = self.f.create("SourceMob")?;
        self.f.add_mob(filemob)?;

        let descriptor = match t.kind {
            Kind::Video => self.video_descriptor(clip)?,
            Kind::Audio => self.audio_descriptor(clip)?,
        };
        self.f.set(filemob, "EssenceDescription", descriptor)?;
        let filemob_slot = self
            .f
            .create_timeline_slot(filemob, rational(t.edit_rate)?, None)?;
        let tape_segment = self.segment(tapemob_slot)?;
        let length = self.length(tape_segment)?;
        let media_kind = self
            .f
            .media_kind(tape_segment)?
            .unwrap_or_else(|| "picture".to_owned());
        let filemob_slot_id = self.slot_id(filemob_slot)?;
        let filemob_clip = self.f.create_mob_source_clip(
            filemob,
            filemob_slot_id,
            None,
            Some(length),
            Some(&media_kind),
        )?;
        self.f.set_source_mob(filemob_clip, tapemob)?;
        self.f.set_source_slot(filemob_clip, tapemob_slot)?;
        let tapemob_slot_id = self.slot_id(tapemob_slot)?;
        self.f
            .set(filemob_clip, "SourceMobSlotID", tapemob_slot_id)?;
        self.f.set(filemob_slot, "Segment", filemob_clip)?;
        Ok((filemob, filemob_slot))
    }

    /// `_create_mastermob`: the clip's master mob, and its slot for this
    /// track's kind, now pointing at the file mob.
    fn create_mastermob(
        &mut self,
        t: &Track,
        clip: NodeId,
        filemob: ObjRef,
        filemob_slot: ObjRef,
    ) -> Result<(ObjRef, ObjRef), Fail> {
        let mastermob = self.unique_mastermob(clip)?;
        let timecode_length = py::float_int(self.media_available_range(clip)?.duration().value())?;

        let slot_id = t.kind.master_mob_slot_id();
        let mut existing = None;
        for slot in self.f.get_objects(mastermob, "Slots")? {
            if self.f.get_int(slot, "SlotID")? == Some(i64::from(slot_id)) {
                existing = Some(slot);
                break;
            }
        }
        let mastermob_slot = match existing {
            Some(slot) => slot,
            None => {
                self.f
                    .create_timeline_slot(mastermob, rational(t.edit_rate)?, Some(slot_id))?
            }
        };
        let mastermob_slot_id = self.slot_id(mastermob_slot)?;
        let mastermob_clip = self.f.create_mob_source_clip(
            mastermob,
            mastermob_slot_id,
            None,
            Some(timecode_length),
            Some(t.kind.media_kind()),
        )?;
        self.f.set_source_mob(mastermob_clip, filemob)?;
        self.f.set_source_slot(mastermob_clip, filemob_slot)?;
        let filemob_slot_id = self.slot_id(filemob_slot)?;
        self.f
            .set(mastermob_clip, "SourceMobSlotID", filemob_slot_id)?;
        self.f.set(mastermob_slot, "Segment", mastermob_clip)?;
        Ok((mastermob, mastermob_slot))
    }

    /// `_create_edgecode_timeline_slot`: slot 20, holding edge code, which
    /// is how Media Composer is told a clip's Frame Count Start and End.
    fn create_edgecode_timeline_slot(
        &mut self,
        edit_rate: f64,
        start: i64,
        length: i64,
    ) -> Result<ObjRef, Fail> {
        let edgecode = self.f.create("EdgeCode")?;
        self.f.set_media_kind(edgecode, "Edgecode")?;
        self.f.set(edgecode, "Start", start)?;
        self.f.set(edgecode, "Length", length)?;
        self.f.set(edgecode, "AvEdgeType", 3)?;
        self.f.set(edgecode, "AvFilmType", 0)?;
        self.f.set(edgecode, "FilmKind", "Ft35MM")?;
        self.f.set(edgecode, "CodeFormat", "EtNull")?;

        let slot =
            self.f
                .create_timeline_mob_slot(Some(20), None, None, 0, rational(edit_rate)?)?;
        self.f.set(slot, "SlotName", "EC1")?;
        self.f.set(slot, "Segment", edgecode)?;
        // Media Composer ignores edge code on any other track number.
        self.f.set(slot, "PhysicalTrackNumber", 6)?;
        Ok(slot)
    }

    /// `aaf_transition`: a transition around an operation group of the
    /// dissolve the metadata describes, or `None`, which the track skips,
    /// for anything that is not a SMPTE dissolve.
    fn aaf_transition(&mut self, t: &Track, transition: NodeId) -> Result<Option<ObjRef>, Fail> {
        let Node::Transition(otio_transition) = self.document.try_get(transition)? else {
            return Err(unwritable("not a transition".to_owned()));
        };
        if otio_transition.transition_type != "SMPTE_Dissolve" {
            // Upstream prints "Unsupported transition type" and moves on.
            return Ok(None);
        }
        let metadata = aaf_metadata(&otio_transition.base.metadata)?;

        let (transition_params, varying_value) = match t.kind {
            Kind::Video => self.video_transition_parameters()?,
            Kind::Audio => self.audio_transition_parameters()?,
        };

        let interpolation_def = self.f.create_definition(
            "InterpolationDef",
            Some(crate::py::auid(LINEAR_INTERP)),
            Some("LinearInterp"),
            Some("Linear keyframe interpolation"),
        )?;
        self.f.register_def(interpolation_def)?;
        let linear = self.f.lookup_interpolationdef("LinearInterp")?;
        self.f.set(varying_value, "Interpolation", linear)?;

        let pointlist = metadata
            .get("PointList")
            .ok_or_else(|| unwritable("a transition without a PointList".to_owned()))?;
        let point = |i: usize, key: &str| -> Result<Any, Fail> {
            let item = match pointlist {
                Any::Vector(items) => items.get(i),
                _ => None,
            }
            .ok_or_else(|| unwritable(format!("a PointList without point {i}")))?;
            item.as_dictionary()
                .and_then(|d| d.get(key))
                .cloned()
                .ok_or_else(|| unwritable(format!("a control point without '{key}'")))
        };
        let mut points = Vec::new();
        for i in 0..2 {
            let c = self.f.create("ControlPoint")?;
            self.f.set(c, "EditHint", "Proportional")?;
            self.f
                .set(c, "Value", py::py_rational(&point(i, "Value")?)?)?;
            self.f
                .set(c, "Time", py::py_rational(&point(i, "Time")?)?)?;
            points.push(c);
        }
        self.f.extend(varying_value, "PointList", &points)?;

        let op_group = metadata.get_dict("OperationGroup")?;
        let operation = op_group.get_dict("Operation")?;
        let effect_id = operation.get("Identification");
        let is_time_warp = operation.get("IsTimeWarp").cloned().unwrap_or(Any::Null);
        let by_pass = operation.get("Bypass").cloned().unwrap_or(Any::Null);
        let number_inputs = operation.get("NumberInputs").cloned().unwrap_or(Any::Null);
        let operation_category = py::py_str(operation.get("OperationCategory"));
        let data_def = self.f.lookup_datadef(t.kind.media_kind())?;
        let description =
            py::py_str(Some(operation.get("Description").ok_or_else(|| {
                unwritable("an operation without a Description".to_owned())
            })?));
        let op_def_name = operation
            .get("Name")
            .ok_or_else(|| unwritable("an operation without a Name".to_owned()))?;

        // `uuid.UUID(effect_id)`.
        let effect_id = match effect_id {
            Some(Any::String(text)) => text
                .parse()
                .map_err(|e: aaf::ParseAuidError| unwritable(e.to_string()))?,
            _ => {
                return Err(unwritable(
                    "an operation whose Identification is not a UUID".to_owned(),
                ));
            }
        };
        let op_def_name = match op_def_name {
            Any::String(name) => Some(name.as_str()),
            Any::Null => None,
            other => {
                return Err(unwritable(format!(
                    "an operation name that is a {}",
                    other.type_name()
                )));
            }
        };
        let op_def =
            self.f
                .create_definition("OperationDef", Some(effect_id), op_def_name, None)?;
        self.f.register_def(op_def)?;
        self.f.set_media_kind(op_def, t.kind.media_kind())?;
        let datadef = self.f.lookup_datadef(t.kind.media_kind())?;
        self.set_py(op_def, "IsTimeWarp", &is_time_warp)?;
        self.set_py(op_def, "Bypass", &by_pass)?;
        self.set_py(op_def, "NumberInputs", &number_inputs)?;
        self.f
            .set(op_def, "OperationCategory", operation_category.as_str())?;
        self.f
            .extend(op_def, "ParametersDefined", &transition_params)?;
        self.f.set(op_def, "DataDefinition", data_def)?;
        self.f.set(op_def, "Description", description.as_str())?;

        let length = py::float_int(self.document.duration(transition)?.value())?;
        let operation_group = self.f.create_operation_group(op_def, length, None)?;
        self.f.set(operation_group, "DataDefinition", datadef)?;
        self.f
            .append(operation_group, "Parameters", varying_value)?;

        let aaf_transition = self.f.create_transition(t.kind.media_kind(), length)?;
        self.f
            .set(aaf_transition, "OperationGroup", operation_group)?;
        let cut_point = metadata
            .get("CutPoint")
            .cloned()
            .ok_or_else(|| unwritable("a transition without a CutPoint".to_owned()))?;
        self.set_py(aaf_transition, "CutPoint", &cut_point)?;
        self.f.set(aaf_transition, "DataDefinition", datadef)?;
        Ok(Some(aaf_transition))
    }

    /// A parameter definition registered in the dictionary.
    fn register_parameter_def(
        &mut self,
        id: &str,
        name: &str,
        typedef: &str,
    ) -> Result<ObjRef, Fail> {
        let typedef = self.f.lookup_typedef(typedef);
        let def = self
            .f
            .create_parameter_def(crate::py::auid(id), name, "", typedef)?;
        self.f.register_def(def)?;
        Ok(def)
    }

    /// `VideoTrackTranscriber._transition_parameters`: the parameters Avid
    /// defines on a dissolve, and a varying opacity.
    fn video_transition_parameters(&mut self) -> Result<(Vec<ObjRef>, ObjRef), Fail> {
        let param_byteorder = self.register_parameter_def(
            PARAMETERDEF_AVIDPARAMETERBYTEORDER,
            "AvidParameterByteOrder",
            "aafUInt16",
        )?;
        let param_effect_id = self.register_parameter_def(
            PARAMETERDEF_AVIDEFFECTID,
            "AvidEffectID",
            "AvidBagOfBits",
        )?;
        self.register_parameter_def(
            PARAMETERDEF_AFX_FG_KEY_OPACITY_U,
            "AFX_FG_KEY_OPACITY_U",
            "Rational",
        )?;

        let opacity_u = self.f.create_varying_value(None, None)?;
        let opacity = self.f.lookup_parameterdef("AFX_FG_KEY_OPACITY_U")?;
        self.f.set_parameter_def(opacity_u, opacity)?;
        self.f.set(
            opacity_u,
            "VVal_Extrapolation",
            crate::py::auid(VVAL_EXTRAPOLATION_ID),
        )?;
        self.f.set(opacity_u, "VVal_FieldCount", 1)?;
        Ok((vec![param_byteorder, param_effect_id], opacity_u))
    }

    /// `AudioTrackTranscriber._transition_parameters`: a varying level.
    fn audio_transition_parameters(&mut self) -> Result<(Vec<ObjRef>, ObjRef), Fail> {
        let param_def_level =
            self.register_parameter_def(PARAMETERDEF_LEVEL, "ParameterDef_Level", "Rational")?;
        let level = self.f.create_varying_value(None, None)?;
        let def = self.f.lookup_parameterdef("ParameterDef_Level")?;
        self.f.set_parameter_def(level, def)?;
        Ok((vec![param_def_level], level))
    }

    /// `aaf_sequence`: a nested track, as a sequence of what is on it.
    ///
    /// Its length adds up everything on it, transitions included, as
    /// upstream's does; and a transition the writer skips is one upstream
    /// fails on here, reading the length of nothing.
    fn aaf_sequence(&mut self, t: &Track, track: NodeId) -> Result<ObjRef, Fail> {
        let sequence = self.f.create_sequence(t.kind.media_kind())?;
        self.f.set(sequence, "Components", Vec::<ObjRef>::new())?;
        let mut length = 0;
        for child in self.document.children_of(track)? {
            let result = self.transcribe(t, child)?.ok_or_else(skipped_in_nesting)?;
            length += self.length(result)?;
            self.f.append(sequence, "Components", result)?;
        }
        self.f.set(sequence, "Length", length)?;
        Ok(sequence)
    }

    /// `aaf_operation_group`: a stack, as a submaster operation over what is
    /// in it.
    fn aaf_operation_group(&mut self, t: &Track, stack: NodeId) -> Result<ObjRef, Fail> {
        let media_kind = t.kind.media_kind();
        let op_def = self.f.create_definition(
            "OperationDef",
            Some(crate::py::auid(OPERATIONDEF_SUBMASTER)),
            Some("Submaster"),
            None,
        )?;
        self.f.register_def(op_def)?;
        self.f.set_media_kind(op_def, media_kind)?;
        let datadef = self.f.lookup_datadef(media_kind)?;
        self.f.set(op_def, "IsTimeWarp", false)?;
        self.f.set(op_def, "Bypass", 0)?;
        self.f.set(op_def, "NumberInputs", -1)?;
        self.f
            .set(op_def, "OperationCategory", "OperationCategory_Effect")?;
        self.f.set(op_def, "DataDefinition", datadef)?;

        let operation_group = self.f.create_operation_group(op_def, 0, None)?;
        self.f.set_media_kind(operation_group, media_kind)?;
        self.f.set(operation_group, "DataDefinition", datadef)?;
        let mut length = 0;
        for child in self.document.children_of(stack)? {
            let result = self.transcribe(t, child)?.ok_or_else(skipped_in_nesting)?;
            length += self.length(result)?;
            self.f.append(operation_group, "InputSegments", result)?;
        }
        self.f.set(operation_group, "Length", length)?;
        Ok(operation_group)
    }
}

impl Track {
    /// The OTIO kind this track has.
    const fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Video => TRACK_KIND_VIDEO,
            Kind::Audio => TRACK_KIND_AUDIO,
        }
    }
}

/// A transition the writer skips, met inside a nested track or stack, where
/// upstream goes on to read its length and fails.
fn skipped_in_nesting() -> Fail {
    unwritable("a transition other than a dissolve inside a nested track or stack".to_owned())
}

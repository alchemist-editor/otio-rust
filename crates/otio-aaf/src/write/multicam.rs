//! EXPERIMENTAL (ADR 0014): the prototype multicam schema written as Media
//! Composer writes a multicam edit.
//!
//! Each `Multicam` in the timeline's `multicams` becomes a backing
//! composition holding every angle's tracks as slots, each slot named after
//! its angle, and a lower-level group composition named after the multicam
//! that selects into it, one selector per track position. Each
//! `MulticamClip` becomes a selector whose choice is a source clip into the
//! slot its angle and track resolve to, with the same span of every other
//! angle's slot at that position as its alternates.
//!
//! ```text
//!   Timeline.multicams["mc"]   ──▶ backing CompositionMob "Group"
//!     angle "A": V, A, A             slot 1 "A" picture, 2 "A" sound, ...
//!     angle "B": V, A                slot 4 "B" picture, 5 "B" sound
//!                                  group CompositionMob "Group" (lower level)
//!                                    slot 1: Selector(1 | 4), slot 2: (2 | 5)
//!   V1: MulticamClip(A, 0)     ──▶ Selector: Selected slot 1, Alternates [4]
//! ```
//!
//! A reader without multicam support takes each selector's choice, so the
//! file reads as the flattened edit everywhere else.

use std::collections::HashMap;

use aaf::write::ObjRef;
use otio_core::multicam;
use otio_core::{Node, NodeId, TRACK_KIND_AUDIO, TRACK_KIND_VIDEO};

use super::py;
use super::track::{Kind, Track};
use super::{Fail, FileTranscriber, rational, unwritable};

/// A multicam, as written: its backing composition and where each angle's
/// tracks went in it.
#[derive(Debug, Default)]
pub(crate) struct Backing {
    mob: Option<ObjRef>,
    /// The angles, in the multicam's order.
    angles: Vec<String>,
    /// The slot ID for each (angle, kind, place among the angle's tracks of
    /// that kind).
    slots: HashMap<(String, Kind, i64), u32>,
    /// Each slot's edit rate.
    rates: HashMap<u32, f64>,
}

/// The kind of a track, if it is one AAF holds.
fn kind_of(node: &Node) -> Option<Kind> {
    match node {
        Node::Track(track) if track.kind == TRACK_KIND_VIDEO => Some(Kind::Video),
        Node::Track(track) if track.kind == TRACK_KIND_AUDIO => Some(Kind::Audio),
        _ => None,
    }
}

impl FileTranscriber<'_> {
    /// Writes every multicam the timeline defines, before the timeline's own
    /// tracks, so that the cuts on them have somewhere to point.
    pub(crate) fn write_multicams(&mut self, timeline: NodeId) -> Result<(), Fail> {
        for (key, definition) in multicam::multicams(self.document, timeline)? {
            let backing = self.write_multicam(definition)?;
            self.multicams.insert(key, backing);
        }
        Ok(())
    }

    fn write_multicam(&mut self, definition: NodeId) -> Result<Backing, Fail> {
        let name = self.document.try_get(definition)?.name().to_owned();
        let mob = self.f.create("CompositionMob")?;
        self.f.set(mob, "Name", name.as_str())?;
        self.f.add_mob(mob)?;

        // The angles' tracks are written as the timeline's are, into the
        // backing composition rather than the timeline's.
        let timeline_mob = std::mem::replace(&mut self.composition_mob, mob);
        let written = self.write_angles(definition);
        self.composition_mob = timeline_mob;
        let mut backing = written?;
        backing.mob = Some(mob);

        // The group Media Composer shows: a selector per track position,
        // choosing the first angle that has a track there.
        let group = self.f.create("CompositionMob")?;
        self.f.set(group, "Name", name.as_str())?;
        self.f.set(group, "UsageCode", "Usage_LowerLevel")?;
        self.f.add_mob(group)?;
        let mut positions: Vec<(Kind, i64)> = Vec::new();
        for (_, kind, index) in Self::ordered_slots(&backing) {
            if !positions.contains(&(kind, index)) {
                positions.push((kind, index));
            }
        }
        for (kind, index) in positions {
            let choices: Vec<u32> = backing
                .angles
                .iter()
                .filter_map(|angle| backing.slots.get(&(angle.clone(), kind, index)).copied())
                .collect();
            let Some(first) = choices.first() else {
                continue;
            };
            let first_slot = self.f.slot_at(mob, *first)?;
            let length = self.f.slot_length(first_slot)?;
            let rate = backing.rates.get(first).copied().unwrap_or(1.0);
            let clips = self.source_clips(mob, &choices, kind, 0, length)?;
            let selector = self.selector(kind, length, clips)?;
            let slot = self.f.create_timeline_slot(group, rational(rate)?, None)?;
            self.f.set(slot, "Segment", selector)?;
        }
        Ok(backing)
    }

    fn write_angles(&mut self, definition: NodeId) -> Result<Backing, Fail> {
        let mut backing = Backing::default();
        for angle in self.document.children_of(definition)? {
            let angle_name = self.document.try_get(angle)?.name().to_owned();
            backing.angles.push(angle_name.clone());
            let mut per_kind: HashMap<Kind, i64> = HashMap::new();
            for track in self.document.children_of(angle)? {
                let Some(kind) = kind_of(self.document.try_get(track)?) else {
                    continue;
                };
                let index = per_kind.entry(kind).or_default();
                let at = *index;
                *index += 1;
                // A track with nothing on it has no rate to give a slot, and
                // a cut into it is refused when the cut is written.
                if self.document.children_of(track)?.is_empty() {
                    continue;
                }
                let t = self.track_transcriber(track)?;
                // The angle's name is what the reader groups slots by.
                self.f
                    .set(t.timeline_mobslot, "SlotName", angle_name.as_str())?;
                for child in self.document.children_of(track)? {
                    if let Some(result) = self.transcribe(&t, child)? {
                        self.f.append(t.sequence, "Components", result)?;
                    }
                }
                let slot_id = self.slot_id(t.timeline_mobslot)?;
                backing
                    .slots
                    .insert((angle_name.clone(), kind, at), slot_id);
                backing.rates.insert(slot_id, t.edit_rate);
            }
        }
        Ok(backing)
    }

    /// Every written slot of a backing composition, in the order written.
    fn ordered_slots(backing: &Backing) -> Vec<(u32, Kind, i64)> {
        let mut slots: Vec<(u32, Kind, i64)> = backing
            .slots
            .iter()
            .map(|((_, kind, index), slot)| (*slot, *kind, *index))
            .collect();
        slots.sort_by_key(|(slot, _, _)| *slot);
        slots
    }

    /// A source clip into each of `slots` of `mob`, all over the same span.
    fn source_clips(
        &mut self,
        mob: ObjRef,
        slots: &[u32],
        kind: Kind,
        start: i64,
        length: i64,
    ) -> Result<Vec<ObjRef>, Fail> {
        let mut clips = Vec::new();
        for slot in slots {
            clips.push(self.f.create_mob_source_clip(
                mob,
                *slot,
                Some(start),
                Some(length),
                Some(kind.media_kind()),
            )?);
        }
        Ok(clips)
    }

    /// A selector choosing the first of `clips`, with the rest as its
    /// alternates.
    fn selector(&mut self, kind: Kind, length: i64, clips: Vec<ObjRef>) -> Result<ObjRef, Fail> {
        let mut clips = clips.into_iter();
        let selected = clips
            .next()
            .ok_or_else(|| unwritable("a selector with nothing to select".to_owned()))?;
        let selector =
            self.f
                .create_component("Selector", Some(kind.media_kind()), Some(length))?;
        self.f.set(selector, "Selected", selected)?;
        self.f
            .set(selector, "Alternates", clips.collect::<Vec<ObjRef>>())?;
        Ok(selector)
    }

    /// A multicam cut, as a selector into its multicam's backing
    /// composition.
    pub(crate) fn aaf_selector(&mut self, t: &Track, cut: NodeId) -> Result<ObjRef, Fail> {
        let node = self.document.try_get(cut)?;
        let found = multicam::cut(node)
            .ok_or_else(|| unwritable(format!("'{}' is not a multicam cut", node.name())))?;
        let backing = self.multicams.get(&found.multicam).ok_or_else(|| {
            unwritable(format!(
                "cut '{}' plays multicam '{}', which the timeline does not define",
                node.name(),
                found.multicam
            ))
        })?;
        let mob = backing
            .mob
            .ok_or_else(|| unwritable("a multicam that was not written".to_owned()))?;
        let position = (t.kind, found.angle_track);
        let Some(chosen) = backing
            .slots
            .get(&(found.angle.clone(), position.0, position.1))
            .copied()
        else {
            return Err(unwritable(format!(
                "cut '{}' plays {} track {} of angle '{}', which multicam '{}' does not have",
                node.name(),
                t.kind_name(),
                found.angle_track,
                found.angle,
                found.multicam
            )));
        };
        let mut slots = vec![chosen];
        for angle in &backing.angles {
            if *angle == found.angle {
                continue;
            }
            if let Some(slot) = backing.slots.get(&(angle.clone(), position.0, position.1)) {
                slots.push(*slot);
            }
        }
        let range = self.document.visible_range(cut)?;
        let start = py::float_int(range.start_time().value())?;
        let length = py::float_int(range.duration().value())?;
        let clips = self.source_clips(mob, &slots, t.kind, start, length)?;
        self.selector(t.kind, length, clips)
    }
}

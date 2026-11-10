//! EXPERIMENTAL (ADR 0014): Avid multicam edits as the prototype multicam
//! schema, on reading.
//!
//! Media Composer writes a multicam edit as a selector per cut. The
//! selector's choice and its alternates are source clips into one backing
//! composition, which holds every angle's tracks as slots: a picture slot
//! and a sound slot per channel, each padded to the group's length. A
//! lower-level composition, named after the group, selects into the backing
//! one too.
//!
//! With [`crate::ReadOptions::multicam`] on, that backing composition becomes
//! a `Multicam`, with an angle per camera, and each selector into it a
//! `MulticamClip` naming the angle and track it plays. Slots are grouped
//! into angles by their name if they have one, which is what this crate's
//! writer gives them, and otherwise by the master clip they start with,
//! which is what Media Composer's files give.
//!
//! ```text
//!   top-level composition            backing composition
//!   V1: Selector ─ Selected ───────▶ slot 1  (A01 picture)   ┐ angle A01
//!                 Alternates ──┐     slot 2  (A01 sound 1)   ┘
//!                              └───▶ slot 18 (A02 picture)   ┐ angle A02
//!                                    ...                     ┘
//! ```

use std::collections::HashMap;
use std::io::{Read, Seek};

use aaf::{MobId, Object};
use opentime::{RationalTime, TimeRange};
use otio_core::multicam::{self as schema, Cut};
use otio_core::schema::Stack;
use otio_core::{Any, AnyDictionary, Document, Node, NodeId};

use crate::Transcriber;
use crate::error::Result;
use crate::py::Py;
use crate::simplify;
use crate::transcribe::{EditRate, frames, item, rate_of, track_kind, wrap};

/// A backing composition, read as a multicam.
#[derive(Debug)]
pub(crate) struct Definition {
    /// The key cuts name it by in `Timeline.multicams`.
    pub(crate) key: String,
    /// The `Multicam` stack.
    pub(crate) node: NodeId,
    /// Each slot of the backing composition, by slot ID: the angle it
    /// belongs to, its OTIO kind, and its place among the angle's tracks of
    /// that kind.
    slots: HashMap<i64, (String, String, i64)>,
}

/// The key a backing composition is filed under.
pub(crate) fn key_for(mob: MobId) -> String {
    format!("aaf:{mob}")
}

/// A name without the extension a file name gives it, as Python's
/// `os.path.splitext`.
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(at) if at > 0 => &name[..at],
        _ => name,
    }
}

impl<R: Read + Seek> Transcriber<R> {
    /// The cut a selector is, if the multicam read is on and the selector
    /// chooses among the slots of a composition.
    ///
    /// `Ok(None)` leaves the selector to the upstream reading.
    pub(crate) fn multicam_cut(
        &mut self,
        selector: &Object,
        selected: &Object,
        edit_rate: EditRate,
    ) -> Result<Option<NodeId>> {
        if !self.multicam || !self.py_is(selected, "SourceClip") {
            return Ok(None);
        }
        let Some(backing) = self.mob_of(selected)? else {
            return Ok(None);
        };
        if !self.py_is(&backing, "CompositionMob") {
            return Ok(None);
        }
        let Some(index) = self.multicam_definition(&backing, edit_rate)? else {
            return Ok(None);
        };
        let slot_id = self
            .value_of(selected, "SourceMobSlotID")?
            .as_i64()
            .unwrap_or_default();
        let Some((angle, _, angle_track)) = self.multicams[index].slots.get(&slot_id).cloned()
        else {
            return Ok(None);
        };
        let rate = rate_of(edit_rate);
        let start = self
            .value_of(selected, "StartTime")?
            .as_i64()
            .unwrap_or_default();
        let length = self
            .length(selector)?
            .or(self.length(selected)?)
            .unwrap_or_default();
        let cut = Cut {
            multicam: self.multicams[index].key.clone(),
            angle: angle.clone(),
            angle_track,
        };
        let range = TimeRange::new(frames(start, rate), frames(length, rate));
        self.log(|| format!("Creating MulticamClip for angle {angle}"));
        Ok(Some(schema::new_multicam_clip(
            &mut self.document,
            &angle,
            &cut,
            range,
        )))
    }

    /// The definition for a backing composition, read on first use.
    ///
    /// `None` if the composition has no picture or sound slot to make an
    /// angle of.
    fn multicam_definition(
        &mut self,
        backing: &Object,
        edit_rate: EditRate,
    ) -> Result<Option<usize>> {
        let Some(mob_id) = self.mob_id_of(backing)? else {
            return Ok(None);
        };
        if let Some(index) = self.multicam_index.get(&mob_id) {
            return Ok(Some(*index));
        }

        // The composition's own tracks, read the ordinary way, by slot.
        let timeline = self
            .nested(|s| s.transcribe(backing, &[], edit_rate))?
            .ok_or(crate::Error::Malformed("a mob transcribed to nothing"))?;
        let mut tracks_by_slot = HashMap::new();
        for track in self.timeline_tracks(timeline)? {
            if let Some(slot) = self.meta_int(track, "SlotID") {
                tracks_by_slot.insert(slot, track);
            }
        }

        // Group the picture and sound slots into angles.
        let mut order: Vec<String> = Vec::new();
        let mut members: HashMap<String, Vec<(i64, String)>> = HashMap::new();
        let mut rate = None;
        for slot in self.aaf.slots(backing)? {
            let kind = track_kind(self.media_kind(&slot)?.as_deref());
            if kind != "Video" && kind != "Audio" {
                continue;
            }
            let Some(slot_id) = self.value_of(&slot, "SlotID")?.as_i64() else {
                continue;
            };
            rate = rate.or(self.edit_rate_of(&slot)?);
            let name = self.angle_name_of(&slot, slot_id)?;
            if !members.contains_key(&name) {
                order.push(name.clone());
            }
            members.entry(name).or_default().push((slot_id, kind));
        }
        if order.is_empty() {
            return Ok(None);
        }

        let name = match self.group_name_for(mob_id)? {
            Some(name) => name,
            None => match self.py_name(backing)? {
                Some(name) if !name.is_empty() => name,
                _ => "Multicam".to_owned(),
            },
        };
        let start = RationalTime::new(0.0, rate.or(edit_rate).unwrap_or(1.0));
        let node = schema::new_multicam(&mut self.document, &name, Some(start));
        let mut aaf = AnyDictionary::new();
        aaf.insert("MobID".to_owned(), Any::String(mob_id.to_string()));
        if let Some(base) = self.document.get_mut(node).and_then(Node::base_mut) {
            base.metadata = wrap(aaf);
        }

        let mut slots = HashMap::new();
        for angle in order {
            let stack = self.document.insert(Node::Stack(Stack {
                item: item(angle.clone(), AnyDictionary::new()),
                children: Vec::new(),
            }));
            let mut per_kind: HashMap<String, i64> = HashMap::new();
            for (slot_id, kind) in &members[&angle] {
                let Some(track) = tracks_by_slot.get(slot_id) else {
                    continue;
                };
                let copy = self.document.deep_clone(*track)?;
                self.document.append_child(stack, copy)?;
                let index = per_kind.entry(kind.clone()).or_default();
                slots.insert(*slot_id, (angle.clone(), kind.clone(), *index));
                *index += 1;
            }
            self.document.append_child(node, stack)?;
        }

        let index = self.multicams.len();
        self.multicams.push(Definition {
            key: key_for(mob_id),
            node,
            slots,
        });
        self.multicam_index.insert(mob_id, index);
        Ok(Some(index))
    }

    /// The angle a backing slot belongs to: the slot's name, else the name
    /// of the first master clip on it without its extension, else one made
    /// from the slot ID.
    fn angle_name_of(&mut self, slot: &Object, slot_id: i64) -> Result<String> {
        if let Py::Str(name) = self.value_of(slot, "SlotName")? {
            if !name.is_empty() {
                return Ok(name);
            }
        }
        for component in self.slot_components(slot)? {
            if !self.py_is(&component, "SourceClip") {
                continue;
            }
            if let Some(mob) = self.mob_of(&component)? {
                if let Some(name) = self.py_name(&mob)? {
                    if !name.is_empty() {
                        return Ok(stem(&name).to_owned());
                    }
                }
            }
        }
        Ok(format!("Angle {slot_id}"))
    }

    /// The name of the group composition that selects from a backing one,
    /// which is the name Media Composer shows for the multicam.
    fn group_name_for(&mut self, backing: MobId) -> Result<Option<String>> {
        for mob in self.aaf.mobs_of("CompositionMob")? {
            if self.mob_id_of(&mob)? == Some(backing) {
                continue;
            }
            for slot in self.aaf.slots(&mob)? {
                let Py::Object(segment) = self.value_of(&slot, "Segment")? else {
                    continue;
                };
                if !self.py_is(&segment, "Selector") {
                    continue;
                }
                let Py::Object(selected) = self.value_of(&segment, "Selected")? else {
                    continue;
                };
                let Some(target) = self.mob_of(&selected)? else {
                    continue;
                };
                if self.mob_id_of(&target)? == Some(backing) {
                    if let Some(name) = self.py_name(&mob)? {
                        if !name.is_empty() {
                            return Ok(Some(name));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// Files each definition on the timelines whose cuts name it: the first
    /// such timeline gets the definition, any other a copy.
    pub(crate) fn attach_multicams(&mut self, root: NodeId) -> Result<()> {
        if self.multicams.is_empty() {
            return Ok(());
        }
        let definitions: Vec<(String, NodeId)> = self
            .multicams
            .iter()
            .map(|definition| (definition.key.clone(), definition.node))
            .collect();
        attach(&mut self.document, root, &definitions)
    }
}

/// The timelines a read produced: the root, or the root's children.
fn timelines(document: &Document, root: NodeId) -> Vec<NodeId> {
    match document.get(root) {
        Some(Node::Timeline(_)) => vec![root],
        Some(node) => node
            .children()
            .unwrap_or_default()
            .iter()
            .copied()
            .filter(|id| matches!(document.get(*id), Some(Node::Timeline(_))))
            .collect(),
        None => Vec::new(),
    }
}

fn attach(document: &mut Document, root: NodeId, definitions: &[(String, NodeId)]) -> Result<()> {
    let mut used = vec![false; definitions.len()];
    for timeline in timelines(document, root) {
        let Some(Node::Timeline(found)) = document.get(timeline) else {
            continue;
        };
        let Some(tracks) = found.tracks else {
            continue;
        };
        let keys: Vec<String> = document
            .find_children(tracks, None, false, &schema::is_multicam_clip)?
            .into_iter()
            .filter_map(|cut| document.get(cut).and_then(schema::cut))
            .map(|cut| cut.multicam)
            .collect();
        for (at, (key, node)) in definitions.iter().enumerate() {
            if !keys.contains(key) {
                continue;
            }
            let node = if used[at] {
                document.deep_clone(*node)?
            } else {
                used[at] = true;
                *node
            };
            schema::set_multicam(document, timeline, key, Some(node))?;
        }
    }
    Ok(())
}

/// Simplifies each angle of each multicam on the read's timelines, as the
/// timeline itself is simplified.
///
/// Each angle is simplified on its own, out of its multicam, so that its
/// tracks are top-level tracks to the pass and keep their place; an angle
/// the pass reduces to its one track is given its stack back.
pub(crate) fn simplify_multicams(document: &mut Document, root: NodeId) -> Result<()> {
    for timeline in timelines(document, root) {
        for multicam in schema::multicams(document, timeline)?.into_values() {
            let angles = document.children_of(multicam)?;
            for (at, angle) in angles.into_iter().enumerate() {
                let at = i64::try_from(at).unwrap_or(i64::MAX);
                document.remove_child(multicam, at)?;
                let name = document.try_get(angle)?.name().to_owned();
                let mut simplified = simplify::simplify(document, angle)?;
                if !matches!(document.get(simplified), Some(Node::Stack(_))) {
                    let stack = document.insert(Node::Stack(Stack {
                        item: item(name.clone(), AnyDictionary::new()),
                        children: Vec::new(),
                    }));
                    document.append_child(stack, simplified)?;
                    simplified = stack;
                }
                if let Some(base) = document.get_mut(simplified).and_then(Node::base_mut) {
                    base.name = name;
                }
                document.insert_child(multicam, at, simplified)?;
            }
        }
    }
    Ok(())
}

//! The path from a master mob to a clip and the media behind it.
//!
//! A master mob is where an edit stops being structure and starts being
//! media, so it does not go through the walk the rest of the file does. Its
//! slots become tracks directly, and each source clip in one is followed down
//! a chain of mobs until the chain reaches a source mob with a file on it.
//!
//! # The chain
//!
//! AAF does not put a path on a clip. A master mob's source clip names a
//! source mob, which describes the signal; that one's source clip names
//! another, which describes the tape or the session it came off; and so on
//! until a mob names nothing. Walking it backwards, from the far end towards
//! the master mob, is what narrows a whole tape down to the span this clip
//! uses: each step's range is clamped into the one before it.
//!
//! Every source mob along the way that carries a file contributes a media
//! reference, so the clip ends up with the original alongside whatever stands
//! in for it. One with no file contributes a missing reference rather than
//! nothing, so the clip still says what it is looking for.

use std::io::{Read, Seek};

use aaf::{Object, Value};
use opentime::{RationalTime, TimeRange};
use otio_core::schema::{Base, Clip, ExternalReference, Gap, MediaReferenceData, Track};
use otio_core::{Any, AnyDictionary, Node, NodeId};

use crate::error::Result;
use crate::{Transcriber, item_with, metadata};

/// The key OTIO keeps a clip's active media reference under.
const DEFAULT_MEDIA_KEY: &str = "DEFAULT_MEDIA";

/// How long a chain of mobs is followed before giving up.
///
/// Each step is a mob naming another. Real files run two or three deep; a
/// file describing a cycle would otherwise run forever.
const MAX_CHAIN: usize = 32;

/// One step of the chain: a mob, the slot in it, and the clip in that slot.
struct Step {
    mob: Object,
    slot: Object,
    clip: Object,
}

impl<R: Read + Seek> Transcriber<R> {
    /// A master mob's slots, as tracks of clips.
    pub(crate) fn master_mob_tracks(&mut self, mob: &Object) -> Result<Vec<NodeId>> {
        // A master mob can carry a timecode of its own, which offsets
        // everything the mob describes.
        let global_start = self.start_timecode(mob)?;
        let mut tracks = Vec::new();

        for slot in self.aaf.slots(mob)? {
            if !self.aaf.is_a(&slot, "TimelineMobSlot") {
                continue;
            }
            let rate = self.edit_rate_of(&slot)?;
            let mut children = Vec::new();

            for component in self.slot_components(&slot)? {
                let length = self.length(&component)?.unwrap_or_default();
                if length == 0 {
                    continue;
                }
                let item = if self.aaf.is_a(&component, "SourceClip") {
                    self.clip_from(mob, &slot, &component, global_start)?
                } else {
                    // Anything that is not a clip still occupies its time.
                    Some(self.gap_of_length(length, rate))
                };
                if let Some(item) = item {
                    children.push(item);
                }
            }

            let kind = match self.aaf.child(&slot, "Segment")? {
                Some(segment) => self.track_kind(&segment)?,
                None => String::new(),
            };
            let name = self.aaf.name(&slot)?.unwrap_or_default();
            // Built here rather than through the walk, so the metadata is
            // the slot's own properties with nothing added: no injected
            // `Name`, and no `MediaKind`, both of which the walk would add.
            let aaf = metadata::object_properties(self, &slot)?;
            let track = self.document.insert(Node::Track(Track {
                item: item_with(name, aaf),
                children,
                kind,
            }));
            self.reparent(track)?;
            tracks.push(track);
        }
        Ok(tracks)
    }

    /// One source clip, as a clip with every media reference behind it.
    fn clip_from(
        &mut self,
        mob: &Object,
        slot: &Object,
        component: &Object,
        global_start: Option<TimeRange>,
    ) -> Result<Option<NodeId>> {
        let chain = self.reference_chain(mob, slot, component)?;
        let mut references: Vec<(String, NodeId)> = Vec::new();
        let mut in_range: Option<TimeRange> = None;
        let mut clip = None;

        // Backwards: the far end of the chain is the widest span, and each
        // step towards the master mob narrows it.
        for step in chain.iter().rev() {
            let is_source = self.aaf.is_a(&step.mob, "SourceMob");
            // A master mob's own timecode is the global offset rather than a
            // start within its media, so it is not applied again here.
            let start_tc = if is_source {
                self.start_timecode(&step.mob)?
            } else {
                None
            };
            let available = self.clip_range(&step.slot, &step.clip, in_range, start_tc)?;

            if is_source {
                references.extend(self.source_mob_references(
                    &step.mob,
                    available,
                    global_start,
                )?);
            } else if self.aaf.is_a(&step.mob, "MasterMob") {
                references.reverse();
                clip = Some(self.clip_with(
                    &step.mob,
                    component,
                    available,
                    std::mem::take(&mut references),
                    global_start,
                )?);
            }
            in_range = Some(available);
        }
        Ok(clip)
    }

    /// The chain of mobs a source clip leads through.
    fn reference_chain(&mut self, mob: &Object, slot: &Object, clip: &Object) -> Result<Vec<Step>> {
        let mut chain = Vec::new();
        let mut mob = mob.clone();
        let mut slot = slot.clone();
        let mut clip = clip.clone();

        loop {
            chain.push(Step {
                mob: mob.clone(),
                slot: slot.clone(),
                clip: clip.clone(),
            });
            if chain.len() > MAX_CHAIN {
                break;
            }
            let Some(next_mob) = self.mob_named_by(&clip)? else {
                break;
            };
            let Some(next_slot) = self.slot_named_by(&next_mob, &clip)? else {
                break;
            };
            let Some(next_clip) = self
                .slot_components(&next_slot)?
                .into_iter()
                .find(|component| self.aaf.is_a(component, "SourceClip"))
            else {
                break;
            };
            mob = next_mob;
            slot = next_slot;
            clip = next_clip;
        }
        Ok(chain)
    }

    /// A source mob's files, as media references.
    fn source_mob_references(
        &mut self,
        mob: &Object,
        available: TimeRange,
        global_start: Option<TimeRange>,
    ) -> Result<Vec<(String, NodeId)>> {
        let mut aaf = metadata::object_properties(self, mob)?;
        aaf.insert("Name".to_owned(), Any::String(self.name_of(mob)?));
        let name = match self.aaf.name(mob)? {
            Some(name) if !name.is_empty() => name,
            _ => self
                .aaf
                .mob_id(mob)?
                .map(|id| id.to_string())
                .unwrap_or_default(),
        };
        let available = shift(available, global_start);

        let urls = self.locator_urls(mob)?;
        let mut out = Vec::new();
        // A source mob with no file still contributes a reference, so the
        // clip says what it could not find rather than saying nothing.
        for url in if urls.is_empty() { vec![None] } else { urls } {
            let media = MediaReferenceData {
                base: Base {
                    name: name.clone(),
                    metadata: crate::transcribe::wrap(aaf.clone()),
                },
                available_range: Some(available),
                available_image_bounds: None,
            };
            let node = match url {
                Some(target_url) => {
                    self.document
                        .insert(Node::ExternalReference(ExternalReference {
                            media,
                            target_url,
                        }))
                }
                None => self.document.insert(Node::MissingReference(
                    otio_core::schema::MissingReference { media },
                )),
            };
            out.push((name.clone(), node));
        }
        Ok(out)
    }

    /// The files a source mob's descriptor points at.
    fn locator_urls(&mut self, mob: &Object) -> Result<Vec<Option<String>>> {
        let Some(descriptor) = self.aaf.child(mob, "EssenceDescription")? else {
            return Ok(Vec::new());
        };
        let mut urls = Vec::new();
        for locator in self.aaf.children(&descriptor, "Locator")? {
            if let Ok(Some(Value::String(url))) = self.aaf.value(&locator, "URLString") {
                if !url.is_empty() {
                    urls.push(Some(file_url(&url)));
                }
            }
        }
        Ok(urls)
    }

    /// The clip a master mob's slot makes, with its references attached.
    fn clip_with(
        &mut self,
        mob: &Object,
        component: &Object,
        source_range: TimeRange,
        references: Vec<(String, NodeId)>,
        global_start: Option<TimeRange>,
    ) -> Result<NodeId> {
        let source_range = shift(source_range, global_start);
        let name = self.aaf.name(mob)?.unwrap_or_default();
        // The clip carries the master mob's own properties rather than the
        // source clip's: a clip in a timeline is the mob, and the source clip
        // is only how the slot reached it.
        let mut aaf = metadata::object_properties(self, mob)?;
        aaf.insert("Name".to_owned(), Any::String(self.name_of(mob)?));
        if let Some(kind) = self.aaf.media_kind(component).ok().flatten() {
            aaf.insert("MediaKind".to_owned(), Any::String(kind));
        }

        let mut item = item_with(name, aaf);
        item.source_range = Some(source_range);

        // Names repeat when two references come off the same mob, so a
        // repeat is numbered rather than overwriting what is already there.
        let mut media_references = std::collections::BTreeMap::new();
        let mut active = String::new();
        for (index, (name, node)) in references.into_iter().enumerate() {
            let key = if index == 0 {
                active = DEFAULT_MEDIA_KEY.to_owned();
                DEFAULT_MEDIA_KEY.to_owned()
            } else {
                let mut key = name.clone();
                let mut attempt = 1;
                while media_references.contains_key(&key) {
                    key = format!("{name}_{attempt:02}");
                    attempt += 1;
                }
                key
            };
            media_references.insert(key, node);
        }

        Ok(self.document.insert(Node::Clip(Clip {
            item,
            media_references,
            active_media_reference_key: active,
        })))
    }

    /// The span of media one step of the chain makes available.
    fn clip_range(
        &mut self,
        slot: &Object,
        clip: &Object,
        in_range: Option<TimeRange>,
        start_tc: Option<TimeRange>,
    ) -> Result<TimeRange> {
        let rate = self.edit_rate_of(slot)?;
        #[expect(
            clippy::cast_precision_loss,
            reason = "AAF times are frame counts, well inside f64"
        )]
        let mut start = RationalTime::new(self.start_of(clip)? as f64, rate);
        #[expect(
            clippy::cast_precision_loss,
            reason = "an AAF length is a frame count, well inside f64"
        )]
        let mut duration = RationalTime::new(self.length(clip)?.unwrap_or_default() as f64, rate);

        if let Some(tc) = start_tc {
            start += tc.start_time().rescaled_to(rate);
            let tc_duration = tc.duration().rescaled_to(rate);
            if tc_duration.value() > duration.value() {
                duration = tc_duration;
            }
        }
        if let Some(in_range) = in_range {
            start += in_range.start_time().rescaled_to(rate);
            return Ok(clamp(TimeRange::new(start, duration), in_range));
        }
        Ok(TimeRange::new(start, duration))
    }

    /// The mob a source clip names, if the file holds it.
    fn mob_named_by(&mut self, clip: &Object) -> Result<Option<Object>> {
        match self.aaf.value(clip, "SourceID") {
            Ok(Some(Value::MobId(id))) => Ok(self.aaf.mob(id)?),
            _ => Ok(None),
        }
    }

    /// The slot a source clip names within the mob it points at.
    fn slot_named_by(&mut self, mob: &Object, clip: &Object) -> Result<Option<Object>> {
        let Some(wanted) = self
            .aaf
            .value(clip, "SourceMobSlotID")
            .ok()
            .flatten()
            .and_then(|value| value.as_i64())
        else {
            return Ok(None);
        };
        for slot in self.aaf.slots(mob)? {
            let found = self
                .aaf
                .value(&slot, "SlotID")
                .ok()
                .flatten()
                .and_then(|value| value.as_i64());
            if found == Some(wanted) {
                return Ok(Some(slot));
            }
        }
        Ok(None)
    }

    /// A slot's components: a sequence's, or the segment itself.
    pub(crate) fn slot_components(&mut self, slot: &Object) -> Result<Vec<Object>> {
        let Some(segment) = self.aaf.child(slot, "Segment")? else {
            return Ok(Vec::new());
        };
        if self.aaf.is_a(&segment, "Sequence") {
            return Ok(self.aaf.components(&segment)?);
        }
        Ok(vec![segment])
    }

    /// A slot's edit rate, which is what its times are counted in.
    pub(crate) fn edit_rate_of(&mut self, slot: &Object) -> Result<f64> {
        Ok(self
            .aaf
            .value(slot, "EditRate")
            .ok()
            .flatten()
            .and_then(|value| value.as_rational())
            .map_or(1.0, |(numerator, denominator)| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "an edit rate is a small ratio like 48000/1"
                )]
                let rate = numerator as f64 / denominator as f64;
                rate
            }))
    }

    /// Where a source clip starts within the media it names.
    fn start_of(&mut self, clip: &Object) -> Result<i64> {
        Ok(self
            .aaf
            .value(clip, "StartTime")
            .ok()
            .flatten()
            .and_then(|value| value.as_i64())
            .unwrap_or_default())
    }

    /// A gap of a length, in a rate.
    fn gap_of_length(&mut self, length: i64, rate: f64) -> NodeId {
        let mut item = item_with(String::new(), AnyDictionary::new());
        #[expect(
            clippy::cast_precision_loss,
            reason = "an AAF length is a frame count, well inside f64"
        )]
        let duration = RationalTime::new(length as f64, rate);
        item.source_range = Some(TimeRange::new(RationalTime::new(0.0, rate), duration));
        self.document.insert(Node::Gap(Gap { item }))
    }
}

/// A range moved along by a mob's own timecode, if it has one.
fn shift(range: TimeRange, by: Option<TimeRange>) -> TimeRange {
    match by {
        Some(by) => TimeRange::new(range.start_time() + by.start_time(), range.duration()),
        None => range,
    }
}

/// A range held within another, as upstream's `clamped` does it.
fn clamp(range: TimeRange, bounds: TimeRange) -> TimeRange {
    let start = if range.start_time().value() < bounds.start_time().value() {
        bounds.start_time()
    } else {
        range.start_time()
    };
    let end = range.start_time() + range.duration();
    let bound_end = bounds.start_time() + bounds.duration();
    let end = if end.value() > bound_end.value() {
        bound_end
    } else {
        end
    };
    TimeRange::new(start, end - start)
}

/// A path as a URL, which is what OTIO holds.
///
/// A path that is already a URL is left alone, and the separators of a
/// Windows path are turned round, since a URL has only the one kind.
fn file_url(path: &str) -> String {
    let url = if path.starts_with("file://") {
        path.to_owned()
    } else {
        format!("file://{path}")
    };
    url.replace('\\', "/")
}

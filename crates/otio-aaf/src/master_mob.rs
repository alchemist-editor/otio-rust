//! The path from a master mob to a clip and the media behind it.
//!
//! A master mob is where an edit stops being structure and starts being
//! media, so it does not go through the walk the rest of the file does. Its
//! slots become tracks directly, and each source clip in one is followed down
//! a chain of mobs until the chain runs out.
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
//! Every source mob along the way contributes a media reference for each file
//! its descriptor locates, so the clip ends up with the original alongside
//! whatever stands in for it. One that locates no file contributes a missing
//! reference rather than nothing, so the clip still says what it is missing.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use aaf::Object;
use opentime::TimeRange;
use otio_core::schema::{Base, Clip, ExternalReference, Gap, MediaReferenceData, MissingReference};
use otio_core::{Any, AnyDictionary, Node, NodeId};

use crate::Transcriber;
use crate::error::Result;
use crate::log::bytes_repr;
use crate::py::Py;
use crate::transcribe::{frames, item, item_fields, track_kind, wrap};

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
    /// A master mob, as a timeline with a track per slot.
    ///
    /// `metadata` is the mob's own, and every clip made here carries a copy
    /// of it: a clip in a timeline stands for the mob, and the source clip in
    /// its slot is only how the slot reached its media.
    pub(crate) fn transcribe_master_mob(
        &mut self,
        mob: &Object,
        chain: &[Object],
        metadata: &AnyDictionary,
    ) -> Result<NodeId> {
        let name = match self.value_of(mob, "Name")? {
            Py::Str(name) => name,
            _ => String::new(),
        };
        let timeline = self.timeline_of(name, AnyDictionary::new(), Vec::new())?;
        let stack = match self.document.get(timeline) {
            Some(Node::Timeline(found)) => found.tracks,
            _ => None,
        }
        .expect("a timeline was just made with a stack");

        // A master mob can carry a timecode of its own, which offsets
        // everything it describes.
        let global_start = self.mob_start_timecode(mob)?;

        for slot in self.aaf.slots(mob)? {
            let rate = self.edit_rate_of(&slot)?.unwrap_or(1.0);
            if self.py_is(&slot, "EventMobSlot") {
                if let Some(track) = self.nested(|s| s.transcribe(&slot, chain, Some(rate)))? {
                    self.document.append_child(stack, track)?;
                }
                continue;
            }
            if !self.py_is(&slot, "TimelineMobSlot") {
                continue;
            }

            if self.log.is_some() {
                let label = bytes_repr(&self.get_name(&slot)?);
                self.log(|| format!("Creating Track for TimelineMobSlot for {label}"));
            }
            let slot_metadata = self.object_properties(&slot)?;
            let kind = match self.value_of(&slot, "Segment")? {
                Py::Object(segment) => track_kind(self.media_kind(&segment)?.as_deref()),
                _ => track_kind(None),
            };
            let slot_name = match self.value_of(&slot, "SlotName")? {
                Py::Str(name) => name,
                _ => String::new(),
            };

            let mut children = Vec::new();
            for group in self.slot_essence_groups(&slot)? {
                let mut items = Vec::new();
                // Upstream names the group in its log by the last component
                // its loop reached, which is what Python leaves bound.
                let last = group.last().cloned();
                for component in group {
                    let length = self.length(&component)?.unwrap_or_default();
                    if length == 0 {
                        continue;
                    }
                    let media_kind = self.media_kind(&component)?;
                    if !self.py_is(&component, "SourceClip") {
                        let mut gap = item_fields();
                        gap.source_range =
                            Some(TimeRange::new(frames(0, rate), frames(length, rate)));
                        items.push(self.document.insert(Node::Gap(Gap { item: gap })));
                        continue;
                    }
                    items.extend(self.clips_from(
                        mob,
                        &slot,
                        &component,
                        metadata,
                        media_kind,
                        global_start,
                    )?);
                }
                // Only the first choice of an essence group is kept.
                if let Some(first) = items.first() {
                    if let Some(last) = last.as_ref().filter(|_| self.log.is_some()) {
                        let kind = match self.document.get(*first) {
                            Some(Node::Gap(_)) => "Gap",
                            _ => "Clip",
                        };
                        let label = bytes_repr(&self.get_name(last)?);
                        self.log_at(self.indent + 2, || format!("Creating {kind} for {label}"));
                    }
                    children.push(*first);
                }
            }

            let track = self.track_of(item(slot_name, wrap(slot_metadata)), children, kind)?;
            self.document.append_child(stack, track)?;
        }

        // A master mob has no transitions, so its markers can be attached
        // straight away.
        self.attach_markers(timeline)?;
        Ok(timeline)
    }

    /// The clips one source clip in a master mob's slot makes.
    ///
    /// Normally one: the chain reaches the master mob once. The media
    /// references gathered on the way are reversed before they are used,
    /// because they were gathered from the far end.
    fn clips_from(
        &mut self,
        mob: &Object,
        slot: &Object,
        component: &Object,
        metadata: &AnyDictionary,
        media_kind: Option<String>,
        global_start: Option<TimeRange>,
    ) -> Result<Vec<NodeId>> {
        let chain = self.reference_chain(mob, slot, component)?;
        let mut in_range: Option<TimeRange> = None;
        let mut references: Vec<(String, NodeId)> = Vec::new();
        let mut clips = Vec::new();

        for step in chain.iter().rev() {
            let is_source = self.py_is(&step.mob, "SourceMob");
            // A master mob's own timecode is the global offset rather than a
            // start within its media, so it is not applied again here.
            let start_tc = if is_source {
                self.mob_start_timecode(&step.mob)?
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
            } else if self.py_is(&step.mob, "MasterMob") {
                references.reverse();
                let clip = self.master_mob_clip(
                    &step.mob,
                    metadata,
                    available,
                    &mut references,
                    global_start,
                )?;
                let mut aaf = metadata.clone();
                aaf.insert(
                    "MediaKind".to_owned(),
                    media_kind.clone().map_or(Any::Null, Any::String),
                );
                if let Some(base) = self.document.get_mut(clip).and_then(Node::base_mut) {
                    base.metadata.insert("AAF".to_owned(), Any::Dictionary(aaf));
                }
                clips.push(clip);
            }
            in_range = Some(available);
        }
        Ok(clips)
    }

    /// The chain of mobs a source clip leads through.
    fn reference_chain(&mut self, mob: &Object, slot: &Object, clip: &Object) -> Result<Vec<Step>> {
        let mut chain = Vec::new();
        let mut step = Step {
            mob: mob.clone(),
            slot: slot.clone(),
            clip: clip.clone(),
        };
        loop {
            let next_mob = self.mob_of(&step.clip)?;
            let next_slot = match &next_mob {
                Some(next_mob) => self.slot_named_by(next_mob, &step.clip)?,
                None => None,
            };
            chain.push(step);
            if chain.len() > MAX_CHAIN {
                break;
            }
            let (Some(next_mob), Some(next_slot)) = (next_mob, next_slot) else {
                break;
            };
            let Some(next_clip) = self
                .slot_components(&next_slot)?
                .into_iter()
                .find(|component| self.py_is(component, "SourceClip"))
            else {
                break;
            };
            step = Step {
                mob: next_mob,
                slot: next_slot,
                clip: next_clip,
            };
        }
        Ok(chain)
    }

    /// A slot's components, with an essence group standing for its choices.
    fn slot_essence_groups(&mut self, slot: &Object) -> Result<Vec<Vec<Object>>> {
        let mut out = Vec::new();
        for component in self.slot_components(slot)? {
            if self.py_is(&component, "EssenceGroup") {
                out.push(self.aaf.children(&component, "Choices")?);
            } else {
                out.push(vec![component]);
            }
        }
        Ok(out)
    }

    /// A source mob's files, as media references.
    fn source_mob_references(
        &mut self,
        mob: &Object,
        available: TimeRange,
        global_start: Option<TimeRange>,
    ) -> Result<Vec<(String, NodeId)>> {
        let metadata = self.object_properties(mob)?;
        let mut urls = Vec::new();
        if let Py::Object(descriptor) = self.value_of(mob, "EssenceDescription")? {
            if let Py::List(locators) = self.value_of(&descriptor, "Locator")? {
                for locator in locators {
                    let Py::Object(locator) = locator else {
                        continue;
                    };
                    if let Py::Str(url) = self.value_of(&locator, "URLString")? {
                        if !url.is_empty() {
                            urls.push(file_url(&url));
                        }
                    }
                }
            }
        }
        let name = match self.value_of(mob, "Name")? {
            Py::Str(name) if !name.is_empty() => name,
            _ => self
                .mob_id_of(mob)?
                .map(|id| id.to_string())
                .unwrap_or_default(),
        };
        let available = shift(available, global_start);

        let targets: Vec<Option<String>> = if urls.is_empty() {
            vec![None]
        } else {
            urls.into_iter().map(Some).collect()
        };
        let label = if self.log.is_some() {
            bytes_repr(&self.get_name(mob)?)
        } else {
            String::new()
        };
        let mut out = Vec::new();
        for target in targets {
            let kind = if target.is_some() {
                "ExternalReference"
            } else {
                "MissingReference"
            };
            self.log_at(self.indent + 2, || {
                format!("Creating {kind} for SourceMob for {label}")
            });
            let media = MediaReferenceData {
                base: Base {
                    name: name.clone(),
                    metadata: wrap(metadata.clone()),
                },
                available_range: Some(available),
                available_image_bounds: None,
            };
            let node = match target {
                Some(target_url) => {
                    self.document
                        .insert(Node::ExternalReference(ExternalReference {
                            media,
                            target_url,
                        }))
                }
                None => self
                    .document
                    .insert(Node::MissingReference(MissingReference { media })),
            };
            out.push((name.clone(), node));
        }
        Ok(out)
    }

    /// The clip a master mob's slot makes, with its references attached.
    fn master_mob_clip(
        &mut self,
        mob: &Object,
        metadata: &AnyDictionary,
        source_range: TimeRange,
        references: &mut Vec<(String, NodeId)>,
        global_start: Option<TimeRange>,
    ) -> Result<NodeId> {
        let source_range = shift(source_range, global_start);
        let name = match self.value_of(mob, "Name")? {
            Py::Str(name) => name,
            _ => String::new(),
        };

        // A path left in the user comments stands in front of every other
        // reference. Upstream calls this custom behaviour it keeps for
        // compatibility.
        let unc_path = match metadata.get("UserComments") {
            Some(Any::Dictionary(comments)) => match comments.get("UNC Path") {
                Some(Any::String(path)) if !path.is_empty() => Some(path.clone()),
                _ => None,
            },
            _ => None,
        };
        if let Some(path) = unc_path {
            let reference = self
                .document
                .insert(Node::ExternalReference(ExternalReference {
                    media: MediaReferenceData {
                        base: Base {
                            name: "UNC Path".to_owned(),
                            metadata: AnyDictionary::new(),
                        },
                        available_range: Some(source_range),
                        available_image_bounds: None,
                    },
                    target_url: file_url(&path),
                }));
            references.insert(0, ("UNC Path".to_owned(), reference));
            self.log_at(self.indent + 2, || {
                "Creating ExternalReference from UserComments for UNC Path".to_owned()
            });
        }

        // Names repeat when two references come off the same mob, so a repeat
        // is numbered rather than overwriting what is already there.
        let mut media_references = BTreeMap::new();
        let mut active = DEFAULT_MEDIA_KEY.to_owned();
        for (index, (name, node)) in references.iter().enumerate() {
            let key = if index == 0 {
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
            media_references.insert(key, *node);
        }
        if media_references.is_empty() {
            // A clip with no references still has OTIO's default: a missing
            // reference under the default key.
            let missing = self
                .document
                .insert(Node::MissingReference(MissingReference {
                    media: MediaReferenceData {
                        base: Base::default(),
                        available_range: None,
                        available_image_bounds: None,
                    },
                }));
            media_references.insert(DEFAULT_MEDIA_KEY.to_owned(), missing);
            active = DEFAULT_MEDIA_KEY.to_owned();
        }

        let mut fields = item_fields();
        fields.base.name = name;
        fields.source_range = Some(source_range);
        Ok(self.document.insert(Node::Clip(Clip {
            item: fields,
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
        let rate = self.edit_rate_of(slot)?.unwrap_or(1.0);
        let start = self
            .value_of(clip, "StartTime")?
            .as_i64()
            .unwrap_or_default();
        let length = self.length(clip)?.unwrap_or_default();
        let mut start = frames(start, rate);
        let mut duration = frames(length, rate);
        if let Some(tc) = start_tc {
            start += tc.start_time().rescaled_to(rate);
            let tc_duration = tc.duration().rescaled_to(rate);
            // Python's `max` keeps the first of two equal values.
            if tc_duration > duration {
                duration = tc_duration;
            }
        }
        if let Some(in_range) = in_range {
            start += in_range.start_time().rescaled_to(rate);
            return Ok(TimeRange::new(start, duration).clamped_range(in_range));
        }
        Ok(TimeRange::new(start, duration))
    }

    /// The slot a source clip names within the mob it points at.
    fn slot_named_by(&mut self, mob: &Object, clip: &Object) -> Result<Option<Object>> {
        let Some(wanted) = self.value_of(clip, "SourceMobSlotID")?.as_i64() else {
            return Ok(None);
        };
        for slot in self.aaf.slots(mob)? {
            if self.value_of(&slot, "SlotID")?.as_i64() == Some(wanted) {
                return Ok(Some(slot));
            }
        }
        Ok(None)
    }
}

/// A range moved along by a mob's own timecode, if it has one.
fn shift(range: TimeRange, by: Option<TimeRange>) -> TimeRange {
    match by {
        Some(by) => TimeRange::new(range.start_time() + by.start_time(), range.duration()),
        None => range,
    }
}

/// A path as a URL, which is what OTIO holds.
///
/// A path that is already a URL is left alone, and the separators of a
/// Windows path are turned round, since a URL has only the one kind.
pub(crate) fn file_url(path: &str) -> String {
    let url = if path.starts_with("file://") {
        path.to_owned()
    } else {
        format!("file://{path}")
    };
    url.replace('\\', "/")
}

//! The walk from AAF objects to OTIO objects.
//!
//! One method per AAF kind, dispatched from [`Transcriber::transcribe`] in the
//! order upstream dispatches them. The order matters: AAF's class hierarchy is
//! deeper than OTIO's, so a `SourceClip` is also a `Segment` and also a
//! `Component`, and asking the questions in the wrong order answers the wrong
//! one.

use std::io::{Read, Seek};

use aaf::{Object, Value};
use opentime::{RationalTime, TimeRange};
use otio_core::schema::{EffectData, Gap, Stack, Timeline, Track};
use otio_core::{Any, AnyDictionary, Node, NodeId};

use crate::error::{Error, Result};
use crate::{Transcriber, item_with, metadata};

/// What an operation group is called when its operation has no name.
const OPERATION_GROUP: &str = "OperationGroup";

/// How deep the walk goes before giving up.
///
/// A source clip is followed to the mob it names, which can name another, and
/// a file is free to describe a cycle. Real files nest a handful deep.
const MAX_DEPTH: usize = 64;

/// The edit rate in force, which a slot sets for everything under it.
type EditRate = Option<f64>;

impl<R: Read + Seek> Transcriber<R> {
    /// Turns one AAF object into one OTIO object, or into nothing.
    ///
    /// `parents` is the chain from the content storage down to this object's
    /// owner, which two of the branches need to know where they are.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, or if what it describes
    /// does not add up — a component whose length disagrees with its range.
    pub(crate) fn transcribe(
        &mut self,
        item: &Object,
        parents: &[Object],
        edit_rate: EditRate,
    ) -> Result<Option<NodeId>> {
        if parents.len() > MAX_DEPTH {
            return Ok(None);
        }
        let mut aaf = metadata::object_properties(self, item)?;
        // Upstream writes a name into the metadata before reading the
        // object's properties, so an object that has no `Name` of its own
        // still records one: its class. A filler is recorded as "Filler".
        let name = self.name_of(item)?;
        aaf.insert("Name".to_owned(), Any::String(name));
        if let Some(kind) = self.media_kind_of(item)? {
            aaf.insert("MediaKind".to_owned(), Any::String(kind));
        }

        // A slot carries the rate everything under it is counted in.
        let edit_rate = match self.aaf.value(item, "EditRate") {
            Ok(Some(value)) => value.as_rational().map_or(edit_rate, |(n, d)| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "an edit rate is a small ratio like 48000/1"
                )]
                Some(n as f64 / d as f64)
            }),
            _ => edit_rate,
        };

        let length = self.length(item)?;
        if let Some(length) = length {
            aaf.insert("Length".to_owned(), Any::Int(length));
        }

        let result = self.dispatch(item, parents, edit_rate, &aaf)?;
        let Some(result) = result else {
            return Ok(None);
        };

        self.finish(result, item, aaf, length)?;
        Ok(Some(result))
    }

    /// Which OTIO object an AAF object becomes.
    ///
    /// The chain is upstream's, in upstream's order. A kind that falls off
    /// the end is ignored, which is how timecode, pulldown and edge code
    /// leave no object behind: they are read for their times elsewhere.
    fn dispatch(
        &mut self,
        item: &Object,
        parents: &[Object],
        edit_rate: EditRate,
        aaf: &AnyDictionary,
    ) -> Result<Option<NodeId>> {
        let name = self.name_of(item)?;
        if self.aaf.is_a(item, "SourceMob") && parents.is_empty() {
            return Err(Error::UnexpectedSourceMob);
        }
        if self.aaf.is_a(item, "MasterMob") || self.aaf.is_a(item, "CompositionMob") {
            return self.transcribe_mob(item, parents, name, aaf).map(Some);
        }
        if self.aaf.is_a(item, "SourceClip") {
            return self.transcribe_source_clip(item, parents, edit_rate, aaf);
        }
        if self.aaf.is_a(item, "Filler") {
            return self.transcribe_filler(item, edit_rate, name, aaf).map(Some);
        }
        if self.aaf.is_a(item, "NestedScope") {
            return self
                .transcribe_nested_scope(item, parents, edit_rate, name, aaf)
                .map(Some);
        }
        if self.aaf.is_a(item, "Sequence") {
            return self
                .transcribe_sequence(item, parents, edit_rate, name, aaf)
                .map(Some);
        }
        if self.aaf.is_a(item, "OperationGroup") {
            return self.transcribe_operation_group(item, parents, edit_rate, aaf);
        }
        if self.aaf.is_a(item, "MobSlot") {
            return self
                .transcribe_slot(item, parents, edit_rate, name, aaf)
                .map(Some);
        }
        Ok(None)
    }

    /// A composition or master mob, as a timeline.
    ///
    /// Reached once per source clip that names the mob, so the result is kept
    /// and handed back on the next ask rather than walked again.
    fn transcribe_mob(
        &mut self,
        mob: &Object,
        parents: &[Object],
        name: String,
        aaf: &AnyDictionary,
    ) -> Result<NodeId> {
        if let Some(id) = self.aaf.mob_id(mob)? {
            if let Some(found) = self.timelines.get(&id) {
                return Ok(*found);
            }
        }

        let chain: Vec<Object> = parents
            .iter()
            .chain(std::iter::once(mob))
            .cloned()
            .collect();
        let tracks = if self.aaf.is_a(mob, "MasterMob") {
            self.master_mob_tracks(mob)?
        } else {
            let mut tracks = Vec::new();
            for slot in self.aaf.slots(mob)? {
                if let Some(track) = self.transcribe(&slot, &chain, None)? {
                    tracks.push(track);
                }
            }
            tracks
        };

        // A timeline's stack is called `tracks`, which is OTIO's own name
        // for it rather than anything the file said.
        let stack = self.document.insert(Node::Stack(Stack {
            item: otio_core::schema::ItemData {
                base: otio_core::schema::Base {
                    name: "tracks".to_owned(),
                    metadata: AnyDictionary::new(),
                },
                ..otio_core::schema::ItemData::new()
            },
            children: tracks,
        }));
        let global_start_time = self.start_timecode(mob)?.map(|range| range.start_time());
        let timeline = self.document.insert(Node::Timeline(Timeline {
            base: otio_core::schema::Base {
                name,
                metadata: wrap(aaf.clone()),
            },
            tracks: Some(stack),
            global_start_time,
        }));
        self.reparent(stack)?;

        if let Some(id) = self.aaf.mob_id(mob)? {
            self.timelines.insert(id, timeline);
        }
        Ok(timeline)
    }

    /// A mob slot, as a track holding whatever its segment became.
    fn transcribe_slot(
        &mut self,
        slot: &Object,
        parents: &[Object],
        edit_rate: EditRate,
        name: String,
        aaf: &AnyDictionary,
    ) -> Result<NodeId> {
        let chain: Vec<Object> = parents
            .iter()
            .chain(std::iter::once(slot))
            .cloned()
            .collect();
        let mut children = Vec::new();
        if let Some(segment) = self.aaf.child(slot, "Segment")? {
            if let Some(child) = self.transcribe(&segment, &chain, edit_rate)? {
                children.push(child);
            }
        }
        let kind = self.track_kind(slot)?;
        let track = self.document.insert(Node::Track(Track {
            item: item_with(name, aaf.clone()),
            children,
            kind,
        }));
        self.reparent(track)?;
        Ok(track)
    }

    /// A sequence, as a track of its components laid end to end.
    fn transcribe_sequence(
        &mut self,
        sequence: &Object,
        parents: &[Object],
        edit_rate: EditRate,
        name: String,
        aaf: &AnyDictionary,
    ) -> Result<NodeId> {
        let chain: Vec<Object> = parents
            .iter()
            .chain(std::iter::once(sequence))
            .cloned()
            .collect();
        let mut children = Vec::new();
        for component in self.aaf.components(sequence)? {
            if let Some(child) = self.transcribe(&component, &chain, edit_rate)? {
                children.push(child);
            }
        }
        let kind = self.track_kind(sequence)?;
        let track = self.document.insert(Node::Track(Track {
            item: item_with(name, aaf.clone()),
            children,
            kind,
        }));
        self.reparent(track)?;
        Ok(track)
    }

    /// A nested scope, as a stack of its slots.
    fn transcribe_nested_scope(
        &mut self,
        scope: &Object,
        parents: &[Object],
        edit_rate: EditRate,
        name: String,
        aaf: &AnyDictionary,
    ) -> Result<NodeId> {
        let chain: Vec<Object> = parents
            .iter()
            .chain(std::iter::once(scope))
            .cloned()
            .collect();
        let mut children = Vec::new();
        for slot in self.aaf.children(scope, "Slots")? {
            if let Some(child) = self.transcribe(&slot, &chain, edit_rate)? {
                children.push(child);
            }
        }
        let stack = self.document.insert(Node::Stack(Stack {
            item: item_with(name, aaf.clone()),
            children,
        }));
        self.reparent(stack)?;
        Ok(stack)
    }

    /// A filler, as a gap of the same length.
    fn transcribe_filler(
        &mut self,
        filler: &Object,
        edit_rate: EditRate,
        name: String,
        aaf: &AnyDictionary,
    ) -> Result<NodeId> {
        let rate = edit_rate.unwrap_or(1.0);
        let length = self.length(filler)?.unwrap_or_default();
        let mut item = item_with(name, aaf.clone());
        item.source_range = Some(TimeRange::new(
            RationalTime::new(0.0, rate),
            #[expect(
                clippy::cast_precision_loss,
                reason = "an AAF length is a frame count, well inside f64"
            )]
            RationalTime::new(length as f64, rate),
        ));
        Ok(self.document.insert(Node::Gap(Gap { item })))
    }

    /// An operation group, as a stack of what it operates on.
    ///
    /// The effect itself carries the group's metadata and sits on the stack,
    /// and each input segment becomes a layer. A segment that did not come
    /// back as a track is wrapped in one, because a stack's layers are
    /// tracks.
    ///
    /// The stack takes its name from the first clip found inside it rather
    /// than from the group, which is upstream's doing: an effect in an Avid
    /// timeline shows the name of the clip it is applied to.
    fn transcribe_operation_group(
        &mut self,
        group: &Object,
        parents: &[Object],
        edit_rate: EditRate,
        aaf: &AnyDictionary,
    ) -> Result<Option<NodeId>> {
        let chain: Vec<Object> = parents
            .iter()
            .chain(std::iter::once(group))
            .cloned()
            .collect();
        let rate = edit_rate.unwrap_or(1.0);
        let length = self.length(group)?.unwrap_or_default();

        // The operation's own name, which the effect and the stack start out
        // with. Most files leave it empty.
        let operation_name = match aaf.get("Operation") {
            Some(Any::Dictionary(operation)) => match operation.get("Name") {
                Some(Any::String(name)) => name.clone(),
                _ => OPERATION_GROUP.to_owned(),
            },
            _ => OPERATION_GROUP.to_owned(),
        };

        let effect = self.document.insert(Node::Effect(EffectData {
            base: otio_core::schema::Base {
                name: operation_name.clone(),
                metadata: wrap(aaf.clone()),
            },
            ..EffectData::new()
        }));

        let mut children = Vec::new();
        for segment in self.aaf.children(group, "InputSegments")? {
            let Some(child) = self.transcribe(&segment, &chain, edit_rate)? else {
                continue;
            };
            children.push(match self.document.get(child) {
                Some(Node::Track(_)) => child,
                _ => {
                    let kind = self.track_kind(&segment)?;
                    let track = self.document.insert(Node::Track(Track {
                        item: otio_core::schema::ItemData::new(),
                        children: vec![child],
                        kind,
                    }));
                    self.reparent(track)?;
                    track
                }
            });
        }

        // The group's own metadata went onto the effect, so the stack keeps
        // none of its own.
        let mut item = item_with(operation_name, AnyDictionary::new());
        item.effects.push(effect);
        #[expect(
            clippy::cast_precision_loss,
            reason = "an AAF length is a frame count, well inside f64"
        )]
        let duration = RationalTime::new(length as f64, rate);
        item.source_range = Some(TimeRange::new(RationalTime::new(0.0, rate), duration));

        let stack = self.document.insert(Node::Stack(Stack { item, children }));
        self.reparent(stack)?;
        if let Some(name) = self.first_clip_name(stack) {
            if let Some(base) = self.document.get_mut(stack).and_then(Node::base_mut) {
                base.name = name;
            }
        }
        Ok(Some(stack))
    }

    /// The name of the first named clip under a node, if there is one.
    fn first_clip_name(&self, root: NodeId) -> Option<String> {
        let node = self.document.get(root)?;
        if let Node::Clip(clip) = node {
            if !clip.item.base.name.is_empty() {
                return Some(clip.item.base.name.clone());
            }
        }
        for child in node.children()?.to_vec() {
            if let Some(name) = self.first_clip_name(child) {
                return Some(name);
            }
        }
        None
    }

    /// A source clip, as whatever the mob it names turns out to be.
    ///
    /// A clip pointing at a master mob is a clip of that mob's media. One
    /// pointing at a composition is that composition nested here, which is a
    /// stack. One pointing at nothing readable is a gap of the right length,
    /// because the edit still occupies that time whether or not the media can
    /// be found.
    fn transcribe_source_clip(
        &mut self,
        clip: &Object,
        parents: &[Object],
        edit_rate: EditRate,
        aaf: &AnyDictionary,
    ) -> Result<Option<NodeId>> {
        let rate = edit_rate.unwrap_or(1.0);
        let length = self.length(clip)?.unwrap_or_default();
        let start = self
            .aaf
            .value(clip, "StartTime")?
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        #[expect(
            clippy::cast_precision_loss,
            reason = "AAF times are frame counts, well inside f64"
        )]
        let source_range = TimeRange::new(
            RationalTime::new(start as f64, rate),
            RationalTime::new(length as f64, rate),
        );

        let target = match self.aaf.value(clip, "SourceID")? {
            Some(Value::MobId(id)) => self.aaf.mob(id)?,
            _ => None,
        };
        let Some(mob) = target else {
            return Ok(Some(self.gap_of(source_range, aaf.clone())));
        };

        let name = self.aaf.name(&mob)?.unwrap_or_default();
        if self.aaf.is_a(&mob, "CompositionMob") {
            let timeline = self.transcribe_mob(&mob, parents, name.clone(), aaf)?;
            let tracks = self
                .document
                .get(timeline)
                .and_then(|node| match node {
                    Node::Timeline(timeline) => timeline.tracks,
                    _ => None,
                })
                .map(|stack| self.clone_subtree(stack))
                .transpose()?;
            let Some(tracks) = tracks else {
                return Ok(Some(self.gap_of(source_range, aaf.clone())));
            };
            let mut item = item_with(name, aaf.clone());
            item.source_range = Some(source_range);
            let children = match self.document.get(tracks) {
                Some(Node::Stack(stack)) => stack.children.clone(),
                _ => Vec::new(),
            };
            let stack = self.document.insert(Node::Stack(Stack { item, children }));
            self.reparent(stack)?;
            return Ok(Some(stack));
        }

        if self.aaf.is_a(&mob, "MasterMob") {
            // The clip stands for one slot of that mob, so what comes back is
            // that slot's track with this clip's span on it, rather than a
            // new object: the mob already knows what media the slot holds.
            let Some(track) = self.slot_track_of(&mob, clip, parents, &name, aaf)? else {
                return Ok(Some(self.gap_of(source_range, aaf.clone())));
            };
            if let Some(item) = self.document.get_mut(track).and_then(Node::item_mut) {
                item.source_range = Some(source_range);
            }
            return Ok(Some(track));
        }

        Ok(Some(self.gap_of(source_range, aaf.clone())))
    }

    /// A copy of the track a source clip's slot became in its mob.
    fn slot_track_of(
        &mut self,
        mob: &Object,
        clip: &Object,
        parents: &[Object],
        name: &str,
        aaf: &AnyDictionary,
    ) -> Result<Option<NodeId>> {
        let Some(wanted) = self
            .aaf
            .value(clip, "SourceMobSlotID")
            .ok()
            .flatten()
            .and_then(|value| value.as_i64())
        else {
            return Ok(None);
        };
        let timeline = self.transcribe_mob(mob, parents, name.to_owned(), aaf)?;
        let Some(Node::Timeline(timeline)) = self.document.get(timeline) else {
            return Ok(None);
        };
        let Some(stack) = timeline.tracks else {
            return Ok(None);
        };
        let Some(Node::Stack(stack)) = self.document.get(stack) else {
            return Ok(None);
        };
        let found = stack
            .children
            .clone()
            .into_iter()
            .find(|track| self.slot_id_of(*track) == Some(wanted));
        let Some(found) = found else {
            return Ok(None);
        };
        self.clone_subtree(found).map(Some)
    }

    /// The slot identifier a transcribed track remembers it came from.
    fn slot_id_of(&self, track: NodeId) -> Option<i64> {
        let Some(Any::Dictionary(aaf)) = self.document.get(track)?.base()?.metadata.get("AAF")
        else {
            return None;
        };
        match aaf.get("SlotID") {
            Some(Any::Int(id)) => Some(*id),
            Some(Any::UInt(id)) => i64::try_from(*id).ok(),
            _ => None,
        }
    }

    /// A gap occupying a span, carrying the metadata of what it replaced.
    fn gap_of(&mut self, range: TimeRange, aaf: AnyDictionary) -> NodeId {
        let mut item = item_with(String::new(), aaf);
        item.source_range = Some(range);
        self.document.insert(Node::Gap(Gap { item }))
    }

    /// The finishing every transcribed object gets.
    ///
    /// The name falls back to the one in metadata, the metadata is attached
    /// if the branch did not attach its own, and a component's length is
    /// checked against the range that came out. That last one is a check on
    /// this crate rather than on the file: both numbers come from the same
    /// place, so them disagreeing means the range was built wrongly.
    fn finish(
        &mut self,
        result: NodeId,
        item: &Object,
        aaf: AnyDictionary,
        length: Option<i64>,
    ) -> Result<()> {
        let name = self.name_of(item)?;
        if let Some(node) = self.document.get_mut(result) {
            if let Some(base) = node.base_mut() {
                if base.name.is_empty() {
                    base.name = name;
                }
                if !base.metadata.contains_key("AAF") {
                    base.metadata.insert("AAF".to_owned(), Any::Dictionary(aaf));
                }
            }
        }

        let range = self
            .document
            .get(result)
            .and_then(Node::item)
            .and_then(|item| item.source_range);
        if let (Some(length), Some(range)) = (length, range) {
            #[expect(
                clippy::cast_precision_loss,
                reason = "an AAF length is a frame count, well inside f64"
            )]
            let expected = length as f64;
            if (range.duration().value() - expected).abs() > f64::EPSILON {
                return Err(Error::WrongDuration {
                    found: range.duration().value(),
                    expected: length,
                });
            }
        }
        Ok(())
    }

    /// What an object is called, falling back to its class name.
    ///
    /// AAF leaves most things unnamed, and upstream uses the class name
    /// rather than an empty string so that a track in a viewer still says
    /// what it is.
    pub(crate) fn name_of(&mut self, item: &Object) -> Result<String> {
        if let Ok(Some(name)) = self.aaf.name(item) {
            if !name.is_empty() {
                return Ok(name);
            }
        }
        Ok(self.aaf.class_name(item).unwrap_or_default().to_owned())
    }

    /// A component's length, if it has one.
    pub(crate) fn length(&mut self, item: &Object) -> Result<Option<i64>> {
        Ok(self
            .aaf
            .value(item, "Length")
            .ok()
            .flatten()
            .and_then(|value| value.as_i64()))
    }

    /// What an object carries, in AAF's own vocabulary.
    ///
    /// A component says so itself, through its data definition. A slot has no
    /// data definition of its own and says so through its segment, which is
    /// how a track ends up knowing whether it is picture or sound.
    pub(crate) fn media_kind_of(&mut self, item: &Object) -> Result<Option<String>> {
        if let Ok(Some(kind)) = self.aaf.media_kind(item) {
            return Ok(Some(kind));
        }
        // A class with no `Segment` at all is not a slot, which is not an
        // error: it simply carries nothing.
        let Some(segment) = self.aaf.child(item, "Segment").ok().flatten() else {
            return Ok(None);
        };
        Ok(self.aaf.media_kind(&segment).ok().flatten())
    }

    /// What a track carries, in OTIO's vocabulary.
    ///
    /// OTIO names two kinds, and AAF names more than two. The ones with no
    /// OTIO name keep AAF's, prefixed, rather than being forced into one of
    /// the two or dropped.
    pub(crate) fn track_kind(&mut self, item: &Object) -> Result<String> {
        let media_kind = self.media_kind_of(item)?.unwrap_or_default();
        Ok(match media_kind.as_str() {
            "Picture" => "Video".to_owned(),
            "Sound" | "SoundMasterTrack" => "Audio".to_owned(),
            "" => String::new(),
            other => format!("AAF_{other}"),
        })
    }

    /// Where a mob's timecode says it starts.
    ///
    /// A mob can carry several timecode slots; the one on physical track 1 is
    /// the one that means the edit's own start, which is upstream's heuristic
    /// and the only one the format supports.
    pub(crate) fn start_timecode(&mut self, mob: &Object) -> Result<Option<TimeRange>> {
        for slot in self.aaf.slots(mob)? {
            let track = self
                .aaf
                .value(&slot, "PhysicalTrackNumber")
                .ok()
                .flatten()
                .and_then(|value| value.as_i64());
            if track != Some(1) {
                continue;
            }
            let Some(segment) = self.aaf.child(&slot, "Segment")? else {
                continue;
            };
            let timecode = if self.aaf.is_a(&segment, "Sequence") {
                self.aaf
                    .components(&segment)?
                    .into_iter()
                    .find(|component| self.aaf.is_a(component, "Timecode"))
            } else if self.aaf.is_a(&segment, "Timecode") {
                Some(segment)
            } else {
                None
            };
            let Some(timecode) = timecode else { continue };

            let rate = self
                .aaf
                .value(&slot, "EditRate")?
                .and_then(|value| value.as_rational())
                .map_or(1.0, |(n, d)| {
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "an edit rate is a small ratio like 24/1"
                    )]
                    let rate = n as f64 / d as f64;
                    rate
                });
            let start = self
                .aaf
                .value(&timecode, "Start")?
                .and_then(|value| value.as_i64())
                .unwrap_or_default();
            let length = self.length(&timecode)?.unwrap_or_default();
            #[expect(
                clippy::cast_precision_loss,
                reason = "AAF times are frame counts, well inside f64"
            )]
            let range = TimeRange::new(
                RationalTime::new(start as f64, rate),
                RationalTime::new(length as f64, rate),
            );
            return Ok(Some(range));
        }
        Ok(None)
    }

    /// Records a composition as its children's parent.
    pub(crate) fn reparent(&mut self, parent: NodeId) -> Result<()> {
        let children = match self.document.get(parent) {
            Some(Node::Track(track)) => track.children.clone(),
            Some(Node::Stack(stack)) => stack.children.clone(),
            _ => return Ok(()),
        };
        for child in children {
            if let Some(node) = self.document.get_mut(child) {
                node.set_parent(Some(parent));
            }
        }
        Ok(())
    }

    /// A deep copy of a subtree, so one mob can appear in two places.
    ///
    /// A mob named by two source clips is transcribed once, and each clip
    /// needs its own copy: the arena holds one object per identifier, and two
    /// parents cannot share one child.
    fn clone_subtree(&mut self, root: NodeId) -> Result<NodeId> {
        self.clone_within(root, 0)
    }

    fn clone_within(&mut self, root: NodeId, depth: usize) -> Result<NodeId> {
        if depth > MAX_DEPTH {
            return Ok(root);
        }
        let node = self
            .document
            .get(root)
            .ok_or(otio_core::Error::StaleHandle)?
            .clone();
        let children = match &node {
            Node::Track(track) => track.children.clone(),
            Node::Stack(stack) => stack.children.clone(),
            _ => Vec::new(),
        };
        let copied: Vec<NodeId> = children
            .into_iter()
            .map(|child| self.clone_within(child, depth + 1))
            .collect::<Result<_>>()?;

        let copy = self.document.insert(match node {
            Node::Track(mut track) => {
                track.children = copied;
                Node::Track(track)
            }
            Node::Stack(mut stack) => {
                stack.children = copied;
                Node::Stack(stack)
            }
            other => other,
        });
        self.reparent(copy)?;
        Ok(copy)
    }
}

/// An `AAF` metadata dictionary wrapped as an object's whole metadata.
pub(crate) fn wrap(aaf: AnyDictionary) -> AnyDictionary {
    let mut metadata = AnyDictionary::new();
    metadata.insert("AAF".to_owned(), Any::Dictionary(aaf));
    metadata
}

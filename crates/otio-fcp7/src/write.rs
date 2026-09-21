//! Writing an OTIO document out as FCP 7 XML.
//!
//! The shape of the file comes from the document, and the detail comes from
//! whatever the reader stashed under `metadata["fcp_xml"]`. Anything the
//! writer computes — timings, rates, timecode, `id` attributes — overrides
//! what the metadata says, because those are the things that go stale as soon
//! as the edit changes.

use std::collections::BTreeMap;

use opentime::{DropFrame, RationalTime, TimeRange};
use otio_adapter::{Error, Result};
use otio_core::schema::{Node, Transition};
use otio_core::{AnyDictionary, Document, NodeId};
use otio_xml::Element;

use crate::dict::{dict_str, dict_sub, dict_to_xml_tree, fcp_metadata};
use crate::util::{format_frames, frame_text, url_basename, url_path};

/// Suffixes FCP 7 treats as audio-only, so a file with one gets no `video`
/// block.
const AUDIO_SUFFIXES: [&str; 6] = [".wav", ".aac", ".mp3", ".aif", ".aiff", ".m4a"];

/// The transition effect written when the document does not name one.
const DEFAULT_TRANSITION_EFFECT: &str = "Cross Dissolve";

/// Writes a document as FCP 7 XML.
///
/// The root must be a `Timeline`, or a `SerializableCollection` of them, since
/// a sequence is the only thing the format carries.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] if the root is something else, or if a track
/// holds an item the format cannot express.
pub fn write_to_string(document: &Document) -> Result<String> {
    let root = document
        .root()
        .ok_or_else(|| Error::unsupported("the document has no root to write"))?;
    let node = document.try_get(root)?;

    let mut project = Element::new("project");
    project.push_text("name", node.name());
    let mut children = Element::new("children");

    let mut writer = Writer {
        document,
        references: BTreeMap::new(),
    };

    match node {
        Node::Timeline(timeline) => {
            let range = writer.timeline_range(timeline.tracks, timeline.global_start_time)?;
            children.push(writer.sequence_for_timeline(root, range)?);
        }
        Node::SerializableCollection(collection) => {
            for &child in &collection.children {
                let Node::Timeline(timeline) = document.try_get(child)? else {
                    continue;
                };
                let range = writer.timeline_range(timeline.tracks, timeline.global_start_time)?;
                children.push(writer.sequence_for_timeline(child, range)?);
            }
        }
        other => {
            return Err(Error::unsupported(format!(
                "cannot write a {} as FCP 7 XML: expected a Timeline or a \
                 SerializableCollection of them",
                other.schema_name()
            )));
        }
    }

    project.push(children);

    let mut tree = Element::new("xmeml");
    tree.attributes.set("version", "4");
    tree.push(project);

    Ok(otio_xml::to_pretty_string(&tree))
}

/// The `id` values handed out so far, per tag.
///
/// FCP XML writes an object once and refers to it by `id` everywhere after.
/// Equal objects share an `id`, which is what keeps a file from repeating the
/// same media reference on every clip that uses it.
type References = BTreeMap<&'static str, Vec<(String, u32)>>;

struct Writer<'a> {
    document: &'a Document,
    references: References,
}

/// What a back-referenced element needs before it is built.
struct Reference {
    /// The `id` attribute to put on the element.
    id: String,
    /// Whether this is the first time the object has been written, and so
    /// whether the element needs its full contents.
    is_new: bool,
}

impl Writer<'_> {
    /// Decides the `id` for an object, and whether it still needs writing.
    ///
    /// Two objects count as the same when they serialize identically, which is
    /// what upstream hashes. A media reference is compared by its URL alone,
    /// so the same file used with different metadata still writes once.
    fn reference_for(&mut self, id: NodeId, tag: &'static str) -> Result<Reference> {
        let node = self.document.try_get(id)?;
        let key = match node {
            Node::ExternalReference(reference) => reference.target_url.clone(),
            _ => otio_core::to_string_pretty_from(self.document, id, otio_core::DEFAULT_INDENT)?,
        };

        let assigned = self.references.entry(tag).or_default();
        if let Some((_, number)) = assigned.iter().find(|(existing, _)| *existing == key) {
            return Ok(Reference {
                id: format!("{tag}-{number}"),
                is_new: false,
            });
        }

        // Keep the id the file came in with where it is still free, so that a
        // read and a write do not renumber a file a person may be diffing.
        let preferred = node
            .base()
            .and_then(|base| fcp_metadata(&base.metadata))
            .and_then(|metadata| dict_str(metadata, "@id"))
            .and_then(|id| parse_reference_id(id, tag));

        let taken: Vec<u32> = assigned.iter().map(|(_, number)| *number).collect();
        let number = match preferred {
            Some(preferred) if !taken.contains(&preferred) => preferred,
            // The lowest number not yet handed out, so the file stays compact.
            _ => (1..=taken.iter().copied().max().unwrap_or(0) + 1)
                .find(|candidate| !taken.contains(candidate))
                .expect("the range is one longer than the set of taken numbers"),
        };

        assigned.push((key, number));
        Ok(Reference {
            id: format!("{tag}-{number}"),
            is_new: true,
        })
    }

    /// The span a timeline covers, which is what its sequence is written
    /// against.
    ///
    /// A timeline with no `global_start_time` is written as if it started at
    /// zero. Upstream has no answer here and fails.
    ///
    /// The zero is taken at the tracks' own rate rather than at whatever a
    /// bare default carries, because this start time is what the sequence and
    /// every track in it are then written against. A zero at one frame per
    /// second would round every item boundary in the file to a whole second.
    fn timeline_range(
        &self,
        tracks: Option<NodeId>,
        global_start_time: Option<RationalTime>,
    ) -> Result<TimeRange> {
        let duration = match tracks {
            Some(tracks) => self.document.duration(tracks)?,
            None => RationalTime::default(),
        };
        let start = global_start_time.unwrap_or(RationalTime::new(0.0, duration.rate()));
        Ok(TimeRange::new(start, duration))
    }

    fn sequence_for_timeline(&mut self, id: NodeId, range: TimeRange) -> Result<Element> {
        let reference = self.reference_for(id, "sequence")?;
        if !reference.is_new {
            return Ok(stub("sequence", &reference.id));
        }

        let Node::Timeline(timeline) = self.document.try_get(id)? else {
            unreachable!("only a timeline reaches here");
        };

        let mut element = element_with_metadata("sequence", self.metadata_of(id)?);
        self.add_stack_elements(timeline.tracks, &mut element, range)?;

        // The stack's name is an implementation detail; the timeline's is the
        // one a person gave it.
        if !timeline.base.name.is_empty() {
            if let Some(name) = element.find_mut("name") {
                name.text = Some(timeline.base.name.clone());
            }
        }

        if let Some(start) = timeline.global_start_time {
            let timecode_metadata = self
                .metadata_of(id)?
                .and_then(|metadata| dict_sub(metadata, "timecode"));
            element.push(timecode_from_metadata(start, timecode_metadata)?);
        }

        element.attributes.set("id", reference.id);
        Ok(element)
    }

    fn sequence_for_stack(&mut self, id: NodeId, range: TimeRange) -> Result<Element> {
        let reference = self.reference_for(id, "sequence")?;
        if !reference.is_new {
            return Ok(stub("sequence", &reference.id));
        }

        let mut element = element_with_metadata("sequence", self.metadata_of(id)?);
        self.add_stack_elements(Some(id), &mut element, range)?;
        element.attributes.set("id", reference.id);
        Ok(element)
    }

    /// Fills in the body every sequence has, whether it came from a timeline
    /// or from a nested stack.
    fn add_stack_elements(
        &mut self,
        stack: Option<NodeId>,
        element: &mut Element,
        range: TimeRange,
    ) -> Result<()> {
        let name = match stack {
            Some(stack) => self.document.try_get(stack)?.name().to_string(),
            None => String::new(),
        };
        element.push_text("name", name);
        element.push_text("duration", frame_text(range.duration()));
        element.push(build_rate(range.start_time().rate()));
        let track_rate = range.start_time().rate();

        let media = element.get_or_create_child("media");
        let mut video = take_child(media, "video");
        let audio = take_child(media, "audio");
        // Resolve refuses a sequence whose `video` block has no `format`,
        // even an empty one. See OpenTimelineIO issue 839.
        video.get_or_create_child("format");
        // The `media` block must hold exactly video then audio, in that order,
        // because the back-references written later assume the video tracks
        // come first in the file.
        *media = Element::new("media");

        let (mut video, mut audio) = (video, audio);
        if let Some(stack) = stack {
            let children = self.document.children_of(stack)?;
            for track in children {
                let Node::Track(data) = self.document.try_get(track)? else {
                    continue;
                };
                let kind = data.kind.clone();
                let built = self.top_level_track(track, track_rate)?;
                match kind.as_str() {
                    "Video" => video.push(built),
                    "Audio" => audio.push(built),
                    // A track of some other kind has nowhere to go in this
                    // format, as upstream also has it.
                    _ => {}
                }
            }
        }

        let media = element.get_or_create_child("media");
        media.push(video);
        media.push(audio);

        if let Some(stack) = stack {
            let markers = self
                .document
                .try_get(stack)?
                .item()
                .map(|item| item.markers.clone())
                .unwrap_or_default();
            for marker in markers {
                let built = self.marker(marker)?;
                element.push(built);
            }
        }

        Ok(())
    }

    fn top_level_track(&mut self, track: NodeId, track_rate: f64) -> Result<Element> {
        let mut element = element_with_metadata("track", self.metadata_of(track)?);
        if let Some(item) = self.document.try_get(track)?.item() {
            apply_enabled(&mut element, item.enabled);
        }
        let children = self.document.children_of(track)?;

        for (index, &item) in children.iter().enumerate() {
            let node = self.document.try_get(item)?;
            if matches!(node, Node::Gap(_)) {
                // A gap is implied by the `start` of whatever follows it.
                continue;
            }

            let mut offsets: [Option<RationalTime>; 2] = [None, None];
            if !matches!(node, Node::Transition(_)) {
                if index > 0 {
                    if let Node::Transition(previous) =
                        self.document.try_get(children[index - 1])?
                    {
                        // A transition that reaches into this item at all is
                        // what makes its `start` be written as -1.
                        offsets[0] =
                            (previous.out_offset.value() != 0.0).then_some(previous.in_offset);
                    }
                }
                if let Some(&next) = children.get(index + 1) {
                    if let Node::Transition(next) = self.document.try_get(next)? {
                        offsets[1] = (next.in_offset.value() != 0.0).then_some(next.out_offset);
                    }
                }
            }

            let range = self
                .document
                .range_of_child_at_index(track, i64::try_from(index).unwrap_or(i64::MAX))?;
            let range = TimeRange::new(
                range.start_time().rescaled_to(track_rate),
                range.duration().rescaled_to(track_rate),
            );

            let built = self.item(item, range, offsets)?;
            element.push(built);
        }

        Ok(element)
    }

    fn item(
        &mut self,
        id: NodeId,
        range: TimeRange,
        offsets: [Option<RationalTime>; 2],
    ) -> Result<Element> {
        match self.document.try_get(id)? {
            Node::Transition(_) => self.transition_item(id, range),
            Node::Clip(clip) => {
                let reference = clip.media_references.get(&clip.active_media_reference_key);
                let is_missing = match reference {
                    None => true,
                    Some(&reference) => {
                        matches!(self.document.try_get(reference)?, Node::MissingReference(_))
                    }
                };
                if is_missing {
                    self.clip_item_without_media(id, range, offsets)
                } else {
                    self.clip_item(id, range, offsets)
                }
            }
            Node::Stack(_) => self.track_item(id, range, offsets),
            other => Err(Error::unsupported(format!(
                "cannot write a {} onto an FCP 7 XML track",
                other.schema_name()
            ))),
        }
    }

    fn transition_item(&mut self, id: NodeId, range: TimeRange) -> Result<Element> {
        let metadata = self.metadata_of(id)?.cloned();
        let Node::Transition(transition) = self.document.try_get(id)? else {
            unreachable!("only a transition reaches here");
        };
        let transition: &Transition = transition;

        let mut element = element_with_metadata("transitionitem", metadata.as_ref());
        element.push_text("start", frame_text(range.start_time()));
        element.push_text("end", frame_text(range.end_time_exclusive()));

        if element.find("alignment").is_none() {
            // A transition with nothing on one side is aligned to black there;
            // anything else straddles the cut.
            let alignment = if transition.in_offset.value() == 0.0 {
                "start-black"
            } else if transition.out_offset.value() == 0.0 {
                "end-black"
            } else {
                "center"
            };
            element.push_text("alignment", alignment);
        }

        element.push(build_rate(range.start_time().rate()));

        if element.find("effect").is_none_or(Element::is_empty) {
            let effect_id = metadata
                .as_ref()
                .and_then(|metadata| fcp_metadata(metadata))
                .and_then(|metadata| dict_str(metadata, "effectid"))
                .unwrap_or(DEFAULT_TRANSITION_EFFECT)
                .to_string();

            let mut effect = Element::new("effect");
            effect.push_text("name", transition.base.name.clone());
            effect.push_text("effectid", effect_id);
            effect.push_text("effecttype", "transition");
            effect.push_text("mediatype", "video");
            element.push(effect);
        }

        Ok(element)
    }

    fn clip_item(
        &mut self,
        id: NodeId,
        range: TimeRange,
        offsets: [Option<RationalTime>; 2],
    ) -> Result<Element> {
        let reference = self.reference_for(id, "clipitem")?;
        if !reference.is_new {
            return Ok(stub("clipitem", &reference.id));
        }

        let media_reference = self.active_media_reference(id)?;
        let media_node = match media_reference {
            Some(media) => Some(self.document.try_get(media)?),
            None => None,
        };
        let is_generator = matches!(media_node, Some(Node::GeneratorReference(_)));

        // Premiere writes its own generators as a clip item with a
        // `mediaSource` rather than as an FCP 7 `generatoritem`, and both read
        // back as a generator reference. The `effecttype` field is what says
        // the file really had a `generatoritem`.
        let is_generator_item = is_generator
            && media_node
                .and_then(Node::base)
                .and_then(|base| fcp_metadata(&base.metadata))
                .is_some_and(|metadata| metadata.contains_key("effecttype"));

        let tag = if is_generator_item {
            "generatoritem"
        } else {
            "clipitem"
        };
        let mut element = element_with_metadata(tag, self.metadata_of(id)?);
        if let Some(item) = self.document.try_get(id)?.item() {
            apply_enabled(&mut element, item.enabled);
        }
        for filter_element in self.filters(id)? {
            element.push(filter_element);
        }
        if !element.attributes.contains("frameBlend") {
            element.attributes.set("frameBlend", "FALSE");
        }

        if is_generator_item {
            let built = self.generator_effect(id)?;
            element.push(built);
        } else {
            let media = media_reference.expect("a clip with media reaches here");
            let built = self.file(media)?;
            element.push(built);
        }

        let node = self.document.try_get(id)?;
        element.push_text("name", node.name());

        let source_range = self.source_range_of(id)?;
        let available_range = media_reference
            .and_then(|media| self.document.get(media))
            .and_then(Node::media)
            .and_then(|media| media.available_range);

        if available_range.is_some() {
            element.push(build_rate(source_range.start_time().rate()));
        }
        let markers = self.markers_of(id)?;
        for marker in markers {
            let built = self.marker(marker)?;
            element.push(built);
        }

        let timecode = available_range.map_or_else(
            || RationalTime::new(0.0, source_range.start_time().rate()),
            TimeRange::start_time,
        );
        build_item_timings(&mut element, source_range, range, offsets, timecode);

        element.attributes.set("id", reference.id);
        Ok(element)
    }

    /// Writes a clip whose media is not where it says it is.
    ///
    /// The file element is a stub: enough for the host application to show a
    /// slot at the right place on the timeline, with nothing to point it at.
    fn clip_item_without_media(
        &mut self,
        id: NodeId,
        range: TimeRange,
        offsets: [Option<RationalTime>; 2],
    ) -> Result<Element> {
        let reference = self.reference_for(id, "clipitem")?;
        if !reference.is_new {
            return Ok(stub("clipitem", &reference.id));
        }

        let mut element = element_with_metadata("clipitem", self.metadata_of(id)?);
        if let Some(item) = self.document.try_get(id)?.item() {
            apply_enabled(&mut element, item.enabled);
        }
        for filter_element in self.filters(id)? {
            element.push(filter_element);
        }
        if !element.attributes.contains("frameBlend") {
            element.attributes.set("frameBlend", "FALSE");
        }

        let media_reference = self.active_media_reference(id)?;
        let available_range = media_reference
            .and_then(|media| self.document.get(media))
            .and_then(Node::media)
            .and_then(|media| media.available_range);
        let media_start = available_range.map_or_else(
            || RationalTime::new(0.0, range.start_time().rate()),
            TimeRange::start_time,
        );

        element.push_text("name", self.document.try_get(id)?.name());
        if let Some(media) = media_reference {
            let built = self.empty_file(media, range)?;
            element.push(built);
        }
        let markers = self.markers_of(id)?;
        for marker in markers {
            let built = self.marker(marker)?;
            element.push(built);
        }

        build_item_timings(
            &mut element,
            self.source_range_of(id)?,
            range,
            offsets,
            media_start,
        );

        element.attributes.set("id", reference.id);
        Ok(element)
    }

    /// Writes a nested stack as a clip item holding a sequence of its own.
    fn track_item(
        &mut self,
        id: NodeId,
        range: TimeRange,
        offsets: [Option<RationalTime>; 2],
    ) -> Result<Element> {
        let reference = self.reference_for(id, "clipitem")?;
        if !reference.is_new {
            return Ok(stub("clipitem", &reference.id));
        }

        let mut element = element_with_metadata("clipitem", self.metadata_of(id)?);
        if let Some(item) = self.document.try_get(id)?.item() {
            apply_enabled(&mut element, item.enabled);
        }
        for filter_element in self.filters(id)? {
            element.push(filter_element);
        }
        if !element.attributes.contains("frameBlend") {
            element.attributes.set("frameBlend", "FALSE");
        }

        let name = self.document.try_get(id)?.name().to_string();
        element.push_text("name", basename(&name));

        let sequence = self.sequence_for_stack(id, range)?;
        let source_range = self.source_range_of(id)?;
        element.push(build_rate(source_range.start_time().rate()));
        let markers = self.markers_of(id)?;
        for marker in markers {
            let built = self.marker(marker)?;
            element.push(built);
        }
        element.push(sequence);

        build_item_timings(
            &mut element,
            source_range,
            range,
            offsets,
            RationalTime::new(0.0, range.start_time().rate()),
        );

        element.attributes.set("id", reference.id);
        Ok(element)
    }

    fn file(&mut self, id: NodeId) -> Result<Element> {
        let reference = self.reference_for(id, "file")?;
        if !reference.is_new {
            return Ok(stub("file", &reference.id));
        }

        let node = self.document.try_get(id)?;
        let media = node
            .media()
            .ok_or_else(|| Error::unsupported("a clip's media reference is not one"))?;
        let available_range = media.available_range.ok_or_else(|| {
            Error::unsupported(format!(
                "media reference `{}` does not say what range of media is \
                 available, so there is no duration to write",
                media.base.name
            ))
        })?;

        let mut element = element_with_metadata("file", fcp_metadata(&media.base.metadata));

        // `url_path` is also what decides whether the file gets a video block
        // below, so it is worked out once here.
        let mut url = String::new();
        let fallback_name = match node {
            Node::ExternalReference(external) => {
                element.push_text("pathurl", external.target_url.clone());
                url = external.target_url.clone();
                url_basename(&url).to_string()
            }
            Node::ImageSequenceReference(sequence) => {
                let target = abstract_target_url(sequence);
                element.push_text("pathurl", target.clone());
                url = target;
                url_basename(&url).to_string()
            }
            Node::GeneratorReference(generator) => {
                element.push_text("mediaSource", generator.generator_kind.clone());
                generator.generator_kind.clone()
            }
            other => {
                return Err(Error::unsupported(format!(
                    "cannot write a {} as an FCP 7 XML file element",
                    other.schema_name()
                )));
            }
        };

        let name = if media.base.name.is_empty() {
            fallback_name
        } else {
            media.base.name.clone()
        };
        element.push_text("name", name);

        element.push(build_rate(available_range.start_time().rate()));
        element.push_text("duration", frame_text(available_range.duration()));
        element.push(timecode_from_metadata(
            available_range.start_time(),
            fcp_metadata(&media.base.metadata).and_then(|metadata| dict_sub(metadata, "timecode")),
        )?);

        // Without a media block naming what the file carries, FCP will not
        // recognize it at all.
        if element.find("media").is_none_or(Element::is_empty) {
            let block = element.get_or_create_child("media");
            let suffix = suffix_of(url_path(&url));
            if !AUDIO_SUFFIXES.contains(&suffix.as_str()) {
                block.get_or_create_child("video");
            }
            // Upstream assumes every file has audio, which is wrong for an
            // image sequence and harmless in practice: FCP ignores an audio
            // block it finds no audio behind.
            block.get_or_create_child("audio");
        }

        element.attributes.set("id", reference.id);
        Ok(element)
    }

    /// Writes the stub file element a clip with no media gets.
    fn empty_file(&mut self, id: NodeId, parent_range: TimeRange) -> Result<Element> {
        let reference = self.reference_for(id, "file")?;
        if !reference.is_new {
            return Ok(stub("file", &reference.id));
        }

        let node = self.document.try_get(id)?;
        let media = node
            .media()
            .ok_or_else(|| Error::unsupported("a clip's media reference is not one"))?;

        let mut element = element_with_metadata("file", fcp_metadata(&media.base.metadata));
        element.push_text("name", media.base.name.clone());

        let available_range = media.available_range.unwrap_or_else(|| {
            TimeRange::new(
                RationalTime::new(0.0, parent_range.start_time().rate()),
                parent_range.duration(),
            )
        });
        let rate = available_range.start_time().rate();
        element.push(build_rate(rate));

        // Only state a duration where the media claimed one. A slug has none,
        // and inventing one would make it look like real media.
        if media.available_range.is_some() {
            element.push_text(
                "duration",
                frame_text(available_range.duration().rescaled_to(rate)),
            );
        }

        element.push(timecode_from_metadata(
            available_range.start_time(),
            fcp_metadata(&media.base.metadata).and_then(|metadata| dict_sub(metadata, "timecode")),
        )?);

        let block = element.get_or_create_child("media");
        block.get_or_create_child("video");

        element.attributes.set("id", reference.id);
        Ok(element)
    }

    /// Rebuilds an FCP 7 `generatoritem`'s effect from the metadata the reader
    /// kept.
    ///
    /// OTIO has no first-class generator schema, so there is nothing to
    /// translate from; what comes back out is what went in. Where the metadata
    /// is not a usable effect, the clip falls back to an empty file, which at
    /// least holds the place.
    /// Writes an item's effects as the `filter` elements the format uses.
    ///
    /// Deliberate deviation from upstream, whose writer never looks at
    /// `effects`: it reproduces whatever `filter` elements the file it read
    /// happened to carry, so an effect added in code is not written at all and
    /// one deleted in code is written anyway. The effect list is the
    /// document's own answer, so it is the one that gets written.
    ///
    /// A `filter` holds one `effect` and nothing else — no attributes of its
    /// own in the format's own documentation, nor in any file upstream ships —
    /// so rebuilding it from the effect keeps everything it said.
    fn filters(&self, id: NodeId) -> Result<Vec<Element>> {
        let effects = self
            .document
            .try_get(id)?
            .item()
            .map(|item| item.effects.clone())
            .unwrap_or_default();

        let mut built = Vec::new();
        for effect in effects {
            let Node::Effect(data) = self.document.try_get(effect)? else {
                continue;
            };
            let mut element = match fcp_metadata(&data.base.metadata) {
                Some(metadata) => dict_to_xml_tree(metadata, "effect"),
                None => Element::new("effect"),
            };
            element.push_text("name", data.base.name.clone());

            let mut filter = Element::new("filter");
            apply_enabled(&mut filter, data.enabled);
            filter.push(element);
            built.push(filter);
        }
        Ok(built)
    }

    fn generator_effect(&mut self, clip: NodeId) -> Result<Element> {
        let Some(media) = self.active_media_reference(clip)? else {
            return self.empty_file(clip, self.source_range_of(clip)?);
        };
        let node = self.document.try_get(media)?;
        let Node::GeneratorReference(generator) = node else {
            return self.empty_file(media, self.source_range_of(clip)?);
        };

        let Some(metadata) = fcp_metadata(&generator.media.base.metadata) else {
            return self.empty_file(media, self.source_range_of(clip)?);
        };

        let mut element = dict_to_xml_tree(metadata, "effect");
        for required in ["effecttype", "mediatype", "effectcategory"] {
            if element.find(required).is_none() {
                return self.empty_file(media, self.source_range_of(clip)?);
            }
        }

        element.push_text("name", generator.media.base.name.clone());
        element.push_text("effectid", generator.generator_kind.clone());
        Ok(element)
    }

    fn marker(&mut self, id: NodeId) -> Result<Element> {
        let Node::Marker(marker) = self.document.try_get(id)? else {
            return Err(Error::unsupported("a marker is not a marker"));
        };

        let mut element = element_with_metadata("marker", fcp_metadata(&marker.base.metadata));
        element.push_text("name", marker.base.name.clone());
        element.push_text("in", frame_text(marker.marked_range.start_time()));
        element.push_text(
            "out",
            format_frames(
                marker.marked_range.start_time().value() + marker.marked_range.duration().value(),
            ),
        );
        Ok(element)
    }

    /// Returns this adapter's metadata namespace for an object.
    fn metadata_of(&self, id: NodeId) -> Result<Option<&AnyDictionary>> {
        Ok(self
            .document
            .try_get(id)?
            .base()
            .and_then(|base| fcp_metadata(&base.metadata)))
    }

    fn markers_of(&self, id: NodeId) -> Result<Vec<NodeId>> {
        Ok(self
            .document
            .try_get(id)?
            .item()
            .map(|item| item.markers.clone())
            .unwrap_or_default())
    }

    fn active_media_reference(&self, clip: NodeId) -> Result<Option<NodeId>> {
        let Node::Clip(clip) = self.document.try_get(clip)? else {
            return Ok(None);
        };
        Ok(clip
            .media_references
            .get(&clip.active_media_reference_key)
            .copied())
    }

    /// An item's source range, which an FCP 7 item always has.
    fn source_range_of(&self, id: NodeId) -> Result<TimeRange> {
        self.document
            .try_get(id)?
            .item()
            .and_then(|item| item.source_range)
            .ok_or_else(|| {
                Error::unsupported(
                    "an item on a track has no source range, so there is no \
                     in and out point to write",
                )
            })
    }
}

/// Writes the five timing elements every track item carries.
///
/// FCP counts a clip's `in` and `out` from the start of the media file, while
/// OTIO counts them from wherever the media's own timecode begins, so the
/// media start time comes off both.
fn build_item_timings(
    element: &mut Element,
    source_range: TimeRange,
    timeline_range: TimeRange,
    offsets: [Option<RationalTime>; 2],
    media_start: RationalTime,
) {
    let rate = source_range.start_time().rate();
    let mut source_start = (source_range.start_time() - media_start).rescaled_to(rate);
    let mut source_end = (source_range.end_time_exclusive() - media_start).rescaled_to(rate);

    let mut start = frame_text(timeline_range.start_time());
    let mut end = frame_text(timeline_range.end_time_exclusive());

    element.push(build_rate(rate));

    // A -1 start or end says "wherever the transition next to me cuts", and
    // the media in and out points widen to cover the transition's full span.
    if let Some(offset) = offsets[0] {
        start = "-1".to_string();
        source_start -= offset;
    }
    if let Some(offset) = offsets[1] {
        end = "-1".to_string();
        source_end += offset;
    }

    element.push_text("duration", frame_text(source_range.duration()));
    element.push_text("start", start);
    element.push_text("end", end);
    element.push_text("in", frame_text(source_start));
    element.push_text("out", frame_text(source_end));
}

/// Builds a `rate` element for a frame rate.
///
/// FCP states a rate as a whole timebase plus a flag saying whether the real
/// rate is that figure times 1000/1001. So 23.976 is written as a timebase of
/// 24 with the NTSC flag set.
fn build_rate(fps: f64) -> Element {
    let timebase = fps.ceil();
    let mut element = Element::new("rate");
    element.push_text("timebase", format_frames(timebase));
    element.push_text(
        "ntsc",
        if (timebase - fps).abs() < f64::EPSILON {
            "FALSE"
        } else {
            "TRUE"
        },
    );
    element
}

/// Builds a `timecode` element, taking what it can from the metadata the
/// reader kept.
fn timecode_from_metadata(time: RationalTime, metadata: Option<&AnyDictionary>) -> Result<Element> {
    let rate = metadata
        .and_then(|metadata| {
            let timebase = dict_str(metadata, "timebase")?.parse::<f64>().ok()?;
            let ntsc = dict_str(metadata, "ntsc").map(|value| value.eq_ignore_ascii_case("true"));
            Some(crate::util::otio_rate(timebase, ntsc))
        })
        .unwrap_or_else(|| time.rate());

    let drop_frame = metadata
        .and_then(|metadata| dict_str(metadata, "displayformat"))
        .unwrap_or("NDF")
        == "DF";

    build_timecode(time, rate, drop_frame, metadata)
}

/// Builds a `timecode` element.
///
/// Only four of the metadata's own children are carried across: the rest
/// describe the timing, which is recomputed here.
fn build_timecode(
    time: RationalTime,
    fps: f64,
    drop_frame: bool,
    metadata: Option<&AnyDictionary>,
) -> Result<Element> {
    let mut element = match metadata {
        Some(metadata) => {
            let kept: AnyDictionary = metadata
                .iter()
                .filter(|(key, _)| matches!(key.as_str(), "field" | "reel" | "source" | "format"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            dict_to_xml_tree(&kept, "timecode")
        }
        None => Element::new("timecode"),
    };

    let rate_element = build_rate(fps);
    let is_ntsc = rate_element
        .find("ntsc")
        .is_some_and(|ntsc| ntsc.text_or_empty() == "TRUE");
    element.push(rate_element);

    // Drop-frame timecode only exists at the NTSC rates, so asking for it at a
    // whole timebase means the whole timebase was the approximation.
    let timecode_fps = if drop_frame && !is_ntsc {
        fps * 1000.0 / 1001.0
    } else {
        fps
    };

    let at_rate = RationalTime::new(time.value_rescaled_to(fps), timecode_fps);
    let timecode = at_rate.to_timecode_at(
        timecode_fps,
        if drop_frame {
            DropFrame::ForceYes
        } else {
            DropFrame::ForceNo
        },
    )?;

    element.push_text("string", timecode.clone());
    element.push_text("frame", format_frames(time.value()));
    element.push_text(
        "displayformat",
        if timecode.contains(';') { "DF" } else { "NDF" },
    );

    Ok(element)
}

/// Builds the element for an object, seeded with whatever the reader kept.
/// Writes an item's `enabled` state into its element.
///
/// Deliberate deviation from upstream, whose writer never looks at the field:
/// it reproduces whatever `enabled` element the file it read happened to
/// carry, so a clip disabled in code is written as enabled and a clip
/// re-enabled in code stays disabled. The field is the document's own answer,
/// so it wins over the preserved element.
///
/// An item that is enabled and carried no `enabled` element gets none, which
/// is what the format means by leaving it out and keeps a read-and-write from
/// adding elements the file never had.
fn apply_enabled(element: &mut Element, enabled: bool) {
    let text = if enabled { "TRUE" } else { "FALSE" };
    if let Some(existing) = element.find_mut("enabled") {
        existing.text = Some(text.to_string());
    } else if !enabled {
        element.push_text("enabled", text);
    }
}

fn element_with_metadata(tag: &str, metadata: Option<&AnyDictionary>) -> Element {
    match metadata {
        Some(metadata) => dict_to_xml_tree(metadata, tag),
        None => Element::new(tag),
    }
}

/// The element written in place of an object that has already been written out
/// in full.
fn stub(tag: &str, id: &str) -> Element {
    let mut element = Element::new(tag);
    element.attributes.set("id", id);
    element
}

/// Takes a child out of an element, or makes a fresh one.
fn take_child(parent: &mut Element, tag: &str) -> Element {
    match parent.children.iter().position(|child| child.tag == tag) {
        Some(index) => parent.children.remove(index),
        None => Element::new(tag),
    }
}

/// Reads the number out of an `id` like `clipitem-22`, if it belongs to `tag`.
fn parse_reference_id(id: &str, tag: &str) -> Option<u32> {
    let (prefix, number) = id.rsplit_once('-')?;
    if prefix != tag || number.is_empty() || !prefix.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    number.parse().ok()
}

/// The URL of an image sequence with its frame number left as a printf
/// placeholder, which is how FCP writes one.
fn abstract_target_url(sequence: &otio_core::schema::ImageSequenceReference) -> String {
    let base = if sequence.target_url_base.ends_with('/') {
        sequence.target_url_base.clone()
    } else {
        format!("{}/", sequence.target_url_base)
    };
    format!(
        "{base}{}%0{}d{}",
        sequence.name_prefix, sequence.frame_zero_padding, sequence.name_suffix
    )
}

/// The lowercase suffix of a path, with its dot, or the empty string.
fn suffix_of(path: &str) -> String {
    let name = match path.rfind('/') {
        Some(index) => &path[index + 1..],
        None => path,
    };
    // A leading dot makes a hidden file, not a suffix, which is what
    // `os.path.splitext` says too.
    if name.is_empty() {
        return String::new();
    }
    match name[1..].rfind('.') {
        Some(index) => name[index + 1..].to_ascii_lowercase(),
        None => String::new(),
    }
}

/// The last component of a name, as `os.path.basename` gives it.
fn basename(name: &str) -> &str {
    match name.rfind('/') {
        Some(index) => &name[index + 1..],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use opentime::{RationalTime, TimeRange};
    use otio_core::schema::{Base, Clip, ItemData, MediaReferenceData, MissingReference, Node};
    use otio_core::{Any, AnyDictionary, Document, NodeId};

    use super::{Writer, basename, build_rate, parse_reference_id, suffix_of};

    #[test]
    fn rates_are_written_as_a_timebase_and_an_ntsc_flag() {
        let rate = build_rate(24.0);
        assert_eq!(rate.find("timebase").map(|e| e.text_or_empty()), Some("24"));
        assert_eq!(rate.find("ntsc").map(|e| e.text_or_empty()), Some("FALSE"));

        let ntsc = build_rate(RationalTime::new(1.0, 30000.0 / 1001.0).rate());
        assert_eq!(ntsc.find("timebase").map(|e| e.text_or_empty()), Some("30"));
        assert_eq!(ntsc.find("ntsc").map(|e| e.text_or_empty()), Some("TRUE"));
    }

    #[test]
    fn an_id_is_reused_only_for_its_own_tag() {
        assert_eq!(parse_reference_id("clipitem-22", "clipitem"), Some(22));
        assert_eq!(parse_reference_id("clipitem-22", "file"), None);
        assert_eq!(parse_reference_id("clipitem-", "clipitem"), None);
        assert_eq!(parse_reference_id("nodash", "clipitem"), None);
    }

    #[test]
    fn suffixes_and_basenames_follow_os_path() {
        assert_eq!(suffix_of("/a/b/take1.MOV"), ".mov");
        assert_eq!(suffix_of("/a/b/take1"), "");
        assert_eq!(suffix_of("/a/b/.hidden"), "");
        assert_eq!(basename("sc01/layerA"), "layerA");
        assert_eq!(basename("layerA"), "layerA");
    }

    /// Builds a clip with a name and, optionally, an `fcp_xml` id to prefer.
    fn clip(document: &mut Document, name: &str, preferred_id: Option<&str>) -> NodeId {
        let mut metadata = AnyDictionary::new();
        if let Some(id) = preferred_id {
            let mut namespaced = AnyDictionary::new();
            namespaced.insert("@id".to_string(), Any::String(id.to_string()));
            metadata.insert(
                crate::dict::META_NAMESPACE.to_string(),
                Any::Dictionary(namespaced),
            );
        }
        document.insert(Node::Clip(Clip {
            item: ItemData {
                base: Base {
                    name: name.to_string(),
                    metadata,
                },
                ..ItemData::new()
            },
            media_references: BTreeMap::new(),
            active_media_reference_key: String::new(),
        }))
    }

    /// Upstream's `test_backreference_for_id`.
    ///
    /// Two objects that serialize the same share an id, so a file does not
    /// repeat the same clip definition on every use of it.
    #[test]
    fn equal_objects_share_one_id() {
        let mut document = Document::new();
        let first = clip(&mut document, "clip1", None);
        let same_again = clip(&mut document, "clip1", None);
        let second = clip(&mut document, "clip2", None);

        let mut writer = Writer {
            document: &document,
            references: BTreeMap::new(),
        };

        let first = writer.reference_for(first, "clipitem").expect("an id");
        assert_eq!((first.id.as_str(), first.is_new), ("clipitem-1", true));

        let second = writer.reference_for(second, "clipitem").expect("an id");
        assert_eq!((second.id.as_str(), second.is_new), ("clipitem-2", true));

        let same_again = writer.reference_for(same_again, "clipitem").expect("an id");
        assert_eq!(
            (same_again.id.as_str(), same_again.is_new),
            ("clipitem-1", false)
        );
    }

    /// Upstream's `test_backreference_for_id_preserved`.
    ///
    /// An id the file came in with is kept where it is still free, so reading
    /// and writing a file does not renumber everything in it. Where it is
    /// taken, the object gets the lowest free number instead of stomping on
    /// whatever holds it.
    #[test]
    fn an_incoming_id_is_kept_unless_it_is_taken() {
        let mut document = Document::new();
        let with_id = clip(&mut document, "clip23", Some("clipitem-23"));
        let plain = clip(&mut document, "clip2", None);
        let conflicting = clip(&mut document, "conflicting_clip", Some("clipitem-1"));

        let mut writer = Writer {
            document: &document,
            references: BTreeMap::new(),
        };
        // Stand in for three objects already written, as upstream's test does.
        writer.references.insert(
            "clipitem",
            vec![
                ("already-1".to_string(), 1),
                ("already-2".to_string(), 2),
                ("already-3".to_string(), 3),
            ],
        );

        let kept = writer.reference_for(with_id, "clipitem").expect("an id");
        assert_eq!((kept.id.as_str(), kept.is_new), ("clipitem-23", true));

        // 1, 2, 3 and 23 are taken, so the next object fills the gap at 4.
        let filled = writer.reference_for(plain, "clipitem").expect("an id");
        assert_eq!((filled.id.as_str(), filled.is_new), ("clipitem-4", true));

        let moved = writer
            .reference_for(conflicting, "clipitem")
            .expect("an id");
        assert_eq!((moved.id.as_str(), moved.is_new), ("clipitem-5", true));
    }

    /// Upstream's `test_build_empty_file`.
    ///
    /// A clip whose media is missing still writes a file element: enough for
    /// the host application to show a slot at the right place, with nothing
    /// behind it. Its timecode is rebuilt, but the `reel` the input named is
    /// carried across.
    #[test]
    fn a_missing_media_reference_writes_a_stub_file() {
        let mut metadata = AnyDictionary::new();
        let mut namespaced = AnyDictionary::new();
        let mut timecode = AnyDictionary::new();
        let mut rate = AnyDictionary::new();
        rate.insert("ntsc".to_string(), Any::String("FALSE".to_string()));
        rate.insert("timebase".to_string(), Any::String("24".to_string()));
        timecode.insert("rate".to_string(), Any::Dictionary(rate));
        timecode.insert("displayformat".to_string(), Any::String("NDF".to_string()));
        let mut reel = AnyDictionary::new();
        reel.insert(
            "name".to_string(),
            Any::String("test_reel_name".to_string()),
        );
        timecode.insert("reel".to_string(), Any::Dictionary(reel));
        namespaced.insert("timecode".to_string(), Any::Dictionary(timecode));
        metadata.insert(
            crate::dict::META_NAMESPACE.to_string(),
            Any::Dictionary(namespaced),
        );

        let available_range = TimeRange::new(
            RationalTime::new(820_489.0, 24.0),
            RationalTime::new(2087.0, 24.0),
        );
        let mut document = Document::new();
        let reference = document.insert(Node::MissingReference(MissingReference {
            media: MediaReferenceData {
                base: Base {
                    name: "test_clip_name".to_string(),
                    metadata,
                },
                available_range: Some(available_range),
                available_image_bounds: None,
            },
        }));

        let mut writer = Writer {
            document: &document,
            references: BTreeMap::new(),
        };
        let element = writer
            .empty_file(reference, available_range)
            .expect("a file element");

        assert_eq!(
            element.find("name").map(|e| e.text_or_empty()),
            Some("test_clip_name")
        );
        assert_eq!(
            element.find("duration").map(|e| e.text_or_empty()),
            Some("2087")
        );
        assert_eq!(
            element
                .find_path(&["rate", "ntsc"])
                .map(|e| e.text_or_empty()),
            Some("FALSE")
        );
        assert_eq!(
            element
                .find_path(&["rate", "timebase"])
                .map(|e| e.text_or_empty()),
            Some("24")
        );

        let timecode = element.find("timecode").expect("a timecode element");
        assert_eq!(
            timecode
                .find_path(&["rate", "ntsc"])
                .map(|e| e.text_or_empty()),
            Some("FALSE")
        );
        assert_eq!(
            timecode
                .find_path(&["rate", "timebase"])
                .map(|e| e.text_or_empty()),
            Some("24")
        );
        assert_eq!(
            timecode.find("string").map(|e| e.text_or_empty()),
            Some("09:29:47:01")
        );
        assert_eq!(
            timecode
                .find_path(&["reel", "name"])
                .map(|e| e.text_or_empty()),
            Some("test_reel_name")
        );
    }
}

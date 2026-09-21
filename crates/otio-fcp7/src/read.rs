//! Reading FCP 7 XML into an OTIO document.
//!
//! Two things in the format need state carried through the read, and they are
//! why this is a struct rather than a pile of functions:
//!
//! - **Inheritance.** An element may leave a value out and take the enclosing
//!   element's instead. [`Context`] models that as a stack.
//! - **The `id` attribute.** The first element with a given `id` is written
//!   out in full; later references are stubs carrying only the `id`. Every
//!   element is put through [`Parser::deref`] before it is read, so the rest of
//!   the code never has to think about it.

use std::collections::BTreeMap;
use std::collections::HashMap;

use opentime::{RationalTime, TimeRange};
use otio_core::schema::{
    Base, Clip, ExternalReference, Gap, GeneratorReference, ItemData, Marker, MediaReferenceData,
    MissingReference, Node, Stack, Timeline, Track, Transition,
};
use otio_core::{Any, AnyDictionary, Document, NodeId};
use otio_xml::Element;

use otio_adapter::{Error, Result};

use crate::dict::{META_NAMESPACE, xml_tree_to_dict};
use crate::err;
use crate::util::{Context, bool_value, name_from_element, parse_f64, parse_i64, require_child};

/// The transition type OTIO gives every FCP 7 transition.
///
/// FCP 7 XML names the effect in metadata; OTIO's schema has only the one
/// transition type, so upstream maps them all to it and leaves the effect name
/// on the object for a writer to put back.
const SMPTE_DISSOLVE: &str = "SMPTE_Dissolve";

/// Tags that carry an item on a track.
const TIMELINE_ITEM_TAGS: [&str; 3] = ["clipitem", "generatoritem", "transitionitem"];

/// Reads a whole FCP 7 XML document.
///
/// The root of the document returned is a `Timeline` when the file holds one
/// sequence, and a `SerializableCollection` of timelines when it holds
/// several.
///
/// # Errors
///
/// Returns [`Error::Parse`] if the input is not well-formed XML, if it holds
/// no sequence, or if a sequence is missing something this adapter needs.
pub fn read_from_string(input: &str) -> Result<Document> {
    let tree = otio_xml::parse(input).map_err(|error| {
        // Turn the byte offset into a line number, which is what a person
        // looking at a 40,000-line export can actually use.
        let line = input[..error.offset.min(input.len())]
            .lines()
            .count()
            .max(1);
        Error::parse_at(line, format!("invalid XML: {}", error.message))
    })?;

    let mut parser = Parser {
        ids: HashMap::new(),
        document: Document::new(),
    };
    let timelines = parser.top_level_sequences(&tree)?;

    let root = match timelines.len() {
        0 => return Err(err::no_sequences()),
        1 => timelines[0],
        _ => parser.document.insert(Node::SerializableCollection(
            otio_core::schema::SerializableCollection {
                base: Base {
                    name: "Sequences".to_string(),
                    metadata: AnyDictionary::new(),
                },
                children: timelines,
            },
        )),
    };

    parser.document.set_root(Some(root));
    Ok(parser.document)
}

struct Parser<'a> {
    /// The first element seen for each `id`, which is the one holding the
    /// content that later stubs refer to.
    ids: HashMap<String, &'a Element>,
    document: Document,
}

impl<'a> Parser<'a> {
    /// Resolves an element through the `id` table.
    ///
    /// The first element seen with a given `id` is the canonical one, and
    /// every later element carrying that `id` resolves to it. An element with
    /// no `id` is itself.
    fn deref(&mut self, element: &'a Element) -> &'a Element {
        match element.attributes.get("id") {
            None => element,
            Some(id) => self.ids.entry(id.to_string()).or_insert(element),
        }
    }

    fn deref_child(&mut self, parent: &'a Element, tag: &str) -> Option<&'a Element> {
        let child = parent.find(tag)?;
        Some(self.deref(child))
    }

    /// Finds every top-level sequence and reads each into a timeline.
    ///
    /// A file may put its sequences at the top level, or inside a project or a
    /// bin, which is what the `children` element marks.
    fn top_level_sequences(&mut self, tree: &'a Element) -> Result<Vec<NodeId>> {
        let mut elements: Vec<&'a Element> = Vec::new();
        for sequence in tree.find_all("sequence") {
            elements.push(self.deref(sequence));
        }
        for container in tree.descendants().filter(|e| e.tag == "children") {
            for sequence in container.find_all("sequence") {
                elements.push(self.deref(sequence));
            }
        }

        let context = Context::new();
        elements
            .into_iter()
            .map(|element| self.timeline_for_sequence(element, &context))
            .collect()
    }

    fn timeline_for_sequence(
        &mut self,
        sequence: &'a Element,
        context: &Context<'a>,
    ) -> Result<NodeId> {
        let local = context.pushing(sequence)?;
        let name = name_from_element(sequence);
        let mut metadata = xml_tree_to_dict(sequence, &["name", "media", "marker", "duration"]);

        let global_start_time = match self.deref_child(sequence, "timecode") {
            Some(timecode) => Some(time_from_timecode_element(timecode, &local)?),
            None => None,
        };

        let tracks = match self.deref_child(sequence, "media") {
            None => None,
            Some(media) => {
                // The `video` and `audio` blocks carry format information the
                // sequence itself has nowhere to put, so lift it onto the
                // timeline's metadata rather than losing it.
                for media_type in &media.children {
                    let info = xml_tree_to_dict(media_type, &["track"]);
                    if info.is_empty() {
                        continue;
                    }
                    let slot = metadata
                        .entry("media".to_string())
                        .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
                    if let Any::Dictionary(media_metadata) = slot {
                        media_metadata.insert(media_type.tag.clone(), Any::Dictionary(info));
                    }
                }

                let stack = self.stack_for_element(media, &local)?;
                if let Some(base) = self.document.try_get_mut(stack)?.base_mut() {
                    base.name.clone_from(&name);
                }
                Some(stack)
            }
        };

        let timeline = self.document.insert(Node::Timeline(Timeline {
            base: Base {
                name,
                metadata: namespaced(metadata),
            },
            tracks,
            global_start_time,
        }));

        // The sequence's own markers belong on the top stack. A sequence with
        // no `media` element has no stack to put them on; upstream fails on
        // such a file, and this keeps the markers rather than the failure.
        let markers = self.markers_from_element(sequence, context)?;
        if let Some(tracks) = tracks {
            if let Some(item) = self.document.try_get_mut(tracks)?.item_mut() {
                item.markers.extend(markers);
            }
        }

        Ok(timeline)
    }

    /// Reads the tracks under a `media` element, or under a nested `sequence`,
    /// as a stack.
    fn stack_for_element(&mut self, element: &'a Element, context: &Context<'a>) -> Result<NodeId> {
        let local = context.pushing(element)?;

        let media_types: Vec<&'a Element> = element
            .children
            .iter()
            .map(|child| self.deref(child))
            .collect();

        let mut tracks = Vec::new();
        for media_type in media_types {
            let Some(kind) = track_kind_from_element(media_type) else {
                continue;
            };
            let is_audio = kind == "Audio";
            let track_elements: Vec<&'a Element> = media_type
                .find_all("track")
                .collect::<Vec<_>>()
                .into_iter()
                .map(|track| self.deref(track))
                .collect();
            for track in track_elements {
                if is_audio && !is_primary_audio_channel(track) {
                    continue;
                }
                tracks.push(self.track_for_element(track, kind, &local)?);
            }
        }

        let markers = self.markers_from_element(element, context)?;
        let stack = self.document.insert(Node::Stack(Stack {
            item: ItemData {
                base: Base {
                    name: name_from_element(element),
                    metadata: AnyDictionary::new(),
                },
                markers,
                ..ItemData::new()
            },
            children: Vec::new(),
        }));
        for track in tracks {
            self.document.append_child(stack, track)?;
        }

        Ok(stack)
    }

    fn track_for_element(
        &mut self,
        element: &'a Element,
        kind: &str,
        context: &Context<'a>,
    ) -> Result<NodeId> {
        let local = context.pushing(element)?;
        let metadata = xml_tree_to_dict(element, &TIMELINE_ITEM_TAGS);

        // Premiere writes the name a user sees as an attribute, and leaves the
        // `name` element for its own internal label.
        let mut name = element
            .find("name")
            .map(|name| name.text_or_empty().to_string())
            .unwrap_or_default();
        if let Some(Any::String(premiere_name)) = metadata.get("@MZ.TrackName") {
            name.clone_from(premiere_name);
        }

        let enabled = element.find("enabled").is_none_or(bool_value);
        let track = self.document.insert(Node::Track(Track {
            item: ItemData {
                base: Base {
                    name,
                    metadata: namespaced(metadata),
                },
                enabled,
                ..ItemData::new()
            },
            children: Vec::new(),
            kind: kind.to_string(),
        }));

        let track_rate = local.require_rate(element)?;
        let mut playhead = RationalTime::new(0.0, track_rate);
        let mut head_transition: Option<&'a Element> = None;

        for (index, child) in element.children.iter().enumerate() {
            if !TIMELINE_ITEM_TAGS.contains(&child.tag.as_str()) {
                continue;
            }
            let item_element = self.deref(child);

            // A `start` of -1 means the item begins where the transition after
            // it cuts, so the next sibling has to be in hand before this item
            // can be placed.
            let tail_transition = element
                .children
                .get(index + 1)
                .filter(|next| next.tag == "transitionitem")
                .map(|next| self.deref(next));

            let (item, item_range) = self.item_and_timing_for_element(
                item_element,
                head_transition,
                tail_transition,
                &local,
            )?;

            if playhead < item_range.start_time() {
                let gap_duration = (item_range.start_time() - playhead).rescaled_to(track_rate);
                let gap = self.document.insert(Node::Gap(Gap {
                    item: ItemData {
                        source_range: Some(TimeRange::new(
                            RationalTime::new(0.0, gap_duration.rate()),
                            gap_duration,
                        )),
                        ..ItemData::new()
                    },
                }));
                self.document.append_child(track, gap)?;
            }

            self.document.append_child(track, item)?;
            playhead = item_range.end_time_exclusive();

            head_transition = (item_element.tag == "transitionitem").then_some(item_element);
        }

        Ok(track)
    }

    /// Reads a track item, and works out the span of the timeline it covers.
    ///
    /// The span is not just the item's own `start` and `end`: either may be
    /// `-1`, meaning the item runs to where a neighbouring transition cuts.
    fn item_and_timing_for_element(
        &mut self,
        element: &'a Element,
        head_transition: Option<&'a Element>,
        tail_transition: Option<&'a Element>,
        context: &Context<'a>,
    ) -> Result<(NodeId, TimeRange)> {
        let start_value = parse_i64("start", require_child(element, "start")?)?;
        let end_value = parse_i64("end", require_child(element, "end")?)?;

        let (start, start_offset) = if start_value == -1 {
            let head = head_transition.ok_or_else(|| err::missing("transitionitem", element))?;
            let start = transition_cut_point(head, context)?;
            // How far into the media the transition has already carried the
            // item. The duration accounts for the same thing at the out point.
            let transition_rate = context.pushing(head)?.require_rate(head)?;
            let transition_start = parse_i64("start", require_child(head, "start")?)?;
            #[expect(
                clippy::cast_precision_loss,
                reason = "frame numbers in an edit are far below the 53-bit exact range"
            )]
            let offset = start - RationalTime::new(transition_start as f64, transition_rate);
            (start, offset)
        } else {
            let parent_rate = context.require_rate(element)?;
            #[expect(
                clippy::cast_precision_loss,
                reason = "frame numbers in an edit are far below the 53-bit exact range"
            )]
            let start = RationalTime::new(start_value as f64, parent_rate);
            (start, RationalTime::default())
        };

        let end = if end_value == -1 {
            let tail = tail_transition.ok_or_else(|| err::missing("transitionitem", element))?;
            transition_cut_point(tail, context)?
        } else {
            let parent_rate = context.require_rate(element)?;
            #[expect(
                clippy::cast_precision_loss,
                reason = "frame numbers in an edit are far below the 53-bit exact range"
            )]
            let end = RationalTime::new(end_value as f64, parent_rate);
            end
        };

        let item_range = TimeRange::new(start, end - start);

        // What the reader turns into a real OTIO field is left out, so it is
        // not written twice; everything else is kept so a round trip does not
        // lose it.
        //
        // A transition is the exception: its `effect` subtree holds the only
        // statement of what the transition actually is — the effect id, the
        // wipe code and accuracy, the start and end ratios, the reverse flag.
        // OTIO has a field for none of that and the reader keeps only the
        // display name, so dropping the subtree turns every wipe into a cross
        // dissolve on the way back out. Deliberate deviation from upstream,
        // which drops it.
        let mut ignored = vec![
            "name", "start", "end", "in", "out", "duration", "file", "marker", "rate", "sequence",
        ];
        if element.tag == "transitionitem" {
            ignored.push("filter");
        } else {
            // A `filter` becomes a real OTIO effect, and the writer builds it
            // back from that, so keeping the subtree here as well would write
            // every filter twice. Deliberate deviation from upstream, which
            // keeps both and writes only the copy.
            ignored.push("effect");
            ignored.push("filter");
        }
        let metadata = xml_tree_to_dict(element, &ignored);

        let item = match element.tag.as_str() {
            "clipitem" | "generatoritem" => {
                self.clip_for_element(element, item_range, start_offset, context)?
            }
            "transitionitem" => self.transition_for_element(element, context)?,
            other => self.document.insert(Node::Item(ItemData {
                base: Base {
                    name: format!("unknown-{other}"),
                    metadata: AnyDictionary::new(),
                },
                source_range: Some(item_range),
                ..ItemData::new()
            })),
        };

        if !metadata.is_empty() {
            if let Some(base) = self.document.try_get_mut(item)?.base_mut() {
                merge_namespaced(&mut base.metadata, metadata);
            }
        }

        Ok((item, item_range))
    }

    fn clip_for_element(
        &mut self,
        element: &'a Element,
        item_range: TimeRange,
        start_offset: RationalTime,
        context: &Context<'a>,
    ) -> Result<NodeId> {
        let local = context.pushing(element)?;
        let name = name_from_element(element);

        let file_element = self.deref_child(element, "file");
        let sequence_element = self.deref_child(element, "sequence");
        let generator_element = if element.tag == "generatoritem" {
            element.find_with_child_text("effect", "effecttype", "generator")
        } else {
            None
        };

        let mut media_start_time = RationalTime::default();
        let item = if let Some(sequence) = sequence_element {
            // A nested sequence reads as a stack in place of a clip. Whether
            // there is a media start time worth taking from it is an open
            // question upstream too.
            self.stack_for_element(sequence, &local)?
        } else if file_element.is_some() || generator_element.is_some() {
            let media_reference = if let Some(file) = file_element {
                let reference = self.media_reference_for_file_element(file, &local)?;
                if let Some(timecode) = file.find("timecode") {
                    // The file goes on the stack before its own timecode is
                    // read, so a timecode that omits a rate takes the file's
                    // rather than the clip's or the track's. Deliberate
                    // deviation from upstream, which reads it in the clip's
                    // context while the same timecode, read again inside the
                    // media reference, gets the file's — so a file whose rate
                    // differs from its clip's ends up with a media start that
                    // disagrees with its own available range.
                    let file_context = local.pushing(file)?;
                    media_start_time = time_from_timecode_element(timecode, &file_context)?;
                }
                reference
            } else {
                let generator = generator_element.expect("one of the two branches holds");
                self.media_reference_for_effect_element(generator)
            };

            let mut media_references = BTreeMap::new();
            media_references.insert("DEFAULT_MEDIA".to_string(), media_reference);
            self.document.insert(Node::Clip(Clip {
                item: ItemData {
                    base: Base {
                        name,
                        metadata: AnyDictionary::new(),
                    },
                    ..ItemData::new()
                },
                media_references,
                active_media_reference_key: "DEFAULT_MEDIA".to_string(),
            }))
        } else {
            return Err(err::unsupported_clip_item(element));
        };

        let markers = self.markers_from_element(element, context)?;
        let enabled = element.find("enabled").is_none_or(bool_value);

        let clip_rate = local.require_rate(element)?;
        let in_value = parse_f64("in", require_child(element, "in")?)?;
        let source_start = RationalTime::new(in_value, clip_rate) + media_start_time + start_offset;

        let source_range = TimeRange::new(
            source_start.rescaled_to(clip_rate),
            item_range.duration().rescaled_to(clip_rate),
        );

        let filters: Vec<&'a Element> = element
            .find_all("filter")
            .collect::<Vec<_>>()
            .into_iter()
            .map(|filter| self.deref(filter))
            .collect();
        let mut effects = Vec::new();
        for filter in filters {
            effects.push(self.effect_from_filter_element(filter)?);
        }

        let data = self
            .document
            .try_get_mut(item)?
            .item_mut()
            .expect("a clip and a stack are both items");
        data.markers.extend(markers);
        data.enabled = enabled;
        data.source_range = Some(source_range);
        data.effects = effects;

        Ok(item)
    }

    fn media_reference_for_file_element(
        &mut self,
        element: &'a Element,
        context: &Context<'a>,
    ) -> Result<NodeId> {
        let local = context.pushing(element)?;
        let rate = local.require_rate(element)?;

        let name = name_from_element(element);
        let metadata = xml_tree_to_dict(element, &["duration", "name", "pathurl", "mediaSource"]);
        let path = element
            .find("pathurl")
            .map(|e| e.text_or_empty().to_string());
        let media_source = element
            .find("mediaSource")
            .map(|e| e.text_or_empty().to_string());

        let timecode_element = element.find("timecode");
        let start_time = match timecode_element {
            Some(timecode) => time_from_timecode_element(timecode, &local)?.rescaled_to(rate),
            None => RationalTime::new(0.0, rate),
        };

        let available_range = match element.find("duration") {
            Some(duration) => Some(TimeRange::new(
                start_time,
                RationalTime::new(parse_f64("duration", duration)?, rate),
            )),
            // A file with a timecode but no duration still says where it
            // starts, so keep that and let the duration be zero.
            None if timecode_element.is_some() => {
                Some(TimeRange::new(start_time, RationalTime::new(0.0, rate)))
            }
            None => None,
        };

        let media = MediaReferenceData {
            base: Base {
                name,
                metadata: namespaced(metadata),
            },
            available_range,
            available_image_bounds: None,
        };

        let node = match (path, media_source) {
            (Some(target_url), _) => {
                Node::ExternalReference(ExternalReference { media, target_url })
            }
            (None, Some(generator_kind)) => Node::GeneratorReference(GeneratorReference {
                media,
                generator_kind,
                parameters: AnyDictionary::new(),
            }),
            (None, None) => Node::MissingReference(MissingReference { media }),
        };

        Ok(self.document.insert(node))
    }

    /// Reads an FCP 7 `generatoritem`'s effect as a generator reference.
    fn media_reference_for_effect_element(&mut self, element: &Element) -> NodeId {
        let metadata = xml_tree_to_dict(element, &["name", "effectid"]);
        let generator_kind = element
            .find("effectid")
            .map(|e| e.text_or_empty().to_string())
            .unwrap_or_default();

        self.document
            .insert(Node::GeneratorReference(GeneratorReference {
                media: MediaReferenceData {
                    base: Base {
                        name: name_from_element(element),
                        metadata: namespaced(metadata),
                    },
                    available_range: None,
                    available_image_bounds: None,
                },
                generator_kind,
                parameters: AnyDictionary::new(),
            }))
    }

    fn effect_from_filter_element(&mut self, filter: &Element) -> Result<NodeId> {
        let effect = filter
            .find("effect")
            .ok_or_else(|| err::missing("effect", filter))?;
        let metadata = xml_tree_to_dict(effect, &["name"]);

        Ok(self
            .document
            .insert(Node::Effect(otio_core::schema::EffectData {
                base: Base {
                    name: name_from_element(effect),
                    metadata: namespaced(metadata),
                },
                effect_name: String::new(),
                enabled: true,
            })))
    }

    fn transition_for_element(
        &mut self,
        element: &'a Element,
        context: &Context<'a>,
    ) -> Result<NodeId> {
        // A transition usually carries its own rate; where it does not, the
        // track's applies.
        let local = context.pushing(element)?;
        let rate = match local.rate()? {
            Some(rate) => rate,
            None => context.require_rate(element)?,
        };

        #[expect(
            clippy::cast_precision_loss,
            reason = "frame numbers in an edit are far below the 53-bit exact range"
        )]
        let start = RationalTime::new(
            parse_i64("start", require_child(element, "start")?)? as f64,
            rate,
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "frame numbers in an edit are far below the 53-bit exact range"
        )]
        let end = RationalTime::new(
            parse_i64("end", require_child(element, "end")?)? as f64,
            rate,
        );
        let cut_point = transition_cut_point(element, context)?;

        let name = element
            .find("effect")
            .map(name_from_element)
            .unwrap_or_default();

        Ok(self.document.insert(Node::Transition(Transition {
            base: Base {
                name,
                metadata: AnyDictionary::new(),
            },
            parent: None,
            in_offset: cut_point - start,
            out_offset: end - cut_point,
            transition_type: SMPTE_DISSOLVE.to_string(),
            enabled: true,
        })))
    }

    fn markers_from_element(
        &mut self,
        element: &'a Element,
        context: &Context<'a>,
    ) -> Result<Vec<NodeId>> {
        let markers: Vec<&Element> = element.find_all("marker").collect();
        if markers.is_empty() {
            return Ok(Vec::new());
        }

        let local = context.pushing(element)?;
        let rate = local.require_rate(element)?;

        markers
            .into_iter()
            .map(|marker| self.marker_for_element(marker, rate))
            .collect()
    }

    fn marker_for_element(&mut self, element: &Element, rate: f64) -> Result<NodeId> {
        let start = RationalTime::new(parse_f64("in", require_child(element, "in")?)?, rate);
        let out_value = parse_f64("out", require_child(element, "out")?)?;

        // FCP writes -1 for a marker with no duration, so only a positive out
        // point marks a span.
        let duration = if out_value > 0.0 {
            RationalTime::new(out_value, rate) - start
        } else {
            RationalTime::new(0.0, rate)
        };

        let metadata = xml_tree_to_dict(element, &["in", "out", "name"]);
        Ok(self.document.insert(Node::Marker(Marker {
            base: Base {
                name: name_from_element(element),
                metadata: namespaced(metadata),
            },
            color: None,
            marked_range: TimeRange::new(start, duration),
            comment: String::new(),
        })))
    }
}

/// Wraps a dictionary under this adapter's metadata key, or gives back an
/// empty one if there was nothing to keep.
fn namespaced(dict: AnyDictionary) -> AnyDictionary {
    let mut metadata = AnyDictionary::new();
    if !dict.is_empty() {
        metadata.insert(META_NAMESPACE.to_string(), Any::Dictionary(dict));
    }
    metadata
}

/// Merges a dictionary into whatever is already under this adapter's key.
fn merge_namespaced(metadata: &mut AnyDictionary, dict: AnyDictionary) {
    let slot = metadata
        .entry(META_NAMESPACE.to_string())
        .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
    if let Any::Dictionary(existing) = slot {
        existing.extend(dict);
    } else {
        *slot = Any::Dictionary(dict);
    }
}

/// Returns the OTIO track kind a `media` sub-element stands for, if it is one.
fn track_kind_from_element(element: &Element) -> Option<&'static str> {
    match element.tag.to_ascii_lowercase().as_str() {
        "audio" => Some("Audio"),
        "video" => Some("Video"),
        _ => None,
    }
}

/// Returns whether this is the track a stereo pair should be read from.
///
/// FCP explodes stereo into one track per channel. OTIO keeps the pair
/// together as a single track, so only the first channel is read.
fn is_primary_audio_channel(track: &Element) -> bool {
    let index = track
        .attributes
        .get("currentExplodedTrackIndex")
        .unwrap_or("0");
    let count = track
        .attributes
        .get("totalExplodedTrackCount")
        .unwrap_or("1");
    index == "0" || count == "1"
}

/// Reads a `timecode` element as a time.
///
/// The frame number is preferred where the file gives one, because it needs no
/// interpretation. Failing that the timecode string is parsed at the rate in
/// force.
fn time_from_timecode_element(element: &Element, context: &Context<'_>) -> Result<RationalTime> {
    let local = context.pushing(element)?;
    let rate = local.require_rate(element)?;

    if let Some(frame) = element.find("frame") {
        #[expect(
            clippy::cast_precision_loss,
            reason = "frame numbers in an edit are far below the 53-bit exact range"
        )]
        let value = parse_i64("frame", frame)? as f64;
        return Ok(RationalTime::new(value, rate));
    }

    let string = element
        .find("string")
        .ok_or_else(|| err::missing("string", element))?;
    Ok(RationalTime::from_timecode(string.text_or_empty(), rate)?)
}

/// Returns the point at which a transition finishes handing over from one item
/// to the next.
///
/// `alignment` says where in the transition the cut falls: at its start, at
/// its end, or, by default, in the middle.
fn transition_cut_point(element: &Element, context: &Context<'_>) -> Result<RationalTime> {
    let alignment = require_child(element, "alignment")?
        .text_or_empty()
        .to_string();
    let start = parse_i64("start", require_child(element, "start")?)?;
    let end = parse_i64("end", require_child(element, "end")?)?;

    // `start` and `end` are counted at the rate of the transition itself.
    let local = context.pushing(element)?;
    let rate = local.require_rate(element)?;

    let value = match alignment.as_str() {
        "end" | "end-black" => end,
        "start" | "start-black" => start,
        // "center", and anything unrecognized, cuts in the middle.
        _ => (start + end) / 2,
    };

    #[expect(
        clippy::cast_precision_loss,
        reason = "frame numbers in an edit are far below the 53-bit exact range"
    )]
    let time = RationalTime::new(value as f64, rate);
    Ok(time)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use opentime::RationalTime;
    use otio_core::Document;
    use otio_core::schema::Node;
    use otio_xml::parse;

    use super::{Context, Parser, time_from_timecode_element, transition_cut_point};

    /// A timecode element with a frame number is read from the frame number.
    ///
    /// Upstream's `test_time_from_timecode_element`.
    #[test]
    fn a_timecode_is_read_from_its_frame_number() {
        let element = parse(
            "<timecode><rate><timebase>30</timebase><ntsc>FALSE</ntsc></rate>\
             <string>01:00:00:00</string><frame>108000</frame>\
             <displayformat>NDF</displayformat></timecode>",
        )
        .expect("well-formed");

        let time = time_from_timecode_element(&element, &Context::new()).expect("a time");
        assert_eq!(time, RationalTime::new(108_000.0, 30.0));
    }

    /// Upstream's `test_time_from_timecode_element_drop_frame`.
    #[test]
    fn a_drop_frame_timecode_keeps_its_ntsc_rate() {
        let element = parse(
            "<timecode><rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>\
             <string>10:03:00;05</string><frame>1084319</frame>\
             <displayformat>DF</displayformat></timecode>",
        )
        .expect("well-formed");

        let time = time_from_timecode_element(&element, &Context::new()).expect("a time");
        assert_eq!(time, RationalTime::new(1_084_319.0, 30000.0 / 1001.0));
    }

    /// Upstream's `test_time_from_timecode_element_ntsc_non_drop_frame`.
    ///
    /// With no frame number, the timecode string is parsed at the NTSC rate.
    #[test]
    fn an_ntsc_timecode_without_a_frame_number_is_parsed_from_its_string() {
        let element = parse(
            "<timecode><rate><timebase>30</timebase><ntsc>TRUE</ntsc></rate>\
             <string>00:59:56:12</string><displayformat>NDF</displayformat></timecode>",
        )
        .expect("well-formed");

        let time = time_from_timecode_element(&element, &Context::new()).expect("a time");
        assert_eq!(time, RationalTime::new(107_892.0, 30000.0 / 1001.0));
    }

    /// Upstream's `test_time_from_timecode_element_implicit_ntsc`.
    ///
    /// The timebase and the NTSC flag are looked up independently, so a
    /// timecode that states only a timebase still inherits the clip's flag.
    /// Getting this wrong reads every NTSC file half a percent fast.
    #[test]
    fn a_timecode_inherits_an_ntsc_flag_it_does_not_state() {
        let clipitem = parse(
            "<clipitem><duration>767</duration>\
             <rate><ntsc>TRUE</ntsc><timebase>24</timebase></rate>\
             <in>447</in><out>477</out><start>264</start><end>294</end>\
             <file><rate><timebase>24</timebase><ntsc>TRUE</ntsc></rate>\
             <duration>767</duration>\
             <timecode><rate><timebase>24</timebase></rate>\
             <string>14:11:44:09</string><frame>1226505</frame>\
             <displayformat>NDF</displayformat><source>source</source></timecode>\
             </file></clipitem>",
        )
        .expect("well-formed");

        let context = Context::new().pushing(&clipitem).expect("a first push");
        let timecode = clipitem
            .find_path(&["file", "timecode"])
            .expect("the timecode is present");

        let time = time_from_timecode_element(timecode, &context).expect("a time");
        assert_eq!(time, RationalTime::new(1_226_505.0, 24000.0 / 1001.0));
    }

    /// Upstream's `test_transition_cut_point`, over all four alignments.
    #[test]
    fn a_transition_cuts_where_its_alignment_says() {
        let track = parse("<track><rate><timebase>30</timebase><ntsc>FALSE</ntsc></rate></track>")
            .expect("well-formed");
        let context = Context::new().pushing(&track).expect("a first push");

        let cut_point_for = |alignment: &str| {
            let element = parse(&format!(
                "<transitionitem><start>538</start><end>557</end>\
                 <alignment>{alignment}</alignment>\
                 <rate><timebase>30</timebase><ntsc>FALSE</ntsc></rate>\
                 </transitionitem>"
            ))
            .expect("well-formed");
            transition_cut_point(&element, &context).expect("a cut point")
        };

        assert_eq!(cut_point_for("end"), RationalTime::new(557.0, 30.0));
        assert_eq!(cut_point_for("end-black"), RationalTime::new(557.0, 30.0));
        assert_eq!(cut_point_for("start"), RationalTime::new(538.0, 30.0));
        assert_eq!(cut_point_for("start-black"), RationalTime::new(538.0, 30.0));
        // The midpoint of 538 and 557 is 547.5. Upstream truncates, and its
        // own test notes that rounding down may not be the right answer.
        assert_eq!(cut_point_for("center"), RationalTime::new(547.0, 30.0));
        assert_eq!(cut_point_for("unheard-of"), RationalTime::new(547.0, 30.0));
    }

    /// Upstream's `test_transition_offset_rate`.
    ///
    /// A transition states its own rate, and its offsets are counted at that
    /// rate rather than the track's.
    #[test]
    fn a_transitions_offsets_are_at_its_own_rate() {
        let track = parse("<track><rate><timebase>60</timebase><ntsc>FALSE</ntsc></rate></track>")
            .expect("well-formed");
        let element = parse(
            "<transitionitem><start>44</start><end>54</end>\
             <alignment>end-black</alignment>\
             <rate><timebase>25</timebase><ntsc>FALSE</ntsc></rate>\
             <effect><name>Cross Dissolve</name></effect></transitionitem>",
        )
        .expect("well-formed");

        let context = Context::new().pushing(&track).expect("a first push");
        let mut parser = Parser {
            ids: HashMap::new(),
            document: Document::new(),
        };
        let id = parser
            .transition_for_element(&element, &context)
            .expect("a transition");

        let Node::Transition(transition) = parser.document.try_get(id).expect("a live transition")
        else {
            panic!("expected a Transition");
        };
        assert_eq!(transition.in_offset.rate(), 25.0);
    }
}

//! Reading FCP X XML into an OTIO document.
//!
//! The format nests differently from the way an edit is usually modelled. A
//! sequence holds one `spine`, the main storyline, and everything layered over
//! or under it hangs off whichever spine item it overlaps, carrying a `lane`
//! number that says how far above or below the storyline it sits. Turning that
//! back into tracks means walking every composable element in the sequence,
//! working out where each one really starts, and grouping them by lane.

use std::collections::{BTreeMap, HashMap};

use opentime::{RationalTime, TimeRange};
use otio_adapter::{Error, Result};
use otio_core::schema::{
    Base, Clip, ExternalReference, Gap, ItemData, Marker, MediaReferenceData, MissingReference,
    Node, SerializableCollection, Stack, Timeline, Track,
};
use otio_core::upgrade::{DEFAULT_MEDIA_KEY, color_from_legacy_name};
use otio_core::{Any, AnyDictionary, Document, NodeId};
use otio_xml::Element;

use crate::rational::{frames_at, to_rational_time};

/// The metadata key an asset's own detail is stashed under.
pub const META_NAMESPACE: &str = "fcpx";

/// The elements that stand for something occupying time on a track.
const COMPOSABLE_ELEMENTS: [&str; 4] = ["video", "audio", "ref-clip", "asset-clip"];

/// The elements that carry an item's timing, which its parts inherit.
const TIMING_ELEMENTS: [&str; 3] = ["clip", "asset-clip", "ref-clip"];

/// Reads a whole FCP X XML document.
///
/// What comes back depends on what the file holds. A library or an event
/// becomes a `SerializableCollection` of timelines, a bare project becomes a
/// `Timeline`, and a file of loose clips becomes a collection of those.
///
/// # Errors
///
/// Returns [`Error::Parse`] if the input is not well-formed XML, if it holds
/// none of those four things, or if it refers to a resource it does not
/// define.
pub fn read_from_string(input: &str) -> Result<Document> {
    let tree = otio_xml::parse(input).map_err(|error| {
        let line = input[..error.offset.min(input.len())]
            .lines()
            .count()
            .max(1);
        Error::parse_at(line, format!("invalid XML: {}", error.message))
    })?;

    let mut reader = Reader {
        root: &tree,
        parents: HashMap::new(),
        document: Document::new(),
    };
    reader.index_parents(&tree);

    let root = reader.read()?;
    reader.document.set_root(Some(root));
    Ok(reader.document)
}

struct Reader<'a> {
    root: &'a Element,
    /// Each element's parent, keyed by address.
    ///
    /// FCP X states a clip's timing on the enclosing `clip` element rather
    /// than on the `video` or `audio` element that names the media, so reading
    /// one means walking upwards from an arbitrary point in the tree.
    parents: HashMap<usize, &'a Element>,
    document: Document,
}

impl<'a> Reader<'a> {
    fn index_parents(&mut self, element: &'a Element) {
        for child in &element.children {
            self.parents
                .insert(std::ptr::from_ref(child) as usize, element);
            self.index_parents(child);
        }
    }

    fn parent_of(&self, element: &Element) -> Option<&'a Element> {
        self.parents
            .get(&(std::ptr::from_ref(element) as usize))
            .copied()
    }

    fn read(&mut self) -> Result<NodeId> {
        if let Some(library) = self.root.find("library") {
            let events: Vec<&'a Element> = library.find_all("event").collect();
            if let Some(&first) = events.first() {
                return self.collection_for_library(first, &events[1..]);
            }
        }
        if let Some(event) = self.root.find("event") {
            return self.collection_for_event(event);
        }
        if let Some(project) = self.root.find("project") {
            return self.timeline_for_project(project);
        }
        if self.root.find("asset-clip").is_some() || self.root.find("ref-clip").is_some() {
            return self.collection_for_clips();
        }
        Err(Error::parse(
            "no library, event, project or clips found: this does not look \
             like an FCP X XML document",
        ))
    }

    /// Reads a library, which is a person's whole set of events.
    ///
    /// Deliberate deviation: upstream reads the first event and silently drops
    /// every other one, along with all their projects. Here the later events'
    /// projects are read too and gathered into the same collection, which
    /// keeps the timelines a person would otherwise lose. The collection takes
    /// the first event's name, because an FCP X file holds one event and that
    /// is all a write can put back; grouping the projects under one event is
    /// what upstream's writer does with them in any case.
    fn collection_for_library(
        &mut self,
        first: &'a Element,
        rest: &[&'a Element],
    ) -> Result<NodeId> {
        let collection = self.collection_for_event(first)?;
        for event in rest {
            let mut projects = Vec::new();
            for project in event.find_all("project") {
                projects.push(self.timeline_for_project(project)?);
            }
            if let Node::SerializableCollection(data) = self.document.try_get_mut(collection)? {
                data.children.extend(projects);
            }
        }
        Ok(collection)
    }

    fn collection_for_event(&mut self, event: &'a Element) -> Result<NodeId> {
        let projects: Vec<&'a Element> = event.find_all("project").collect();
        let children = projects
            .into_iter()
            .map(|project| self.timeline_for_project(project))
            .collect::<Result<Vec<_>>>()?;

        Ok(self
            .document
            .insert(Node::SerializableCollection(SerializableCollection {
                base: Base {
                    name: event.attributes.get("name").unwrap_or_default().to_string(),
                    metadata: AnyDictionary::new(),
                },
                children,
            })))
    }

    fn timeline_for_project(&mut self, project: &'a Element) -> Result<NodeId> {
        let sequence = project
            .find("sequence")
            .ok_or_else(|| Error::parse("a project with no sequence has no timeline in it"))?;
        let tracks = self.sequence_to_stack(sequence, String::new(), None)?;

        Ok(self.document.insert(Node::Timeline(Timeline {
            base: Base {
                name: project
                    .attributes
                    .get("name")
                    .unwrap_or_default()
                    .to_string(),
                metadata: AnyDictionary::new(),
            },
            tracks: Some(tracks),
            global_start_time: None,
        })))
    }

    /// Reads a file that holds loose clips rather than an edit.
    ///
    /// This is what a Final Cut event looks like exported on its own: the
    /// clips a person has in hand, with no statement about how they go
    /// together.
    fn collection_for_clips(&mut self) -> Result<NodeId> {
        let mut children = Vec::new();

        let asset_clips: Vec<&'a Element> = self.root.find_all("asset-clip").collect();
        for clip in asset_clips {
            let format = clip.attributes.get("format").map(str::to_string);
            children.push(self.build_composable(clip, format.as_deref())?);
        }

        let ref_clips: Vec<&'a Element> = self.root.find_all("ref-clip").collect();
        for clip in ref_clips {
            // Upstream assumes the first resource is the format here, since a
            // `ref-clip` names a compound clip rather than a format.
            children.push(self.build_composable(clip, Some("r1"))?);
        }

        Ok(self
            .document
            .insert(Node::SerializableCollection(SerializableCollection {
                base: Base::default(),
                children,
            })))
    }

    /// Turns a sequence's spine into a stack of tracks, one per lane.
    fn sequence_to_stack(
        &mut self,
        sequence: &'a Element,
        name: String,
        source_range: Option<TimeRange>,
    ) -> Result<NodeId> {
        let default_format = sequence.attributes.get("format").map(str::to_string);
        let default_format = default_format.as_deref();

        struct Placed {
            lane: String,
            offset: RationalTime,
            composable: NodeId,
            audio_only: bool,
        }

        let elements: Vec<&'a Element> = std::iter::once(sequence)
            .chain(sequence.descendants())
            .filter(|element| COMPOSABLE_ELEMENTS.contains(&element.tag.as_str()))
            .collect();

        let mut placed = Vec::new();
        for element in elements {
            let composable = self.build_composable(element, default_format)?;
            let (offset, lane) = self.offset_and_lane(element, default_format)?;
            placed.push(Placed {
                lane,
                offset,
                composable,
                audio_only: self.audio_only(element),
            });
        }

        // A stack's track order is its compositing order, so lanes have to be
        // sorted by what they mean rather than by how they are spelled.
        //
        // Deliberate deviation: upstream sorts the lane strings, which puts
        // lane "10" under lane "2" and lays the picture up in the wrong
        // order. It never shows on a file with fewer than ten lanes, which is
        // every file upstream tests, but on one that has them the result is
        // wrong rather than merely surprising, so the numbers are compared as
        // numbers here. A lane that is not a number keeps the old spelling
        // order, after the ones that are.
        let mut lanes: Vec<String> = placed.iter().map(|item| item.lane.clone()).collect();
        lanes.sort_by(|a, b| match (a.parse::<i64>(), b.parse::<i64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        });
        lanes.dedup();

        let stack = self.document.insert(Node::Stack(Stack {
            item: ItemData {
                base: Base {
                    name,
                    metadata: AnyDictionary::new(),
                },
                source_range,
                ..ItemData::new()
            },
            children: Vec::new(),
        }));

        for lane in lanes {
            let mut lane_items: Vec<&Placed> =
                placed.iter().filter(|item| item.lane == lane).collect();
            lane_items.sort_by(|a, b| {
                a.offset
                    .partial_cmp(&b.offset)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let kind = if lane_items.iter().all(|item| item.audio_only) {
                "Audio"
            } else {
                "Video"
            };
            let track = self.document.insert(Node::Track(Track {
                item: ItemData {
                    base: Base {
                        name: lane.clone(),
                        metadata: AnyDictionary::new(),
                    },
                    ..ItemData::new()
                },
                children: Vec::new(),
                kind: kind.to_string(),
            }));

            for item in lane_items {
                // Anything between the end of the last item and this one is a
                // hole in the lane, which OTIO spells as a gap.
                let filled = self.document.duration(track)?.value();
                let difference = item.offset.value().trunc() - filled;
                if difference > 0.0 {
                    let gap = self.create_gap(0.0, difference, default_format)?;
                    self.document.append_child(track, gap)?;
                }
                self.document.append_child(track, item.composable)?;
            }

            self.document.append_child(stack, track)?;
        }

        Ok(stack)
    }

    fn build_composable(
        &mut self,
        element: &'a Element,
        default_format: Option<&str>,
    ) -> Result<NodeId> {
        let timing_clip = self.timing_clip(element)?;
        let format_id = self.format_id_for_clip(element, default_format)?;
        let source_range = self.time_range(timing_clip, format_id.as_deref())?;

        let composable = if element.tag == "ref-clip" {
            let reference = element.attributes.get("ref").unwrap_or_default();
            let media = self
                .compound_clip_by_id(reference)
                .ok_or_else(|| Error::parse(format!("no compound clip with id `{reference}`")))?;
            let sequence = media.find("sequence").ok_or_else(|| {
                Error::parse(format!("compound clip `{reference}` holds no sequence"))
            })?;
            let name = media.attributes.get("name").unwrap_or_default().to_string();
            self.sequence_to_stack(sequence, name, Some(source_range))?
        } else {
            let reference =
                self.reference_from_id(element.attributes.get("ref"), default_format)?;
            let mut media_references = BTreeMap::new();
            media_references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);
            self.document.insert(Node::Clip(Clip {
                item: ItemData {
                    base: Base {
                        name: timing_clip
                            .attributes
                            .get("name")
                            .unwrap_or_default()
                            .to_string(),
                        metadata: AnyDictionary::new(),
                    },
                    source_range: Some(source_range),
                    ..ItemData::new()
                },
                media_references,
                active_media_reference_key: DEFAULT_MEDIA_KEY.to_string(),
            }))
        };

        let markers: Vec<&Element> = timing_clip
            .descendants()
            .filter(|child| child.tag == "marker")
            .collect();
        let mut marker_ids = Vec::new();
        for marker in markers {
            marker_ids.push(self.marker(marker, default_format)?);
        }
        if let Some(item) = self.document.try_get_mut(composable)?.item_mut() {
            item.markers.extend(marker_ids);
        }

        Ok(composable)
    }

    /// Walks up to the element that carries this one's timing.
    fn timing_clip(&self, element: &'a Element) -> Result<&'a Element> {
        let mut current = element;
        while !TIMING_ELEMENTS.contains(&current.tag.as_str()) {
            current = self.parent_of(current).ok_or_else(|| {
                Error::parse(format!(
                    "`{}` element is not inside a clip, so nothing states its timing",
                    current.tag
                ))
            })?;
        }
        Ok(current)
    }

    /// Returns where an element really starts on the timeline, and which lane
    /// it sits in.
    ///
    /// An element's own `offset` is stated relative to whatever it hangs off,
    /// so reaching the timeline's own clock means subtracting the parent's
    /// `start` and adding the parent's `offset`.
    fn offset_and_lane(
        &self,
        element: &'a Element,
        default_format: Option<&str>,
    ) -> Result<(RationalTime, String)> {
        let clip_format = self.format_id_for_clip(element, default_format)?;
        let clip = self.timing_clip(element)?;
        let mut parent = self
            .parent_of(clip)
            .ok_or_else(|| Error::parse("a clip with no parent has no place on the timeline"))?;
        let parent_format = self.format_id_for_clip(parent, default_format)?;

        // A secondary storyline is a `spine` of its own with a lane. Its items
        // are offset within it, so the lane comes from the spine and the
        // element it hangs off is the spine's own parent.
        let (lane, on_secondary_spine) = if parent.tag == "spine"
            && parent.attributes.get("lane").is_some()
        {
            let lane = parent.attributes.get("lane").unwrap_or("0").to_string();
            parent = self.parent_of(parent).ok_or_else(|| {
                Error::parse("a secondary storyline with no parent has no place on the timeline")
            })?;
            (lane, true)
        } else {
            (
                clip.attributes.get("lane").unwrap_or("0").to_string(),
                false,
            )
        };

        let clip_rate = self.format_frame_rate_float(clip_format.as_deref())?;
        let parent_rate = self.format_frame_rate_float(parent_format.as_deref())?;

        let clip_offset = frames_at(clip.attributes.get("offset"), clip_rate);
        let parent_start = if on_secondary_spine {
            0.0
        } else {
            frames_at(parent.attributes.get("start"), parent_rate)
        };
        let parent_offset = frames_at(parent.attributes.get("offset"), parent_rate);

        let offset = clip_offset - parent_start + parent_offset;
        Ok((
            RationalTime::new(offset, self.format_frame_rate(clip_format.as_deref())?),
            lane,
        ))
    }

    /// Returns the format an element's times are counted in.
    fn format_id_for_clip(
        &self,
        element: &Element,
        default_format: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(reference) = element.attributes.get("ref") else {
            return Ok(default_format.map(str::to_string));
        };
        if element.tag == "gap" {
            return Ok(default_format.map(str::to_string));
        }

        let resource = match self.asset_by_id(reference) {
            Some(asset) => asset,
            None => self
                .compound_clip_by_id(reference)
                .and_then(|media| media.find("sequence"))
                .ok_or_else(|| {
                    Error::parse(format!("no asset or compound clip with id `{reference}`"))
                })?,
        };

        Ok(resource
            .attributes
            .get("format")
            .map(str::to_string)
            .or_else(|| default_format.map(str::to_string)))
    }

    fn reference_from_id(
        &mut self,
        asset_id: Option<&str>,
        default_format: Option<&str>,
    ) -> Result<NodeId> {
        let asset_id = asset_id.unwrap_or_default();
        let asset = self
            .asset_by_id(asset_id)
            .ok_or_else(|| Error::parse(format!("no asset with id `{asset_id}`")))?;

        let source = asset.attributes.get("src").unwrap_or_default();
        if source.is_empty() {
            return Ok(self
                .document
                .insert(Node::MissingReference(MissingReference::default())));
        }

        let format = asset
            .attributes
            .get("format")
            .or(default_format)
            .map(str::to_string);
        let rate = self.format_frame_rate(format.as_deref())?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a frame rate is a small whole number"
        )]
        let rate_int = rate as i64;
        let available_range = TimeRange::new(
            to_rational_time(asset.attributes.get("start"), rate_int),
            to_rational_time(asset.attributes.get("duration"), rate_int),
        );

        // What Final Cut knows about the media — its notes, keywords and
        // Spotlight metadata — lives on the event's clip rather than on the
        // asset, so it is gathered from there.
        let detail = self
            .assetclip_by_ref(asset_id)
            .map(collect_asset_metadata)
            .unwrap_or_default();
        let mut metadata = AnyDictionary::new();
        metadata.insert(META_NAMESPACE.to_string(), Any::Dictionary(detail));

        let target_url = source.to_string();
        Ok(self
            .document
            .insert(Node::ExternalReference(ExternalReference {
                media: MediaReferenceData {
                    base: Base {
                        name: String::new(),
                        metadata,
                    },
                    available_range: Some(available_range),
                    available_image_bounds: None,
                },
                target_url,
            })))
    }

    fn marker(&mut self, element: &Element, default_format: Option<&str>) -> Result<NodeId> {
        // Final Cut has no marker colours as such: a to-do marker is red until
        // it is completed, at which point it turns green, and a plain marker
        // is purple.
        let color = match element.attributes.get("completed") {
            Some("1") => "GREEN",
            Some(_) => "RED",
            None => "PURPLE",
        };

        let marked_range = self.time_range(element, default_format)?;
        Ok(self.document.insert(Node::Marker(Marker {
            base: Base {
                name: element
                    .attributes
                    .get("value")
                    .unwrap_or_default()
                    .to_string(),
                metadata: AnyDictionary::new(),
            },
            color: Some(color_from_legacy_name(color)),
            marked_range,
            comment: String::new(),
        })))
    }

    /// Returns whether an element carries only audio, which is what decides a
    /// lane's track kind.
    fn audio_only(&self, element: &Element) -> bool {
        match element.tag.as_str() {
            "audio" => true,
            "asset-clip" => element
                .attributes
                .get("ref")
                .and_then(|reference| self.asset_by_id(reference))
                .is_some_and(|asset| asset.attributes.get("hasVideo").unwrap_or("0") == "0"),
            "ref-clip" => element.attributes.get("srcEnable").unwrap_or("video") == "audio",
            _ => false,
        }
    }

    fn create_gap(
        &mut self,
        start: f64,
        frames: f64,
        default_format: Option<&str>,
    ) -> Result<NodeId> {
        let rate = self.format_frame_rate(default_format)?;
        Ok(self.document.insert(Node::Gap(Gap {
            item: ItemData {
                source_range: Some(TimeRange::new(
                    RationalTime::new(start, rate),
                    RationalTime::new(frames, rate),
                )),
                ..ItemData::new()
            },
        })))
    }

    fn time_range(&self, element: &Element, format_id: Option<&str>) -> Result<TimeRange> {
        let rate = self.format_frame_rate(format_id)?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a frame rate is a small whole number"
        )]
        let rate_int = rate as i64;
        Ok(TimeRange::new(
            to_rational_time(
                Some(element.attributes.get("start").unwrap_or("0s")),
                rate_int,
            ),
            to_rational_time(element.attributes.get("duration"), rate_int),
        ))
    }

    // ------------------------------------------------------------ lookups --

    fn resources(&self) -> Option<&'a Element> {
        self.root.find("resources")
    }

    fn by_id(&self, tag: &str, id: &str) -> Option<&'a Element> {
        self.resources()?
            .children
            .iter()
            .find(|element| element.tag == tag && element.attributes.get("id") == Some(id))
    }

    fn asset_by_id(&self, id: &str) -> Option<&'a Element> {
        self.by_id("asset", id)
    }

    fn format_by_id(&self, id: &str) -> Option<&'a Element> {
        self.by_id("format", id)
    }

    fn compound_clip_by_id(&self, id: &str) -> Option<&'a Element> {
        self.by_id("media", id)
    }

    /// Returns the event's clip for an asset, which is where its detail lives.
    fn assetclip_by_ref(&self, asset_id: &str) -> Option<&'a Element> {
        let container = self.root.find("event").unwrap_or(self.root);
        container.children.iter().find(|element| {
            element.tag == "asset-clip" && element.attributes.get("ref") == Some(asset_id)
        })
    }

    /// Returns a format's frame duration as its two parts.
    fn frame_duration(&self, format_id: Option<&str>) -> Result<(f64, f64)> {
        let format_id = format_id.unwrap_or_default();
        let format = self
            .format_by_id(format_id)
            .ok_or_else(|| Error::parse(format!("no format with id `{format_id}`")))?;
        let duration = format.attributes.get("frameDuration").ok_or_else(|| {
            Error::parse(format!("format `{format_id}` states no frame duration"))
        })?;

        let (total, rate) = duration.split_once('/').ok_or_else(|| {
            Error::parse(format!(
                "format `{format_id}` states a frame duration of `{duration}`, \
                 which is not a fraction"
            ))
        })?;
        let total: f64 = total
            .parse()
            .map_err(|_| Error::parse(format!("frame duration `{duration}` is not a fraction")))?;
        let rate: f64 = rate
            .trim_end_matches('s')
            .parse()
            .map_err(|_| Error::parse(format!("frame duration `{duration}` is not a fraction")))?;
        Ok((total, rate))
    }

    /// The frame rate of a format, as a whole number.
    ///
    /// Upstream truncates here, so a `1001/24000s` frame duration reports 23
    /// rather than 23.976. Every time read against such a format is at that
    /// rate, so the truncation is visible in the document, not just in an
    /// intermediate.
    fn format_frame_rate(&self, format_id: Option<&str>) -> Result<f64> {
        let (total, rate) = self.frame_duration(format_id)?;
        if total == 0.0 {
            return Err(Error::parse("a format states a frame duration of zero"));
        }
        Ok((rate / total).trunc())
    }

    /// The frame rate of a format, unrounded.
    ///
    /// Upstream keeps a rounded and an unrounded rate side by side and uses
    /// each in different places. This is the one its offset arithmetic uses.
    fn format_frame_rate_float(&self, format_id: Option<&str>) -> Result<f64> {
        let (total, rate) = self.frame_duration(format_id)?;
        if total == 0.0 {
            return Err(Error::parse("a format states a frame duration of zero"));
        }
        Ok(rate / total)
    }
}

/// Gathers the note, keywords and metadata Final Cut keeps about an asset.
fn collect_asset_metadata(element: &Element) -> AnyDictionary {
    let mut metadata = AnyDictionary::new();

    for child in std::iter::once(element).chain(element.descendants()) {
        match child.tag.as_str() {
            "md" => {
                let mut entry = AnyDictionary::new();
                entry.insert(
                    child.attributes.get("key").unwrap_or_default().to_string(),
                    child
                        .attributes
                        .get("value")
                        .map_or(Any::Null, |value| Any::String(value.to_string())),
                );
                push(&mut metadata, "metadata", Any::Dictionary(entry));
            }
            "note" => {
                metadata.insert(
                    "note".to_string(),
                    Any::String(child.text_or_empty().to_string()),
                );
            }
            "keyword" => {
                let entry: AnyDictionary = child
                    .attributes
                    .iter()
                    .map(|(name, value)| (name.to_string(), Any::String(value.to_string())))
                    .collect();
                push(&mut metadata, "keywords", Any::Dictionary(entry));
            }
            _ => {}
        }
    }

    metadata
}

fn push(metadata: &mut AnyDictionary, key: &str, value: Any) {
    match metadata
        .entry(key.to_string())
        .or_insert_with(|| Any::Vector(Vec::new()))
    {
        Any::Vector(items) => items.push(value),
        slot => *slot = Any::Vector(vec![value]),
    }
}

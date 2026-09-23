//! Writing an OTIO document as FCP X XML.
//!
//! Final Cut does not think in tracks. It has one storyline, the `spine`, and
//! everything else hangs off whichever spine item it overlaps, carrying a
//! `lane` number that says how far above or below the storyline it sits.
//! Writing a stack of tracks therefore means laying the first video track
//! straight into the spine and then, for every item on every other track,
//! finding the spine item it sits over and restating its offset relative to
//! that item.
//!
//! Media is not inline either: assets, formats and compound clips are
//! collected in a `resources` element and referred to by `rN` ids, so the
//! writer builds that table as it goes and assembles the document at the end.

use opentime::{RationalTime, TimeRange};
use otio_adapter::{Error, Result};
use otio_core::schema::{Gap, ItemData, Node};
use otio_core::{Any, AnyDictionary, Document, NodeId};
use otio_xml::Element;

use crate::rational::{frames_at, from_rational_time, rational_number};
use crate::read::META_NAMESPACE;

/// The frame duration Final Cut writes for each frame rate it knows.
///
/// Upstream keeps this table rather than computing the fraction. Final Cut is
/// particular about the exact spelling, and `25/600s` for 24fps is the one it
/// writes even though `1/24s` is the same number, so the table is kept.
///
/// A rate that is not in the table is where this differs from upstream, which
/// writes an empty `frameDuration` for it: a file its own reader, and Final
/// Cut, refuse. Any Premiere or FCP 7 timeline at 15, 48 or 23.976 fps went
/// that way. Here the fraction is worked out instead — `1001/…000s` for a
/// drop-frame rate, as Final Cut spells those, and one frame's worth of
/// seconds otherwise.
fn frame_duration_for_rate(rate: f64) -> String {
    // Upstream looks the rate up as an integer first and as a float second,
    // so 23.98 misses `23` and then hits the float key.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "matching Python's int() on the frame rate"
    )]
    let whole = rate.trunc() as i64;
    let known = match whole {
        24 => Some("25/600s"),
        25 => Some("1/25s"),
        30 => Some("100/3000s"),
        50 => Some("1/50s"),
        60 => Some("1/60s"),
        _ if rate == 23.98 => Some("1001/24000s"),
        _ if rate == 29.97 => Some("1001/30000s"),
        _ if rate == 59.94 => Some("1001/60000s"),
        _ => None,
    };
    if let Some(known) = known {
        return known.to_string();
    }
    if !rate.is_finite() || rate <= 0.0 {
        return String::new();
    }

    // A drop-frame rate is a whole rate slowed by 1000/1001.
    let whole_rate = (rate * 1.001).round();
    if rate.fract() != 0.0 && (rate * 1.001 - whole_rate).abs() < 1e-3 {
        return format!("1001/{}s", whole_rate * 1000.0);
    }
    rational_number(1.0, rate)
}

/// Builds the `name` Final Cut gives a video format, such as
/// `FFVideoFormat1080p25`.
///
/// `frame_size` is what `ffprobe` reports for the media, as `widthxheight`.
/// Upstream runs `ffprobe` itself; this adapter does not shell out, so
/// nothing in a write reaches this function and every format it writes is
/// named `""` — which is also what upstream writes whenever `ffprobe` is
/// missing or the media is not on disk, as it is for every file in its own
/// test suite. The naming quirks are ported here so the rule is stated and
/// testable.
#[must_use]
pub fn format_name(frame_rate: i64, frame_size: &str) -> String {
    let frame_size = frame_size.trim_end();
    if frame_size.is_empty() {
        return String::new();
    }

    // Upstream collapses a couple of common sizes to the number a person
    // would recognise, and does it by substring, so `1920x1080` becomes
    // `1080` but `1280x720` does not become `720` — only a size that *ends*
    // in 1280 does.
    let frame_size = if frame_size.contains("1920") {
        "1080"
    } else if frame_size.ends_with("1280") {
        "720"
    } else {
        frame_size
    };

    format!("FFVideoFormat{frame_size}p{frame_rate}")
}

/// Writes a document as an FCP X XML string.
///
/// # Errors
///
/// Returns [`Error::Parse`] if the document holds a handle that is not live,
/// or an arrangement the format cannot express.
pub fn write_to_string(document: &Document) -> Result<String> {
    let settled = settle_record_offsets(document)?;
    let document = settled.as_ref().unwrap_or(document);
    let root = document
        .root()
        .ok_or_else(|| Error::unsupported("an empty document has nothing to write"))?;

    let timelines = match document.try_get(root)? {
        Node::Timeline(_) => vec![root],
        _ => {
            document.find_children(root, None, false, &|node| matches!(node, Node::Timeline(_)))?
        }
    };

    let mut writer = Writer {
        document,
        resources: Element::new("resources"),
        event_children: Vec::new(),
        resource_count: 0,
    };

    for &timeline in &timelines {
        let Node::Timeline(data) = document.try_get(timeline)? else {
            continue;
        };
        let tracks = data
            .tracks
            .ok_or_else(|| Error::unsupported("a timeline with no tracks has nothing to write"))?;
        let sequence = writer.stack_to_sequence(tracks, false)?;

        let mut project = Element::new("project");
        project.attributes.set("name", data.base.name.clone());
        project
            .attributes
            .set("uid", metadata_uid(&data.base.metadata));
        project.push(sequence);
        writer.event_children.push(project);
    }

    if timelines.is_empty() {
        // A document of loose clips rather than an edit: every clip becomes an
        // asset, and every compound clip a `ref-clip` standing on its own.
        for clip in
            document.find_children(root, None, false, &|node| matches!(node, Node::Clip(_)))?
        {
            if document.try_get(clip)?.parent().is_none() {
                writer.add_asset(clip, false)?;
            }
        }
        for stack in
            document.find_children(root, None, false, &|node| matches!(node, Node::Stack(_)))?
        {
            if let Some(element) = writer.element_for_item(stack, None, true, true)? {
                writer.event_children.push(element);
            }
        }
    }

    let event_name = writer.event_name(root)?;
    let mut fcpxml = Element::new("fcpxml");
    fcpxml.attributes.set("version", "1.8");
    fcpxml.push(writer.resources);
    if timelines.len() > 1 {
        let mut event = Element::new("event");
        event.attributes.set("name", event_name);
        event.children = writer.event_children;
        fcpxml.push(event);
    } else {
        fcpxml.children.extend(writer.event_children);
    }

    // Markers are written as soon as their item is built, but lane items are
    // appended to that item afterwards, so a marker can end up in the middle
    // of its parent's children. Final Cut wants them last.
    move_markers_last(&mut fcpxml);

    Ok(otio_xml::to_pretty_string(&fcpxml).replacen(
        "<?xml version=\"1.0\" ?>",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n",
        1,
    ))
}

/// Turns tracks whose range starts before zero into tracks that start with a
/// gap, returning `None` when no track does.
///
/// That is how the EDL reader, following upstream's, records where a track
/// starts in record time: a track whose first event is at `01:00:00:00` gets
/// a range starting an hour *before* zero and as long as its events. Read
/// strictly, such a range shows nothing — it ends before the events begin —
/// so every item on the track is trimmed away, which upstream's writer then
/// fails on. The range is read here the way it was meant instead: each track
/// is moved later by its offset, less the offset every track shares, since a
/// sequence starts at its first edit.
fn settle_record_offsets(document: &Document) -> Result<Option<Document>> {
    let offset_of = |track: NodeId| -> Result<Option<RationalTime>> {
        Ok(match document.try_get(track)? {
            Node::Track(data) => data
                .item
                .source_range
                .map(|range| range.start_time())
                .filter(|start| start.value() < 0.0),
            _ => None,
        })
    };

    let mut stacks = Vec::new();
    for (id, node) in document.iter() {
        let Node::Timeline(data) = node else {
            continue;
        };
        let Some(stack) = data.tracks else {
            continue;
        };
        let mut offset_tracks = false;
        for track in document.children_of(stack)? {
            offset_tracks |= offset_of(track)?.is_some();
        }
        if offset_tracks {
            stacks.push((id, stack));
        }
    }
    if stacks.is_empty() {
        return Ok(None);
    }

    let mut settled = document.clone();
    for (_, stack) in stacks {
        let tracks = document.children_of(stack)?;
        // How far into record time each track starts. An empty track has no
        // first edit to say where the sequence starts.
        let mut leads = Vec::with_capacity(tracks.len());
        for &track in &tracks {
            let lead = match offset_of(track)? {
                Some(start) => -start,
                None => RationalTime::new(0.0, document.duration(track)?.rate()),
            };
            let occupied = !document.children_of(track)?.is_empty();
            leads.push((track, lead, occupied));
        }
        let Some(shared) = leads
            .iter()
            .filter(|(_, _, occupied)| *occupied)
            .map(|(_, lead, _)| *lead)
            .reduce(|a, b| if b < a { b } else { a })
        else {
            continue;
        };

        for (track, lead, _) in leads {
            if offset_of(track)?.is_none() {
                continue;
            }
            let content = document.available_range(track)?.duration();
            if let Some(item) = settled.try_get_mut(track)?.item_mut() {
                // The range's length is the events', so it says nothing the
                // track does not unless it is shorter.
                item.source_range = item
                    .source_range
                    .map(|range| range.duration())
                    .filter(|&duration| duration < content)
                    .map(|duration| {
                        TimeRange::new(RationalTime::new(0.0, duration.rate()), duration)
                    });
            }
            let gap = (lead - shared).rescaled_to(content.rate());
            if gap.value() > 0.0 {
                let gap = settled.insert(Node::Gap(Gap {
                    item: ItemData {
                        source_range: Some(TimeRange::new(RationalTime::new(0.0, gap.rate()), gap)),
                        ..ItemData::new()
                    },
                }));
                settled.insert_child(track, 0, gap)?;
            }
        }
    }
    Ok(Some(settled))
}

struct Writer<'a> {
    document: &'a Document,
    /// The `resources` table: formats, assets and compound clips.
    resources: Element,
    /// What goes under the `event` element, or straight under the root when
    /// there is only one timeline and so no event.
    event_children: Vec<Element>,
    resource_count: u32,
}

impl Writer<'_> {
    // ------------------------------------------------------- compositions --

    /// Turns a stack of tracks into a `sequence` with one `spine`.
    fn stack_to_sequence(&mut self, stack: NodeId, compound: bool) -> Result<Element> {
        let format_id = self.find_or_create_format(stack)?;
        let duration = self.document.duration(stack)?;

        let mut sequence = Element::new("sequence");
        sequence.attributes.set(
            "duration",
            rational_number(duration.value(), duration.rate()),
        );
        sequence.attributes.set("format", format_id);

        let mut spine = Element::new("spine");
        let tracks = self.document.children_of(stack)?;
        let of_kind = |kind: &str| -> Vec<NodeId> {
            tracks
                .iter()
                .copied()
                .filter(|&track| match self.document.try_get(track) {
                    Ok(Node::Track(data)) => data.kind == kind,
                    _ => false,
                })
                .collect()
        };
        let video = of_kind("Video");
        let audio = of_kind("Audio");

        // Where the storyline stops short of the sequence — a timeline with no
        // picture at all, or one whose first video track ends before another
        // track does — whatever lies past it has nothing to hang off. Final
        // Cut fills such a stretch of storyline with a gap, so that is what is
        // added, the first time something needs it.
        let storyline_end = match video.first() {
            Some(&track) => self.document.duration(track)?,
            None => RationalTime::new(0.0, duration.rate()),
        };
        let mut padding = (storyline_end < duration).then_some((storyline_end, duration));

        for (index, track) in video.into_iter().enumerate() {
            let lane = i64::try_from(index).unwrap_or(i64::MAX);
            self.track_for_spine(track, lane, &mut spine, compound, &mut padding)?;
        }
        // Audio hangs below the storyline, so its lanes count downwards.
        for (index, track) in audio.into_iter().enumerate() {
            let lane = -(i64::try_from(index).unwrap_or(i64::MAX) + 1);
            self.track_for_spine(track, lane, &mut spine, compound, &mut padding)?;
        }

        sequence.push(spine);
        Ok(sequence)
    }

    /// Lays one track's items into the spine.
    ///
    /// Lane zero is the storyline itself, so its items go straight in.
    /// Everything else is attached to whichever storyline item it sits over,
    /// with its offset restated in that item's own clock. `padding` is the
    /// stretch past the storyline's end still to be filled with a gap, taken
    /// the first time an item lands there.
    fn track_for_spine(
        &mut self,
        track: NodeId,
        lane: i64,
        spine: &mut Element,
        compound: bool,
        padding: &mut Option<(RationalTime, RationalTime)>,
    ) -> Result<()> {
        let items = self.document.find_children(track, None, false, &|node| {
            matches!(node, Node::Gap(_) | Node::Stack(_) | Node::Clip(_))
        })?;

        for item in items {
            // The contents of a compound clip are written once, inside the
            // compound clip itself, not again in the timeline that uses it.
            if !compound && self.in_compound_clip(item)? {
                continue;
            }

            let Some(mut element) = self.element_for_item(item, Some(lane), false, compound)?
            else {
                continue;
            };

            if lane == 0 {
                spine.push(element);
                continue;
            }
            // A gap is the absence of a clip, which off the storyline is just
            // nothing at all.
            if matches!(self.document.try_get(item)?, Node::Gap(_)) {
                continue;
            }

            let start = self
                .document
                .trimmed_range_of_child(track, item)?
                .ok_or_else(|| {
                    Error::unsupported(
                        "an item trimmed out of its track has no place to be written",
                    )
                })?
                .start_time();
            let track_format = self.find_or_create_format(track)?;

            let mut path = find_parent_path(&self.resources, spine, start, &track_format)?;
            if path.is_none() {
                if let Some((from, to)) = padding.filter(|(from, _)| start >= *from) {
                    // Upstream dereferences the missing parent and raises.
                    // Deliberate deviation: past the storyline's end, the
                    // storyline is lengthened with a gap to hang it off.
                    *padding = None;
                    spine.push(padding_gap(from, to));
                    path = find_parent_path(&self.resources, spine, start, &track_format)?;
                }
            }
            let Some(path) = path else {
                // Nothing in the storyline covers this item, so there is
                // nowhere in the format to hang it: say so rather than
                // crashing.
                return Err(Error::unsupported(format!(
                    "no storyline item covers lane {lane} at {} frames, so there is \
                     nothing to attach to",
                    start.value()
                )));
            };
            let parent = element_at_path(spine, &path);
            let offset = offset_based_on_parent(&self.resources, &element, parent, &track_format)?;
            element.attributes.set("offset", from_rational_time(offset));
            element_at_path_mut(spine, &path).push(element);
        }

        Ok(())
    }

    // -------------------------------------------------------------- items --

    /// Builds the element for one composable, with its markers.
    fn element_for_item(
        &mut self,
        item: NodeId,
        lane: Option<i64>,
        reference_only: bool,
        compound: bool,
    ) -> Result<Option<Element>> {
        let total = self.document.duration(item)?;
        let duration = rational_number(total.value(), total.rate());

        let mut element = match self.document.try_get(item)? {
            Node::Clip(_) => {
                let asset_id = self.add_asset(item, compound)?;
                Some(self.clip_element(item, &asset_id, &duration, lane)?)
            }
            Node::Gap(_) => Some(self.gap_element(item, &duration)?),
            Node::Stack(_) => Some(self.stack_element(item, &duration, reference_only)?),
            _ => None,
        };

        let Some(element) = element.as_mut() else {
            return Ok(None);
        };
        if let Some(lane) = lane_attribute(lane) {
            element.attributes.set("lane", lane);
        }

        let markers = self
            .document
            .try_get(item)?
            .item()
            .map(|data| data.markers.clone())
            .unwrap_or_default();
        for marker in markers {
            let Node::Marker(data) = self.document.try_get(marker)? else {
                continue;
            };
            let mut marker_element = Element::new("marker");
            marker_element
                .attributes
                .set("start", from_rational_time(data.marked_range.start_time()));
            marker_element
                .attributes
                .set("duration", from_rational_time(data.marked_range.duration()));
            marker_element
                .attributes
                .set("value", data.base.name.clone());
            // Purple is what Final Cut shows for a marker with no state, so
            // it is spelled by leaving the attribute off.
            match data.color.as_ref().map(|color| color.name.as_str()) {
                Some("Red") => marker_element.attributes.set("completed", "0"),
                Some("Green") => marker_element.attributes.set("completed", "1"),
                _ => {}
            }
            element.push(marker_element);
        }

        Ok(element.clone().into())
    }

    fn clip_element(
        &mut self,
        item: NodeId,
        asset_id: &str,
        duration: &str,
        lane: Option<i64>,
    ) -> Result<Element> {
        let name = self.document.try_get(item)?.name().to_string();
        let mut element = Element::new("clip");
        element.attributes.set("name", name);
        element
            .attributes
            .set("offset", from_rational_time(self.offset_in_parent(item)?));
        element.attributes.set("duration", duration);

        let source_start = self.source_range(item)?.start_time();
        let start = from_rational_time(source_start);
        if start != "0s" {
            element.attributes.set("start", start);
        }

        let asset_duration = self.asset_duration(item)?;
        if self.parent_kind(item)?.as_deref() != Some("Audio") {
            let mut video = Element::new("video");
            video.attributes.set("offset", "0s");
            video.attributes.set("ref", asset_id);
            video.attributes.set("duration", asset_duration);
            element.push(video);
        } else {
            // Audio hangs inside a gap, which is how Final Cut spells a clip
            // that contributes sound but no picture.
            let mut gap = Element::new("gap");
            gap.attributes.set("name", "Gap");
            gap.attributes.set("offset", "0s");
            gap.attributes.set("duration", &asset_duration);

            let mut audio = Element::new("audio");
            audio.attributes.set("offset", "0s");
            audio.attributes.set("ref", asset_id);
            audio.attributes.set("duration", &asset_duration);
            if let Some(lane) = lane_attribute(lane) {
                audio.attributes.set("lane", lane);
            }
            gap.push(audio);
            element.push(gap);
        }

        Ok(element)
    }

    fn gap_element(&self, item: NodeId, duration: &str) -> Result<Element> {
        let mut element = Element::new("gap");
        element.attributes.set("name", "Gap");
        element.attributes.set("duration", duration);
        element
            .attributes
            .set("offset", from_rational_time(self.offset_in_parent(item)?));
        // An hour in is where Final Cut's own timeline starts, and it writes
        // every gap as starting there.
        element.attributes.set("start", "3600s");
        Ok(element)
    }

    fn stack_element(
        &mut self,
        item: NodeId,
        duration: &str,
        reference_only: bool,
    ) -> Result<Element> {
        let media_id = self.add_compound_clip(item)?;
        let name = self.document.try_get(item)?.name().to_string();

        let mut element = Element::new("ref-clip");
        element.attributes.set("name", name);
        element.attributes.set("duration", duration);
        element.attributes.set("ref", media_id);

        if !reference_only {
            element
                .attributes
                .set("offset", from_rational_time(self.offset_in_parent(item)?));
            element.attributes.set(
                "start",
                from_rational_time(self.source_range(item)?.start_time()),
            );
        }
        if self.parent_kind(item)?.as_deref() == Some("Audio") {
            element.attributes.set("srcEnable", "audio");
        }

        Ok(element)
    }

    // --------------------------------------------------------- resources --

    fn next_resource_id(&mut self) -> String {
        self.resource_count += 1;
        format!("r{}", self.resource_count)
    }

    /// Returns the id of the format for an object's frame rate, creating it if
    /// this is the first object at that rate.
    fn find_or_create_format(&mut self, item: NodeId) -> Result<String> {
        let rate = self.document.duration(item)?.rate();
        let frame_duration = frame_duration_for_rate(rate);
        let name = self.format_name_for(item)?;

        if let Some(existing) = self.resources.children.iter_mut().find(|child| {
            child.tag == "format" && child.attributes.get("frameDuration") == Some(&frame_duration)
        }) {
            // A format created for a clip with no probeable media has an empty
            // name; the first clip that can name it fills it in.
            if existing
                .attributes
                .get("name")
                .unwrap_or_default()
                .is_empty()
            {
                existing.attributes.set("name", name);
            }
            return Ok(existing
                .attributes
                .get("id")
                .unwrap_or_default()
                .to_string());
        }

        let id = self.next_resource_id();
        let mut format = Element::new("format");
        format.attributes.set("id", &id);
        format.attributes.set("frameDuration", frame_duration);
        format.attributes.set("name", name);
        self.resources.push(format);
        Ok(id)
    }

    /// Adds an asset for a clip's media, and the event clip that describes it.
    ///
    /// Returns the asset's resource id.
    fn add_asset(&mut self, clip: NodeId, compound_only: bool) -> Result<String> {
        let format_id = self.find_or_create_format(clip)?;
        let asset_id = self.create_asset_element(clip, &format_id)?;

        let name = self.document.try_get(clip)?.name().to_string();
        let already_described = self
            .event_children
            .iter()
            .any(|child| child.tag == "asset-clip" && child.attributes.get("name") == Some(&name));
        if !compound_only && !already_described {
            self.create_asset_clip_element(clip, &format_id, &asset_id)?;
        }

        let kind = self.parent_kind(clip)?;
        let Some(asset) =
            self.resources.children.iter_mut().find(|child| {
                child.tag == "asset" && child.attributes.get("id") == Some(&asset_id)
            })
        else {
            return Ok(asset_id);
        };
        match kind.as_deref() {
            // A clip standing on its own, outside any track, could be either,
            // so it is written as both.
            None => {
                asset.attributes.set("hasAudio", "1");
                asset.attributes.set("hasVideo", "1");
            }
            Some("Audio") => asset.attributes.set("hasAudio", "1"),
            Some("Video") => asset.attributes.set("hasVideo", "1"),
            Some(_) => {}
        }
        Ok(asset_id)
    }

    /// Returns the id of the asset for a clip's media, creating it if this is
    /// the first clip to use that file.
    fn create_asset_element(&mut self, clip: NodeId, format_id: &str) -> Result<String> {
        let target_url = self.target_url(clip)?;
        if let Some(existing) = self.resources.children.iter().find(|child| {
            child.tag == "asset" && child.attributes.get("src") == Some(target_url.as_str())
        }) {
            return Ok(existing
                .attributes
                .get("id")
                .unwrap_or_default()
                .to_string());
        }

        let id = self.next_resource_id();
        let mut asset = Element::new("asset");
        asset
            .attributes
            .set("name", self.document.try_get(clip)?.name().to_string());
        asset.attributes.set("src", target_url);
        asset.attributes.set("format", format_id);
        asset.attributes.set("id", &id);
        asset.attributes.set("duration", self.asset_duration(clip)?);
        asset.attributes.set("start", self.asset_start(clip)?);
        asset.attributes.set("hasAudio", "0");
        asset.attributes.set("hasVideo", "0");
        self.resources.push(asset);
        Ok(id)
    }

    /// Adds the event's own clip for an asset, which carries what Final Cut
    /// knows about the media: its note, its keywords and its metadata.
    fn create_asset_clip_element(
        &mut self,
        clip: NodeId,
        format_id: &str,
        asset_id: &str,
    ) -> Result<()> {
        let mut element = Element::new("asset-clip");
        element
            .attributes
            .set("name", self.document.try_get(clip)?.name().to_string());
        element.attributes.set("format", format_id);
        element.attributes.set("ref", asset_id);
        element
            .attributes
            .set("duration", self.asset_duration(clip)?);

        if let Some(detail) = self.media_metadata(clip)? {
            if let Some(note) = detail.get("note").and_then(Any::as_str) {
                if !note.is_empty() {
                    element.push(Element::with_text("note", note));
                }
            }
            for keyword in detail
                .get("keywords")
                .and_then(Any::as_slice)
                .unwrap_or_default()
            {
                let Some(fields) = keyword.as_dictionary() else {
                    continue;
                };
                let mut keyword_element = Element::new("keyword");
                for (key, value) in fields {
                    if let Some(value) = value.as_str() {
                        keyword_element.attributes.set(key.clone(), value);
                    }
                }
                element.push(keyword_element);
            }
            if let Some(entries) = detail.get("metadata").and_then(Any::as_slice) {
                let mut metadata = Element::new("metadata");
                for entry in entries {
                    let Some(fields) = entry.as_dictionary() else {
                        continue;
                    };
                    for (key, value) in fields {
                        let mut md = Element::new("md");
                        md.attributes.set("key", key.clone());
                        md.attributes
                            .set("value", value.as_str().unwrap_or_default());
                        metadata.push(md);
                    }
                }
                element.push(metadata);
            }
        }

        self.event_children.push(element);
        Ok(())
    }

    /// Adds a compound clip for a stack, and returns its resource id.
    fn add_compound_clip(&mut self, item: NodeId) -> Result<String> {
        let name = self.document.try_get(item)?.name().to_string();
        if let Some(existing) = self
            .resources
            .children
            .iter()
            .find(|child| child.tag == "media" && child.attributes.get("name") == Some(&name))
        {
            return Ok(existing
                .attributes
                .get("id")
                .unwrap_or_default()
                .to_string());
        }

        let id = self.next_resource_id();
        let mut media = Element::new("media");
        media.attributes.set(
            "name",
            if name.is_empty() {
                format!("compound_clip_{id}")
            } else {
                name
            },
        );
        media.attributes.set("id", &id);
        let uid = self
            .document
            .try_get(item)?
            .base()
            .map(|base| metadata_uid(&base.metadata))
            .unwrap_or_default();
        if !uid.is_empty() {
            media.attributes.set("uid", uid);
        }

        // The compound clip goes into the table before its own sequence is
        // built, so a compound clip that contains itself is found rather than
        // recursed into forever.
        self.resources.push(media);
        let sequence = self.stack_to_sequence(item, true)?;
        if let Some(media) =
            self.resources.children.iter_mut().find(|child| {
                child.tag == "media" && child.attributes.get("id") == Some(id.as_str())
            })
        {
            media.push(sequence);
        }
        Ok(id)
    }

    // ----------------------------------------------------------- helpers --

    /// Returns the name of the event holding this document's projects.
    fn event_name(&self, root: NodeId) -> Result<String> {
        let name = self.document.try_get(root)?.name().to_string();
        if !name.is_empty() {
            return Ok(name);
        }
        // Upstream falls back to today's date, which would make a write
        // irreproducible. An unnamed event is written unnamed instead.
        Ok(String::new())
    }

    fn source_range(&self, item: NodeId) -> Result<TimeRange> {
        self.document
            .try_get(item)?
            .item()
            .and_then(|data| data.source_range)
            .ok_or_else(|| {
                Error::unsupported("an item with no source range has no duration to write")
            })
    }

    fn offset_in_parent(&self, item: NodeId) -> Result<RationalTime> {
        Ok(self
            .document
            .trimmed_range_in_parent(item)?
            .ok_or_else(|| {
                Error::unsupported("an item trimmed out of its track has no offset to write")
            })?
            .start_time())
    }

    /// Returns the kind of the track an item sits on, if it sits on one.
    fn parent_kind(&self, item: NodeId) -> Result<Option<String>> {
        let Some(parent) = self.document.try_get(item)?.parent() else {
            return Ok(None);
        };
        Ok(match self.document.try_get(parent)? {
            Node::Track(track) => Some(track.kind.clone()),
            _ => None,
        })
    }

    /// Returns whether an item is inside a compound clip.
    ///
    /// One stack above it is the timeline's own; two or more means it belongs
    /// to a compound clip, which is written once in the resources table.
    fn in_compound_clip(&self, item: NodeId) -> Result<bool> {
        let mut stacks = 0;
        let mut current = self.document.try_get(item)?.parent();
        while let Some(id) = current {
            let node = self.document.try_get(id)?;
            if matches!(node, Node::Stack(_)) {
                stacks += 1;
            }
            current = node.parent();
        }
        Ok(stacks > 1)
    }

    /// Returns a clip's active media reference, if it has one that is not a
    /// stand-in for missing media.
    fn media_reference(&self, clip: NodeId) -> Result<Option<NodeId>> {
        let Node::Clip(data) = self.document.try_get(clip)? else {
            return Ok(None);
        };
        let Some(&reference) = data.media_references.get(&data.active_media_reference_key) else {
            return Ok(None);
        };
        Ok(match self.document.try_get(reference)? {
            Node::MissingReference(_) => None,
            _ => Some(reference),
        })
    }

    fn media_metadata(&self, clip: NodeId) -> Result<Option<&AnyDictionary>> {
        let Some(reference) = self.media_reference(clip)? else {
            return Ok(None);
        };
        Ok(self
            .document
            .try_get(reference)?
            .base()
            .and_then(|base| base.metadata.get(META_NAMESPACE))
            .and_then(Any::as_dictionary))
    }

    /// Returns the length of the media behind a clip, falling back to the
    /// length of the clip itself when none is known.
    fn asset_duration(&self, clip: NodeId) -> Result<String> {
        if let Some(range) = self.available_range(clip)? {
            let duration = range.duration();
            return Ok(rational_number(duration.value(), duration.rate()));
        }
        let duration = self.document.duration(clip)?;
        Ok(rational_number(duration.value(), duration.rate()))
    }

    /// Returns where the media behind a clip starts, falling back to where the
    /// clip starts in it.
    fn asset_start(&self, clip: NodeId) -> Result<String> {
        if let Some(range) = self.available_range(clip)? {
            let start = range.start_time();
            return Ok(rational_number(start.value(), start.rate()));
        }
        let start = self.source_range(clip)?.start_time();
        Ok(rational_number(start.value(), start.rate()))
    }

    fn available_range(&self, clip: NodeId) -> Result<Option<TimeRange>> {
        let Some(reference) = self.media_reference(clip)? else {
            return Ok(None);
        };
        Ok(self
            .document
            .try_get(reference)?
            .media()
            .and_then(|data| data.available_range))
    }

    /// Returns the file a clip's media lives at.
    ///
    /// A clip with no media still needs a distinct `src`, because that is what
    /// the writer keys assets on, so it gets a made-up path under `/tmp`.
    fn target_url(&self, clip: NodeId) -> Result<String> {
        if let Some(reference) = self.media_reference(clip)? {
            if let Node::ExternalReference(data) = self.document.try_get(reference)? {
                return Ok(data.target_url.clone());
            }
        }
        Ok(format!(
            "file:///tmp/{}",
            self.document.try_get(clip)?.name()
        ))
    }

    /// Returns the format name for an object's media.
    ///
    /// Always empty: naming a format means probing the media for its frame
    /// size, which this adapter does not do. See [`format_name`].
    fn format_name_for(&self, item: NodeId) -> Result<String> {
        let _ = self.document.try_get(item)?;
        Ok(String::new())
    }
}

// ------------------------------------------------------- spine placement --

/// A storyline gap from `from` to `to`, for lane items past the storyline's
/// end to hang off. Written as [`Writer::gap_element`] writes a gap.
fn padding_gap(from: RationalTime, to: RationalTime) -> Element {
    let length = to - from;
    let mut element = Element::new("gap");
    element.attributes.set("name", "Gap");
    element
        .attributes
        .set("duration", rational_number(length.value(), length.rate()));
    element.attributes.set("offset", from_rational_time(from));
    element.attributes.set("start", "3600s");
    element
}

/// Returns the path to the storyline item covering `at`, if there is one.
///
/// The path is a list of child indices from the spine down, because the
/// element it names is about to be appended to and Rust will not hold a
/// reference across that.
fn find_parent_path(
    resources: &Element,
    spine: &Element,
    at: RationalTime,
    default_format: &str,
) -> Result<Option<Vec<usize>>> {
    let mut stack: Vec<(Vec<usize>, &Element)> = vec![(Vec::new(), spine)];
    while let Some((path, element)) = stack.pop() {
        // Push children in reverse so the walk stays in document order.
        for (index, child) in element.children.iter().enumerate().rev() {
            let mut child_path = path.clone();
            child_path.push(index);
            stack.push((child_path, child));
        }

        if !matches!(
            element.tag.as_str(),
            "clip" | "asset-clip" | "gap" | "ref-clip"
        ) {
            continue;
        }
        // An item that is itself in a lane is not part of the storyline.
        if element.attributes.contains("lane") {
            continue;
        }
        // A gap holding audio is a clip with no picture, not a hole.
        if element.tag == "gap" && element.find("audio").is_some() {
            continue;
        }

        let rate = frame_rate_from_element(resources, element, default_format)?;
        let offset = RationalTime::new(frames_at(element.attributes.get("offset"), rate), rate);
        let duration = RationalTime::new(frames_at(element.attributes.get("duration"), rate), rate);
        if offset > at {
            continue;
        }
        if offset + duration > at {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Restates a child's offset in its new parent's clock.
fn offset_based_on_parent(
    resources: &Element,
    child: &Element,
    parent: &Element,
    default_format: &str,
) -> Result<RationalTime> {
    let parent_rate = frame_rate_from_element(resources, parent, default_format)?;
    let child_rate = frame_rate_from_element(resources, child, default_format)?;

    let parent_offset = RationalTime::new(
        frames_at(parent.attributes.get("offset"), parent_rate),
        parent_rate,
    );
    let parent_start = RationalTime::new(
        frames_at(parent.attributes.get("start"), parent_rate),
        parent_rate,
    );
    let child_offset = RationalTime::new(
        frames_at(child.attributes.get("offset"), child_rate),
        child_rate,
    );

    Ok((child_offset - parent_offset) + parent_start)
}

/// Returns the frame rate an element's times are counted in.
fn frame_rate_from_element(
    resources: &Element,
    element: &Element,
    default_format: &str,
) -> Result<f64> {
    let format_id = match element.tag.as_str() {
        "ref-clip" => {
            let reference = element.attributes.get("ref").unwrap_or_default();
            resource_by_id(resources, "media", reference)
                .and_then(|media| media.find("sequence"))
                .and_then(|sequence| sequence.attributes.get("format"))
                .unwrap_or(default_format)
        }
        "clip" => {
            let asset_id = match element.find("gap").and_then(|gap| gap.find("audio")) {
                Some(audio) => audio.attributes.get("ref"),
                None => element.find("video").and_then(|v| v.attributes.get("ref")),
            }
            .unwrap_or_default();
            resource_by_id(resources, "asset", asset_id)
                .and_then(|asset| asset.attributes.get("format"))
                .unwrap_or(default_format)
        }
        "asset-clip" => {
            let reference = element.attributes.get("ref").unwrap_or_default();
            resource_by_id(resources, "asset", reference)
                .and_then(|asset| asset.attributes.get("format"))
                .unwrap_or(default_format)
        }
        _ => default_format,
    };

    let format = resource_by_id(resources, "format", format_id)
        .ok_or_else(|| Error::unsupported(format!("no format with id `{format_id}`")))?;
    let duration = format.attributes.get("frameDuration").ok_or_else(|| {
        Error::unsupported(format!("format `{format_id}` states no frame duration"))
    })?;
    let (total, rate) = duration.split_once('/').ok_or_else(|| {
        Error::unsupported(format!(
            "format `{format_id}` states a frame duration of `{duration}`, \
             which is not a fraction"
        ))
    })?;
    let total: f64 = total.parse().unwrap_or(1.0);
    let rate: f64 = rate.trim_end_matches('s').parse().unwrap_or(0.0);
    if total == 0.0 {
        return Ok(0.0);
    }
    Ok((rate / total).trunc())
}

fn resource_by_id<'a>(resources: &'a Element, tag: &str, id: &str) -> Option<&'a Element> {
    resources
        .children
        .iter()
        .find(|child| child.tag == tag && child.attributes.get("id") == Some(id))
}

fn element_at_path<'a>(root: &'a Element, path: &[usize]) -> &'a Element {
    let mut element = root;
    for &index in path {
        element = &element.children[index];
    }
    element
}

fn element_at_path_mut<'a>(root: &'a mut Element, path: &[usize]) -> &'a mut Element {
    let mut element = root;
    for &index in path {
        element = &mut element.children[index];
    }
    element
}

// ------------------------------------------------------------- utilities --

/// Returns the lane attribute for a lane number, which lane zero does not get.
///
/// Lane zero is the storyline, and Final Cut spells that by leaving the
/// attribute off rather than by writing `lane="0"`.
fn lane_attribute(lane: Option<i64>) -> Option<String> {
    lane.filter(|lane| *lane != 0).map(|lane| lane.to_string())
}

/// Returns the `uid` Final Cut gave an object, if it was read from a file it
/// wrote.
fn metadata_uid(metadata: &AnyDictionary) -> String {
    metadata
        .get(META_NAMESPACE)
        .and_then(Any::as_dictionary)
        .and_then(|detail| detail.get("uid"))
        .and_then(Any::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Moves every `marker` to the end of its parent's children, in place.
fn move_markers_last(element: &mut Element) {
    for child in &mut element.children {
        move_markers_last(child);
    }
    let markers: Vec<Element> = element
        .children
        .iter()
        .filter(|child| child.tag == "marker")
        .cloned()
        .collect();
    if markers.is_empty() {
        return;
    }
    element.children.retain(|child| child.tag != "marker");
    element.children.extend(markers);
}

#[cfg(test)]
mod tests {
    use otio_xml::Element;

    use super::{frame_duration_for_rate, lane_attribute, move_markers_last};

    /// Final Cut is particular about the spelling of a frame duration, and
    /// the table is the only place these exact fractions come from.
    #[test]
    fn a_frame_rate_maps_to_the_fraction_final_cut_writes() {
        assert_eq!(frame_duration_for_rate(24.0), "25/600s");
        assert_eq!(frame_duration_for_rate(25.0), "1/25s");
        assert_eq!(frame_duration_for_rate(30.0), "100/3000s");
        assert_eq!(frame_duration_for_rate(50.0), "1/50s");
        assert_eq!(frame_duration_for_rate(60.0), "1/60s");
    }

    /// The drop-frame rates are looked up as floats, because truncating them
    /// to an integer misses: 23.98 is not 23.
    #[test]
    fn a_drop_frame_rate_is_matched_as_a_fraction_of_a_frame() {
        assert_eq!(frame_duration_for_rate(23.98), "1001/24000s");
        assert_eq!(frame_duration_for_rate(29.97), "1001/30000s");
        assert_eq!(frame_duration_for_rate(59.94), "1001/60000s");
    }

    /// A rate the table does not have gets its fraction worked out, where
    /// upstream writes an empty `frameDuration` that no reader accepts.
    #[test]
    fn an_unknown_rate_gets_its_frame_duration_worked_out() {
        assert_eq!(frame_duration_for_rate(15.0), "1/15s");
        assert_eq!(frame_duration_for_rate(48.0), "1/48s");
        assert_eq!(frame_duration_for_rate(12.5), "2/25s");
        assert_eq!(frame_duration_for_rate(23.976), "1001/24000s");
        assert_eq!(frame_duration_for_rate(24000.0 / 1001.0), "1001/24000s");
        assert_eq!(frame_duration_for_rate(119.88), "1001/120000s");
        assert_eq!(frame_duration_for_rate(0.0), "");
    }

    /// Lane zero is the storyline, which Final Cut spells by leaving the
    /// attribute off rather than by writing it.
    #[test]
    fn the_storyline_carries_no_lane_number() {
        assert_eq!(lane_attribute(None), None);
        assert_eq!(lane_attribute(Some(0)), None);
        assert_eq!(lane_attribute(Some(1)), Some("1".to_string()));
        assert_eq!(lane_attribute(Some(-1)), Some("-1".to_string()));
    }

    /// Markers are written before the lane items that attach to their clip,
    /// so they have to be moved back to the end afterwards.
    #[test]
    fn markers_end_up_last_among_their_siblings() {
        let mut clip = Element::new("clip");
        clip.push(Element::new("video"));
        clip.push(Element::new("marker"));
        clip.push(Element::new("asset-clip"));
        clip.push(Element::new("marker"));

        move_markers_last(&mut clip);

        let tags: Vec<&str> = clip.children.iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(tags, ["video", "asset-clip", "marker", "marker"]);
    }

    /// The move reaches all the way down, because a marker on a lane item is
    /// nested inside the storyline item it hangs off.
    #[test]
    fn a_marker_deep_in_the_spine_is_moved_too() {
        let mut inner = Element::new("clip");
        inner.push(Element::new("marker"));
        inner.push(Element::new("video"));

        let mut outer = Element::new("clip");
        outer.push(inner);

        move_markers_last(&mut outer);

        let tags: Vec<&str> = outer.children[0]
            .children
            .iter()
            .map(|c| c.tag.as_str())
            .collect();
        assert_eq!(tags, ["video", "marker"]);
    }
}

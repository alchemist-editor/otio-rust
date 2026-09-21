//! Writing a timeline out as an EDL.
//!
//! The awkward part is that an EDL states a transition differently from the
//! way a timeline holds one. A timeline has the transition straddling the cut,
//! reaching back into the outgoing clip and forward into the incoming one:
//!
//! ```text
//!            |---57.0 Trans 43.0----|
//! |------Clip1 102.0------|----------Clip2 143.0----------|Clip3 24.0|
//! ```
//!
//! An EDL states the same thing as the outgoing clip ending early and the
//! incoming one starting early, with the whole of the transition reaching
//! forward:
//!
//! ```text
//!            |---0.0 Trans 100.0----|
//! |Clip1 45.0|----------------Clip2 200.0-----------------|Clip3 24.0|
//! ```
//!
//! So writing starts by restating the track that way. Upstream does this by
//! editing the caller's timeline in place, which leaves the document changed
//! after a write. Here the work happens on a copy.

use std::fmt::Write as _;

use opentime::{DropFrame, RationalTime, TimeRange};
use otio_adapter::cdl::Cdl;
use otio_adapter::text::float;
use otio_adapter::{Error, Result};
use otio_core::schema::{Gap, ItemData, Node};
use otio_core::{Any, AnyDictionary, Document, NodeId};

use crate::image_sequence;
use crate::path::{basename, flip_windows_slashes, strip_alphabetic_extension};
use crate::{Style, WriteOptions};

/// The reel name for a source with nothing to name it by.
const AUXILIARY_REEL: &str = "AX";

/// The reel name for black.
const BLACK_REEL: &str = "BL";

/// Writes an EDL.
pub fn write(document: &Document, options: &WriteOptions) -> Result<String> {
    // The transition rewrite above edits items, so work on a copy and leave
    // the caller's document as it was.
    let mut scratch = document.clone();

    let root = scratch
        .root()
        .ok_or_else(|| Error::unsupported("the document has no root object to write"))?;
    let Node::Timeline(timeline) = scratch.try_get(root)? else {
        return Err(Error::unsupported(format!(
            "an EDL is a whole timeline; cannot write a {}",
            scratch.try_get(root)?.schema_name()
        )));
    };
    let title = timeline.base.name.clone();
    let stack = timeline
        .tracks
        .ok_or_else(|| Error::unsupported("the timeline holds no tracks"))?;

    let tracks = scratch.children_of(stack)?;
    let (mut video, mut audio) = (0, 0);
    for track in &tracks {
        let Node::Track(track) = scratch.try_get(*track)? else {
            continue;
        };
        if !track.item.enabled {
            continue;
        }
        match track.kind.as_str() {
            "Video" => video += 1,
            "Audio" => audio += 1,
            _ => {}
        }
    }
    if video != 1 {
        return Err(Error::unsupported(format!(
            "only a single video track is supported, got: {video}"
        )));
    }
    if audio > 2 {
        return Err(Error::unsupported("no more than 2 audio tracks are supported"));
    }

    let Some(&first) = tracks.first() else {
        return Err(Error::unsupported("the timeline holds no tracks"));
    };
    // Every track is assumed to be at the rate of the first, which is what an
    // EDL can say: it states one rate for the whole file, implicitly.
    let rate = match options.rate {
        Some(rate) => rate,
        None => scratch.duration(first)?.rate(),
    };

    Writer {
        document: &mut scratch,
        stack,
        rate,
        style: options.style,
        reelname_len: options.reelname_len,
    }
    .content_for_track(first, &title)
}

/// One line of an EDL event.
#[derive(Debug, Clone)]
struct Line {
    reel: String,
    /// `V` or `A`.
    kind: char,
    source_in: RationalTime,
    source_out: RationalTime,
    record_in: RationalTime,
    record_out: RationalTime,
    /// How long the transition on this line runs. Zero means a cut.
    dissolve: RationalTime,
}

impl Line {
    fn new(kind: char, rate: f64, reel: String) -> Self {
        let zero = RationalTime::new(0.0, rate);
        Self {
            reel,
            kind,
            source_in: zero,
            source_out: zero,
            record_in: zero,
            record_out: zero,
            dissolve: zero,
        }
    }

    /// Renders the line, in the columns every EDL reader expects.
    fn render(&self, edit_number: usize, rate: f64) -> Result<String> {
        let timecode =
            |time: RationalTime| -> Result<String> { Ok(time.to_timecode_at(rate, DropFrame::InferFromRate)?) };
        let times = format!(
            "{} {} {} {}",
            timecode(self.source_in)?,
            timecode(self.source_out)?,
            timecode(self.record_in)?,
            timecode(self.record_out)?
        );

        if self.dissolve.value() > 0.0 {
            let frames = self.dissolve.to_frames_at_rate(rate);
            Ok(format!(
                "{edit_number:03}  {:8} {:5} D {frames:03}    {times}",
                self.reel, self.kind
            ))
        } else {
            Ok(format!(
                "{edit_number:03}  {:8} {:5} C        {times}",
                self.reel, self.kind
            ))
        }
    }
}

/// One event, ready to render.
#[derive(Debug, Clone)]
struct Event {
    /// A dissolve is two lines: a zero-length cut, then the dissolve itself.
    cut: Option<Line>,
    line: Line,
    comments: Vec<String>,
    /// What a following dissolve borrows from this event.
    clip: NodeId,
}

/// Writes one track.
struct Writer<'a> {
    document: &'a mut Document,
    stack: NodeId,
    rate: f64,
    style: Style,
    reelname_len: Option<usize>,
}

impl Writer<'_> {
    /// Renders one track as the body of an EDL.
    fn content_for_track(&mut self, track: NodeId, title: &str) -> Result<String> {
        self.close_trailing_transition(track)?;
        self.restate_transitions(track)?;

        let events = self.events_for(track)?;

        let mut content = String::new();
        if !title.is_empty() {
            let _ = write!(content, "TITLE: {title}\n\n");
        }

        let enabled = self
            .document
            .try_get(track)?
            .item()
            .is_some_and(|item| item.enabled);
        if !enabled {
            return Ok(content);
        }

        for (index, event) in events.iter().enumerate() {
            let edit_number = index + 1;
            let mut lines = Vec::new();
            if let Some(cut) = &event.cut {
                lines.push(cut.render(edit_number, self.rate)?);
            }
            lines.push(event.line.render(edit_number, self.rate)?);
            lines.extend(event.comments.iter().cloned());
            let _ = writeln!(content, "{}", lines.join("\n"));
        }

        Ok(content)
    }

    /// Gives a track ending in a transition something to dissolve into.
    ///
    /// A transition needs a clip on each side. One at the very end of a track
    /// is a fade out, so the thing it dissolves to is nothing: a zero-length
    /// gap, which writes as black.
    fn close_trailing_transition(&mut self, track: NodeId) -> Result<()> {
        let children = self.document.children_of(track)?;
        let Some(&last) = children.last() else {
            return Ok(());
        };
        if !matches!(self.document.try_get(last)?, Node::Transition(_)) {
            return Ok(());
        }

        let duration = self.document.duration(last)?;
        let gap = self.document.insert(Node::Gap(Gap {
            item: ItemData {
                source_range: Some(TimeRange::new(duration, RationalTime::new(0.0, self.rate))),
                ..ItemData::new()
            },
        }));
        self.document.append_child(track, gap)?;
        Ok(())
    }

    /// Restates the track's cut points the way an EDL states them.
    ///
    /// See this module's header for the picture. Each transition is moved
    /// entirely onto the incoming side: the outgoing clip is shortened by the
    /// transition's reach back, the incoming one is lengthened by it, and the
    /// transition itself is left reaching only forward.
    fn restate_transitions(&mut self, track: NodeId) -> Result<()> {
        let children = self.document.children_of(track)?;

        for (index, child) in children.iter().copied().enumerate() {
            let Node::Transition(transition) = self.document.try_get(child)? else {
                continue;
            };
            let in_offset = transition.in_offset;

            if index > 0 {
                let previous = children[index - 1];
                self.extend(previous, -in_offset)?;
            }

            let Some(&next) = children.get(index + 1) else {
                return Err(Error::unsupported(
                    "a transition needs an item after it to dissolve into",
                ));
            };
            let range = self.source_range_of(next)?;
            self.document
                .try_get_mut(next)?
                .item_mut()
                .ok_or_else(|| Error::unsupported("a transition can only join items"))?
                .source_range = Some(TimeRange::new(
                range.start_time() - in_offset,
                range.duration() + in_offset,
            ));

            let Node::Transition(transition) = self.document.try_get_mut(child)? else {
                unreachable!("matched a transition just above");
            };
            transition.out_offset += in_offset;
            transition.in_offset = RationalTime::new(0.0, self.rate);
        }

        Ok(())
    }

    /// Returns an item's own range, which an EDL event cannot do without.
    fn source_range_of(&self, id: NodeId) -> Result<TimeRange> {
        self.document
            .try_get(id)?
            .item()
            .and_then(|item| item.source_range)
            .ok_or_else(|| {
                Error::unsupported(
                    "an EDL states every event's source timecode, so each item needs a source range"
                        .to_string(),
                )
            })
    }

    /// Lengthens an item's own range by `duration`, which may be negative.
    fn extend(&mut self, id: NodeId, duration: RationalTime) -> Result<()> {
        let range = self.source_range_of(id)?;
        self.document
            .try_get_mut(id)?
            .item_mut()
            .expect("checked just above")
            .source_range = Some(range.duration_extended_by(duration));
        Ok(())
    }

    /// Groups the track's children into events.
    ///
    /// A clip on its own is a cut. A clip after a transition is a dissolve,
    /// which takes two lines and borrows the outgoing side from the event
    /// before it. A gap is nothing at all — an EDL says "no event here" by
    /// leaving a hole in the record timecode.
    fn events_for(&mut self, track: NodeId) -> Result<Vec<Event>> {
        let children = self.document.children_of(track)?;
        let kind = match self.document.try_get(track)? {
            Node::Track(track) if track.kind == "Audio" => 'A',
            _ => 'V',
        };

        let mut events: Vec<Event> = Vec::new();
        for (index, child) in children.iter().copied().enumerate() {
            if matches!(self.document.try_get(child)?, Node::Transition(_)) {
                continue;
            }

            let previous = index.checked_sub(1).map(|at| children[at]);
            let after_transition = match previous {
                Some(previous) => matches!(self.document.try_get(previous)?, Node::Transition(_)),
                None => false,
            };

            if after_transition {
                let transition = previous.expect("checked just above");
                let event = self.dissolve_event(events.last(), transition, child, kind)?;
                events.push(event);
            } else if matches!(self.document.try_get(child)?, Node::Clip(_)) {
                let enabled = self
                    .document
                    .try_get(child)?
                    .item()
                    .is_some_and(|item| item.enabled);
                if enabled {
                    let event = self.cut_event(child, kind)?;
                    events.push(event);
                }
            }
        }

        Ok(events)
    }

    /// Builds a plain cut.
    fn cut_event(&mut self, clip: NodeId, kind: char) -> Result<Event> {
        let reel = if self.style == Style::Premiere {
            AUXILIARY_REEL.to_string()
        } else {
            self.reel_for(clip)?
        };
        let mut line = Line::new(kind, self.rate, reel);

        let source_range = self.source_range_of(clip)?;
        line.source_in = source_range.start_time();
        line.source_out = source_range.end_time_exclusive();

        // A speed change or a freeze frame means the event uses a different
        // amount of source than it occupies on the timeline, and it is the
        // source end that moves.
        match self.timing_effect(clip)? {
            Some(Timing::FreezeFrame) => {
                line.source_out = line.source_in + RationalTime::new(1.0, line.source_in.rate());
            }
            Some(Timing::Warp(time_scalar)) => {
                let used = self.document.trimmed_range(clip)?.duration().value() / time_scalar;
                line.source_out = line.source_in + RationalTime::new(used, self.rate);
            }
            None => {}
        }

        let in_timeline = self.document.transformed_time_range(
            self.document.trimmed_range(clip)?,
            clip,
            self.stack,
        )?;
        line.record_in = in_timeline.start_time();
        line.record_out = in_timeline.end_time_exclusive();

        let comments = self.comments_for(clip, Direction::From)?;
        Ok(Event {
            cut: None,
            line,
            comments,
            clip,
        })
    }

    /// Builds a dissolve, which is a zero-length cut followed by the dissolve.
    fn dissolve_event(
        &mut self,
        a_side: Option<&Event>,
        transition: NodeId,
        b_side: NodeId,
        kind: char,
    ) -> Result<Event> {
        let mut cut = Line::new(kind, self.rate, BLACK_REEL.to_string());
        let mut from_comments = Vec::new();

        // With nothing before it, the dissolve is a fade up from black, and
        // the cut line says so by naming the black reel at zero.
        if let Some(a_side) = a_side {
            cut.reel = a_side.line.reel.clone();
            cut.source_in = a_side.line.source_out;
            cut.source_out = a_side.line.source_out;
            cut.record_in = a_side.line.record_out;
            cut.record_out = a_side.line.record_out;
            from_comments = self.comments_for(a_side.clip, Direction::From)?;
        }

        let mut line = Line::new(kind, self.rate, self.reel_for(b_side)?);
        let source_range = self.source_range_of(b_side)?;
        line.source_in = source_range.start_time();
        line.source_out = source_range.end_time_exclusive();

        let in_timeline = self.document.transformed_time_range(
            self.document.trimmed_range(b_side)?,
            b_side,
            self.stack,
        )?;
        line.record_in = in_timeline.start_time();
        line.record_out = in_timeline.end_time_exclusive();

        let Node::Transition(transition) = self.document.try_get(transition)? else {
            unreachable!("only called with a transition");
        };
        line.dissolve = transition.out_offset;

        let mut comments = from_comments;
        comments.extend(self.comments_for(b_side, Direction::To)?);

        Ok(Event {
            cut: Some(cut),
            line,
            comments,
            clip: b_side,
        })
    }

    /// Returns the reel name to write for an item.
    fn reel_for(&self, id: NodeId) -> Result<String> {
        let node = self.document.try_get(id)?;
        if matches!(node, Node::Gap(_)) {
            return Ok(BLACK_REEL.to_string());
        }

        if let Some(reel) = self.metadata_string(id, "reel") {
            // A reel the document already carries is written as it stands: it
            // came from a file that had it, and shortening it again would
            // lose what the round trip is for.
            return Ok(reel);
        }

        let mut reel = match node.name() {
            "" => AUXILIARY_REEL.to_string(),
            name => name.to_string(),
        };
        if let Some(reference) = self.media_reference(id)? {
            match self.document.try_get(reference)? {
                Node::ExternalReference(external) => {
                    reel = if external.media.base.name.is_empty() {
                        basename(&external.target_url).to_string()
                    } else {
                        external.media.base.name.clone()
                    };
                }
                Node::ImageSequenceReference(sequence) => {
                    reel = if sequence.media.base.name.is_empty() {
                        image_sequence::url_for_range(sequence, self.document.trimmed_range(id)?)
                    } else {
                        sequence.media.base.name.clone()
                    };
                }
                _ => {}
            }
        }

        let flipped = flip_windows_slashes(&reel);
        let reel = strip_alphabetic_extension(basename(&flipped));

        let Some(length) = self.reelname_len else {
            return Ok(reel.to_string());
        };

        // Most systems accept letters, digits and spaces in a reel and
        // nothing else, so drop the rest rather than write a file they
        // refuse.
        let mut reel: String = reel
            .chars()
            .filter(|character| {
                *character == ' ' || character.is_ascii_alphanumeric()
            })
            .collect();
        reel.truncate(length);
        while reel.chars().count() < length {
            reel.push(' ');
        }
        Ok(reel)
    }

    /// Returns the comment lines describing one item.
    ///
    /// `Gap` has nothing to say about itself, so a fade to black carries only
    /// the comments of the clip it fades from.
    fn comments_for(&mut self, clip: NodeId, direction: Direction) -> Result<Vec<String>> {
        if matches!(self.document.try_get(clip)?, Node::Gap(_)) {
            return Ok(Vec::new());
        }

        let mut lines = Vec::new();
        let timing = self.timing_effect(clip)?;
        let suffix = if matches!(timing, Some(Timing::FreezeFrame)) {
            " FF"
        } else {
            ""
        };

        let url = self.media_url(clip)?;

        // Premiere reads any `FROM` comment as meaning the clip has no name,
        // so the name has to come from the path instead.
        if self.style == Style::Premiere {
            if let Some(url) = &url {
                let name = basename(url).to_string();
                self.document
                    .try_get_mut(clip)?
                    .item_mut()
                    .expect("a clip is an item")
                    .base
                    .name = name;
            }
        }

        let name = self.document.try_get(clip)?.name().to_string();

        if let Some(Timing::Warp(time_scalar)) = timing {
            let start = self.document.trimmed_range(clip)?.start_time();
            let _ = lines.push(format!(
                "M2   {name}\t\t{}\t\t\t{}",
                float(time_scalar * self.rate),
                start.to_timecode_at(self.rate, DropFrame::InferFromRate)?
            ));
        }

        if !name.is_empty() {
            // Avid writes two spaces after the colon here, so match it: files
            // are diffed against Avid's own output more often than not.
            lines.push(format!("* {direction} CLIP NAME:  {name}{suffix}"));
        }

        if matches!(timing, Some(Timing::FreezeFrame)) {
            lines.push("* * FREEZE FRAME".to_string());
        }

        if let Some(url) = &url {
            let url = flip_windows_slashes(url);
            match self.style.media_comment() {
                Some(word) => lines.push(format!("* {direction} {word}: {url}")),
                None => lines.push(format!("* OTIO REFERENCE {direction}: {url}")),
            }
        }

        if self.reelname_len.is_some() && self.metadata_string(clip, "reel").is_none() {
            // The reel was shortened to fit, so record what it was shortened
            // from: that is what makes the round trip lossless.
            let source = url.clone().unwrap_or_else(|| name.clone());
            lines.push(format!(
                "* OTIO TRUNCATED REEL NAME FROM: {}",
                basename(&flip_windows_slashes(&source))
            ));
        }

        if self.style == Style::Premiere {
            self.set_metadata_string(clip, "reel", AUXILIARY_REEL)?;
        }

        lines.extend(self.cdl_comments(clip));
        lines.extend(self.marker_comments(clip)?);

        // Anything read from a file that this adapter did not understand goes
        // back out untouched.
        for comment in self.metadata_strings(clip, "comments") {
            lines.push(format!("* {comment}"));
        }

        Ok(lines)
    }

    /// Returns the colour-decision comments for a clip, if it carries any.
    fn cdl_comments(&self, clip: NodeId) -> Vec<String> {
        let Some(metadata) = self.metadata(clip) else {
            return Vec::new();
        };
        let Some(values) = metadata.get("cdl").and_then(Any::as_dictionary) else {
            return Vec::new();
        };
        let cdl = Cdl::from_metadata(values);

        let mut lines = Vec::new();
        if let Some(sop) = cdl.sop {
            let triple = |values: [f64; 3]| {
                format!("({} {} {})", float(values[0]), float(values[1]), float(values[2]))
            };
            lines.push(format!(
                "*ASC_SOP {} {} {}",
                triple(sop.slope),
                triple(sop.offset),
                triple(sop.power)
            ));
        }
        if let Some(sat) = cdl.sat {
            lines.push(format!("*ASC_SAT {}", float(sat)));
        }
        lines
    }

    /// Returns the `LOC` comments for a clip's markers.
    fn marker_comments(&self, clip: NodeId) -> Result<Vec<String>> {
        let Some(item) = self.document.try_get(clip)?.item() else {
            return Ok(Vec::new());
        };

        let mut lines = Vec::new();
        for marker in item.markers.clone() {
            let Node::Marker(marker) = self.document.try_get(marker)? else {
                continue;
            };
            let timecode = marker
                .marked_range
                .start_time()
                .to_timecode_at(self.rate, DropFrame::InferFromRate)?;
            let color = marker
                .color
                .as_ref()
                .map(|color| color.name.to_uppercase())
                .unwrap_or_default();
            let comment = marker.base.name.to_uppercase();
            lines.push(format!("* LOC: {timecode} {color:7} {comment}"));
        }
        Ok(lines)
    }

    /// Returns where a clip's media is, if anything names it.
    fn media_url(&self, clip: NodeId) -> Result<Option<String>> {
        let Some(reference) = self.media_reference(clip)? else {
            return Ok(None);
        };
        Ok(match self.document.try_get(reference)? {
            Node::ExternalReference(external) => Some(external.target_url.clone()),
            Node::ImageSequenceReference(sequence) => Some(image_sequence::url_for_range(
                sequence,
                self.document.trimmed_range(clip)?,
            )),
            // Generated and missing media have no path to write.
            _ => None,
        })
    }

    /// Returns a clip's active media reference.
    fn media_reference(&self, clip: NodeId) -> Result<Option<NodeId>> {
        let Node::Clip(clip) = self.document.try_get(clip)? else {
            return Ok(None);
        };
        Ok(clip
            .media_references
            .get(&clip.active_media_reference_key)
            .copied())
    }

    /// Returns the timing effect on a clip, refusing what an EDL cannot say.
    fn timing_effect(&self, clip: NodeId) -> Result<Option<Timing>> {
        let Some(item) = self.document.try_get(clip)?.item() else {
            return Ok(None);
        };

        let mut timings = Vec::new();
        for effect in &item.effects {
            match self.document.try_get(*effect)? {
                Node::FreezeFrame { .. } => timings.push(Timing::FreezeFrame),
                Node::LinearTimeWarp { time_scalar, .. } => timings.push(Timing::Warp(*time_scalar)),
                // A plain effect says nothing about timing, so it passes
                // through; a TimeEffect says something an EDL cannot.
                Node::TimeEffect(_) => {
                    return Err(Error::unsupported(format!(
                        "clip '{}' has a timing effect an EDL cannot express",
                        self.document.try_get(clip)?.name()
                    )));
                }
                _ => {}
            }
        }

        if timings.len() > 1 {
            return Err(Error::unsupported(
                "an EDL allows one timing effect per clip",
            ));
        }
        Ok(timings.into_iter().next())
    }

    /// Returns an item's own metadata, if it has any.
    fn metadata(&self, id: NodeId) -> Option<&AnyDictionary> {
        Some(&self.document.get(id)?.base()?.metadata)
    }

    /// Returns a string from an item's `cmx_3600` metadata.
    fn metadata_string(&self, id: NodeId, key: &str) -> Option<String> {
        let value = self
            .metadata(id)?
            .get("cmx_3600")?
            .as_dictionary()?
            .get(key)?
            .as_str()?;
        (!value.is_empty()).then(|| value.to_string())
    }

    /// Returns a list of strings from an item's `cmx_3600` metadata.
    fn metadata_strings(&self, id: NodeId, key: &str) -> Vec<String> {
        self.metadata(id)
            .and_then(|metadata| metadata.get("cmx_3600")?.as_dictionary()?.get(key)?.as_slice())
            .unwrap_or_default()
            .iter()
            .filter_map(|value| Some(value.as_str()?.to_string()))
            .collect()
    }

    /// Sets a string in an item's `cmx_3600` metadata.
    fn set_metadata_string(&mut self, id: NodeId, key: &str, value: &str) -> Result<()> {
        // Only ever called on a clip, which is an item; anything else has
        // nothing an EDL would record here.
        let Some(item) = self.document.try_get_mut(id)?.item_mut() else {
            return Ok(());
        };
        let entry = item
            .base
            .metadata
            .entry("cmx_3600".to_string())
            .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
        if let Any::Dictionary(cmx) = entry {
            cmx.insert(key.to_string(), Any::String(value.to_string()));
        }
        Ok(())
    }
}

/// The timing effect an event carries.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Timing {
    /// The event holds one frame for its whole length.
    FreezeFrame,
    /// The event plays at a multiple of normal speed.
    Warp(f64),
}

/// Which side of a dissolve a comment describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    From,
    To,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::From => "FROM",
            Self::To => "TO",
        })
    }
}

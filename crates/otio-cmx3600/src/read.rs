//! Reading an EDL into a timeline.

use std::collections::BTreeMap;

use opentime::{RationalTime, TimeRange};
use otio_adapter::cdl::{Cdl, Sop};
use otio_adapter::{Error, Result};
use otio_core::schema::{
    Base, Clip, EffectData, ExternalReference, GeneratorReference, ImageSequenceReference,
    ItemData, Marker, MediaReferenceData, MissingFramePolicy, Node, Stack, Timeline, Track,
    Transition,
};
use otio_core::upgrade::{DEFAULT_MEDIA_KEY, color_from_legacy_name};
use otio_core::{Any, AnyDictionary, Document, NodeId};

use crate::ReadOptions;
use crate::comment::Comments;
use crate::image_sequence;
use crate::path::{basename, strip_extension};
use crate::statement::{Statement, leading_number};

/// The reel name systems use for a source they are not naming.
const AUXILIARY_REEL: &str = "AX";

/// The channel shorthands the format allows, and the tracks they mean.
///
/// A channel not listed here names a track directly, so `A3` is the track
/// `A3`.
const CHANNEL_MAP: [(&str, &[&str]); 5] = [
    ("A", &["A1"]),
    ("AA", &["A1", "A2"]),
    ("B", &["V", "A1"]),
    ("A2/V", &["V", "A2"]),
    ("AA/V", &["V", "A1", "A2"]),
];

/// The marker colours the format's `LOC` comments may name.
///
/// Anything else is read as red, which is what upstream does: a marker in an
/// unknown colour is still a marker, and the name it was written with is kept
/// in metadata either way.
const MARKER_COLORS: [&str; 11] = [
    "PINK", "RED", "ORANGE", "YELLOW", "GREEN", "CYAN", "BLUE", "PURPLE", "MAGENTA", "BLACK",
    "WHITE",
];

/// Reads an EDL.
pub fn read(input: &str, options: &ReadOptions) -> Result<Document> {
    Parser::new(options).parse(input)
}

/// The state of a parse in progress.
struct Parser<'a> {
    document: Document,
    timeline: NodeId,
    stack: NodeId,
    /// Each track by the name the EDL calls it, such as `V` or `A2`.
    tracks_by_name: BTreeMap<String, NodeId>,
    options: &'a ReadOptions,
}

impl<'a> Parser<'a> {
    fn new(options: &'a ReadOptions) -> Self {
        let mut document = Document::new();
        let stack = document.insert(Node::Stack(Stack::default()));
        let timeline = document.insert(Node::Timeline(Timeline {
            base: Base::default(),
            tracks: Some(stack),
            global_start_time: None,
        }));
        document.set_root(Some(timeline));

        Self {
            document,
            timeline,
            stack,
            tracks_by_name: BTreeMap::new(),
            options,
        }
    }

    /// Reads the whole file.
    ///
    /// An event is an indeterminate number of lines: one edit statement, then
    /// optionally a second statement with the same event number — which is
    /// how the format spells a transition — then any number of comments.
    /// Nothing marks the end of an event but the start of the next one.
    fn parse(mut self, input: &str) -> Result<Document> {
        // Blank lines carry nothing here, so drop them up front and keep each
        // line's number for error messages.
        let lines: Vec<(usize, &str)> = input
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line.trim()))
            .filter(|(_, line)| !line.is_empty())
            .collect();

        let mut at = 0;
        while at < lines.len() {
            let (number, line) = lines[at];
            at += 1;

            if let Some(title) = line.strip_prefix("TITLE:") {
                self.set_title(title.trim())?;
            } else if line.starts_with("FCM") {
                // Drop-frame or not, for tape timecode. Nothing downstream
                // uses it.
            } else if line.starts_with("SPLIT") {
                self.parse_split(&lines, &mut at, line, number)?;
            } else if line.starts_with(|character: char| character.is_ascii_digit()) {
                self.parse_event(&lines, &mut at, line, number)?;
            } else {
                return Err(Error::parse_at(
                    number,
                    format!("unknown event type: {line}"),
                ));
            }
        }

        self.settle_track_ranges()?;
        Ok(self.document)
    }

    fn set_title(&mut self, title: &str) -> Result<()> {
        if let Node::Timeline(timeline) = self.document.try_get_mut(self.timeline)? {
            timeline.base.name = title.to_string();
        }
        Ok(())
    }

    /// Reads a `SPLIT`, where one event lands on picture and sound at
    /// different times.
    ///
    /// The two statements that follow share one set of comments. The delay
    /// itself is not modelled: the statements already carry the times it
    /// implies.
    fn parse_split(
        &mut self,
        lines: &[(usize, &str)],
        at: &mut usize,
        line: &str,
        number: usize,
    ) -> Result<()> {
        let audio_delay = line.contains("AUDIO DELAY");
        let video_delay = line.contains("VIDEO DELAY");
        if audio_delay && video_delay {
            return Err(Error::parse_at(
                number,
                "both audio and video delay declared after SPLIT",
            ));
        }
        if !audio_delay && !video_delay {
            return Err(Error::parse_at(
                number,
                "either audio or video delay declared after SPLIT",
            ));
        }

        let Some(&(first_number, first)) = lines.get(*at) else {
            return Err(Error::parse_at(number, "SPLIT with no statement after it"));
        };
        let Some(&(second_number, second)) = lines.get(*at + 1) else {
            return Err(Error::parse_at(
                number,
                "SPLIT with only one statement after it",
            ));
        };
        *at += 2;

        let mut comments = Vec::new();
        while let Some(&(_, next)) = lines.get(*at) {
            if next.starts_with(|character: char| character.is_ascii_digit()) {
                break;
            }
            comments.push(next);
            *at += 1;
        }

        self.add_event(first, &comments, None, first_number)?;
        self.add_event(second, &comments, None, second_number)
    }

    /// Reads one ordinary event, and the transition and comments after it.
    fn parse_event(
        &mut self,
        lines: &[(usize, &str)],
        at: &mut usize,
        line: &str,
        number: usize,
    ) -> Result<()> {
        let event_id = leading_number(line)
            .ok_or_else(|| Error::parse_at(number, format!("event id is not a number: {line}")))?;

        let mut comments = Vec::new();
        let mut transition_line = None;

        while let Some(&(next_number, next)) = lines.get(*at) {
            let Some(next_id) = leading_number(next) else {
                comments.push(next);
                *at += 1;
                continue;
            };
            if next_id != event_id {
                break;
            }
            // The same event number twice means a transition between this
            // event and the one before it.
            if transition_line.is_some() {
                return Err(Error::parse_at(
                    next_number,
                    format!("invalid transition {next}"),
                ));
            }
            transition_line = Some(next);
            *at += 1;
        }

        self.add_event(line, &comments, transition_line, number)
    }

    /// Turns one event into a clip, and a transition if it has one.
    fn add_event(
        &mut self,
        line: &str,
        comments: &[&str],
        transition_line: Option<&str>,
        number: usize,
    ) -> Result<()> {
        self.add_event_inner(line, comments, transition_line)
            .map_err(|error| error.at_line(number))
    }

    fn add_event_inner(
        &mut self,
        line: &str,
        comments: &[&str],
        transition_line: Option<&str>,
    ) -> Result<()> {
        let rate = self.options.rate;
        let comments = Comments::parse(comments);

        let cut = Statement::parse(line, rate)?;
        // A transition statement overrides the cut's reel and timecodes: the
        // cut line before a dissolve is a zero-length placeholder, and the
        // clip the event really describes is the one on the far side.
        let statement = match transition_line {
            Some(line) => Statement::parse(line, rate)?,
            None => cut.clone(),
        };

        let clip = self.make_clip(&cut, &statement, &comments, rate)?;
        let transition = match transition_line {
            Some(_) => Some(self.make_transition(&cut, &statement, &comments, clip)?),
            None => None,
        };

        self.annotate(clip, &statement, &comments)?;

        let record_in = RationalTime::from_timecode(&statement.record_in, rate)?;
        let record_out = RationalTime::from_timecode(&statement.record_out, rate)?;
        let (record_in, record_out) =
            self.reconcile_durations(clip, record_in, record_out, &comments, rate)?;

        self.place(
            clip,
            transition,
            &statement.channel,
            record_in,
            record_out,
            rate,
        )
    }

    /// Builds the clip an event describes.
    fn make_clip(
        &mut self,
        cut: &Statement,
        statement: &Statement,
        comments: &Comments,
        rate: f64,
    ) -> Result<NodeId> {
        let source_range = TimeRange::range_from_start_end_time(
            RationalTime::from_timecode(&statement.source_in, rate)?,
            RationalTime::from_timecode(&statement.source_out, rate)?,
        );

        let (reference, url) =
            self.make_media_reference(statement, comments, source_range, rate)?;

        // The event number is the fallback name, for a file that says nothing
        // about what the clip is called.
        let mut name = cut.event_id.clone();
        if let Some(clip_name) = &comments.clip_name {
            name = clip_name.clone();
        } else if let Some(url) = &url {
            name = strip_extension(basename(url)).to_string();
        }
        // `TO CLIP NAME` wins over `FROM CLIP NAME`, because on a dissolve the
        // clip this event carries is the one being dissolved to.
        if let Some(dest) = &comments.dest_clip_name {
            name = dest.clone();
        }

        let mut metadata = AnyDictionary::new();
        let cdl = read_cdl(comments)?;
        if !cdl.is_empty() {
            metadata.insert("cdl".to_string(), Any::Dictionary(cdl.to_metadata()));
        }

        let markers = self.make_markers(comments, rate)?;

        let mut media_references = BTreeMap::new();
        let mut active_media_reference_key = String::new();
        if let Some(reference) = reference {
            media_references.insert(DEFAULT_MEDIA_KEY.to_string(), reference);
            active_media_reference_key = DEFAULT_MEDIA_KEY.to_string();
        }

        Ok(self.document.insert(Node::Clip(Clip {
            item: ItemData {
                base: Base { name, metadata },
                source_range: Some(source_range),
                markers,
                ..ItemData::new()
            },
            media_references,
            active_media_reference_key,
        })))
    }

    /// Builds the clip's media reference, and returns the URL it names.
    ///
    /// `BL`, `BLACK` and `BARS` are the format's special sources: they are
    /// generated rather than stored, so nothing on disk corresponds to them.
    fn make_media_reference(
        &mut self,
        statement: &Statement,
        comments: &Comments,
        source_range: TimeRange,
        rate: f64,
    ) -> Result<(Option<NodeId>, Option<String>)> {
        let generator = match statement.reel.as_str() {
            "BL" | "BLACK" => Some("black"),
            "BARS" => Some("SMPTEBars"),
            _ => None,
        };
        if let Some(kind) = generator {
            let id = self
                .document
                .insert(Node::GeneratorReference(GeneratorReference {
                    media: MediaReferenceData::default(),
                    generator_kind: kind.to_string(),
                    parameters: AnyDictionary::new(),
                }));
            return Ok((Some(id), None));
        }

        let Some(url) = &comments.media_reference else {
            // Nothing said where the media is. The event is still real, so
            // the clip keeps a reference that says exactly that.
            let id = self.document.insert(Node::MissingReference(
                otio_core::schema::MissingReference {
                    media: MediaReferenceData::default(),
                },
            ));
            return Ok((Some(id), None));
        };

        if let Some(sequence) = image_sequence::parse_url(url) {
            let id = self
                .document
                .insert(Node::ImageSequenceReference(ImageSequenceReference {
                    media: MediaReferenceData {
                        available_range: Some(source_range),
                        ..MediaReferenceData::default()
                    },
                    target_url_base: sequence.directory,
                    name_prefix: sequence.prefix,
                    name_suffix: sequence.suffix,
                    start_frame: sequence.start,
                    frame_step: 1,
                    rate,
                    frame_zero_padding: sequence.padding,
                    missing_frame_policy: MissingFramePolicy::Error,
                }));
            // The name a sequence gives a clip comes from the URL that
            // stands for the whole range, not from any single frame.
            return Ok((Some(id), Some(url.clone())));
        }

        let id = self
            .document
            .insert(Node::ExternalReference(ExternalReference {
                media: MediaReferenceData::default(),
                target_url: url.clone(),
            }));
        Ok((Some(id), Some(url.clone())))
    }

    /// Builds the markers an event's `LOC` comments describe.
    fn make_markers(&mut self, comments: &Comments, rate: f64) -> Result<Vec<NodeId>> {
        let mut markers = Vec::new();

        for locator in &comments.locators {
            // `01:00:01:14 RED     ANIM FIX NEEDED`. These are nominally
            // fixed-width, but there are too many dialects to insist on it,
            // so a locator that does not read is skipped rather than fatal.
            let Some((timecode, rest)) = locator.split_once(char::is_whitespace) else {
                continue;
            };
            let Ok(start) = RationalTime::from_timecode(timecode, rate) else {
                continue;
            };
            let rest = rest.trim_start();
            let split = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (color_name, comment) = rest.split_at(split);
            if !color_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }

            let upper = color_name.to_uppercase();
            let color = if MARKER_COLORS.contains(&upper.as_str()) {
                color_from_legacy_name(&upper)
            } else {
                color_from_legacy_name("RED")
            };

            // The name as written is kept whether or not it was a colour the
            // format knows, so nothing the file said is lost.
            let mut cmx = AnyDictionary::new();
            cmx.insert("color".to_string(), Any::String(color_name.to_string()));
            let mut metadata = AnyDictionary::new();
            metadata.insert("cmx_3600".to_string(), Any::Dictionary(cmx));

            markers.push(self.document.insert(Node::Marker(Marker {
                base: Base {
                    name: comment.trim().to_string(),
                    metadata,
                },
                color: Some(color),
                marked_range: TimeRange::new(start, RationalTime::new(0.0, rate)),
                comment: String::new(),
            })));
        }

        Ok(markers)
    }

    /// Builds the transition an event describes.
    fn make_transition(
        &mut self,
        cut: &Statement,
        statement: &Statement,
        comments: &Comments,
        clip: NodeId,
    ) -> Result<NodeId> {
        // Upstream reads a transition's id off a nine-field statement and a
        // cut's number off an eight-field one, then compares the two. A second
        // line that is itself a cut never sets the transition's id, so what
        // upstream reports is the mismatch rather than anything about the edit
        // type. The same thing said directly: the second line of an event has
        // to be a transition.
        let Some(transition_data) = statement.transition_data.as_deref() else {
            return Err(Error::parse(format!(
                "transition and event id mismatch: none vs {}",
                cut.event_id
            )));
        };

        if statement.event_id != cut.event_id {
            return Err(Error::parse(format!(
                "transition and event id mismatch: {} vs {}",
                statement.event_id, cut.event_id
            )));
        }

        let transition_type = if statement.edit_type == "D" {
            "SMPTE_Dissolve"
        } else if is_wipe(&statement.edit_type) {
            "SMPTE_Wipe"
        } else {
            return Err(Error::unsupported(format!(
                "transition type '{}' is not supported by the CMX EDL reader",
                statement.edit_type
            )));
        };

        let frames: f64 = transition_data.parse().map_err(|_| {
            Error::parse(format!(
                "transition length is not a number: {transition_data}"
            ))
        })?;
        let duration =
            RationalTime::new(frames, self.document.trimmed_range(clip)?.duration().rate());

        // A transition is written unconventionally in an EDL. Where it would
        // normally be drawn straddling the cut:
        //
        //            |---57.0 Trans 43.0----|
        // |------Clip1 102.0------|----------Clip2 143.0----------|
        //
        // an EDL states it as beginning where the outgoing clip ends:
        //
        //            |---0.0 Trans 100.0----|
        // |Clip1 45.0|----------------Clip2 200.0-----------------|
        //
        // so the whole of it reaches forward and none of it reaches back.
        let mut name = format!(
            "{transition_type} to {}",
            self.document.try_get(clip)?.name()
        );
        if let (Some(from), Some(to)) = (&comments.clip_name, &comments.dest_clip_name) {
            name = format!("{transition_type} from {from} to {to}");
        }

        let mut cmx = AnyDictionary::new();
        cmx.insert(
            "transition".to_string(),
            Any::String(statement.edit_type.clone()),
        );
        cmx.insert(
            "transition_duration".to_string(),
            Any::Double(duration.value()),
        );
        let mut metadata = AnyDictionary::new();
        metadata.insert("cmx_3600".to_string(), Any::Dictionary(cmx));

        Ok(self.document.insert(Node::Transition(Transition {
            base: Base { name, metadata },
            parent: None,
            in_offset: RationalTime::new(0.0, duration.rate()),
            out_offset: duration,
            transition_type: transition_type.to_string(),
            enabled: true,
        })))
    }

    /// Records on the clip what the event said that has no field of its own.
    fn annotate(&mut self, clip: NodeId, statement: &Statement, comments: &Comments) -> Result<()> {
        let mut cmx = AnyDictionary::new();

        if !comments.unhandled.is_empty() {
            cmx.insert(
                "comments".to_string(),
                Any::Vector(
                    comments
                        .unhandled
                        .iter()
                        .map(|comment| Any::String(comment.clone()))
                        .collect(),
                ),
            );
        }

        // `AX` means "some source we are not naming", so recording it would
        // put a value in the document that says nothing.
        if !statement.reel.is_empty() && statement.reel != AUXILIARY_REEL {
            cmx.insert("reel".to_string(), Any::String(statement.reel.clone()));
        }

        if cmx.is_empty() {
            return Ok(());
        }

        let node = self.document.try_get_mut(clip)?;
        let metadata = &mut node.item_mut().expect("a clip is an item").base.metadata;
        metadata.insert("cmx_3600".to_string(), Any::Dictionary(cmx));
        Ok(())
    }

    /// Settles what to do when an event's source and record spans disagree.
    ///
    /// They should be the same length. When they are not, the event is either
    /// a speed change or a freeze frame — both of which make a different
    /// number of source frames fill the record span — or the file is simply
    /// wrong.
    fn reconcile_durations(
        &mut self,
        clip: NodeId,
        record_in: RationalTime,
        record_out: RationalTime,
        comments: &Comments,
        rate: f64,
    ) -> Result<(RationalTime, RationalTime)> {
        let source_duration = self.document.duration(clip)?;
        let record_duration = record_out - record_in;
        if record_duration == source_duration {
            return Ok((record_in, record_out));
        }

        if comments.motion_effect.is_some() || comments.freeze_frame.is_some() {
            // The clip occupies the record span; the effect accounts for the
            // difference in how much source that takes.
            let source_range = self.document.trimmed_range(clip)?;
            let item = self
                .document
                .try_get_mut(clip)?
                .item_mut()
                .expect("a clip is an item");
            item.source_range = Some(TimeRange::new(source_range.start_time(), record_duration));

            let effect = if comments.freeze_frame.is_some() {
                let name = self.document.try_get(clip)?.name().to_string();
                // The writer adds the suffix back, so carrying it in the name
                // would double it on a round trip.
                if let Some(trimmed) = name.strip_suffix(" FF") {
                    let trimmed = trimmed.to_string();
                    self.document
                        .try_get_mut(clip)?
                        .item_mut()
                        .expect("a clip is an item")
                        .base
                        .name = trimmed;
                }
                Node::FreezeFrame {
                    effect: EffectData {
                        effect_name: "FreezeFrame".to_string(),
                        ..EffectData::new()
                    },
                    time_scalar: 0.0,
                }
            } else {
                let motion = comments
                    .motion_effect
                    .as_deref()
                    .expect("checked just above");
                let fps = speed_of(motion)?;
                Node::LinearTimeWarp {
                    effect: EffectData {
                        effect_name: "LinearTimeWarp".to_string(),
                        ..EffectData::new()
                    },
                    time_scalar: fps / rate,
                }
            };

            let effect = self.document.insert(effect);
            self.document
                .try_get_mut(clip)?
                .item_mut()
                .expect("a clip is an item")
                .effects
                .push(effect);

            return Ok((record_in, record_out));
        }

        if self.options.ignore_timecode_mismatch {
            // Believe the source and move the record out to match. Nothing
            // downstream reads record_out again; adjusting it just states
            // what ignoring the mismatch amounts to.
            return Ok((record_in, record_in + source_duration));
        }

        Err(Error::parse(format!(
            "source and record duration don't match: {} != {} for clip {}",
            source_duration.value(),
            record_duration.value(),
            self.document.try_get(clip)?.name()
        )))
    }

    /// Puts the clip, and its transition, on every track the event targets.
    fn place(
        &mut self,
        clip: NodeId,
        transition: Option<NodeId>,
        channel: &str,
        record_in: RationalTime,
        record_out: RationalTime,
        rate: f64,
    ) -> Result<()> {
        let _ = record_out;
        let tracks = self.tracks_for_channel(channel)?;

        let mut record_in = record_in;
        for (index, track) in tracks.iter().copied().enumerate() {
            // One event can land on several tracks, and each needs its own
            // copy: an object belongs to one composition. The first track
            // takes the original, so nothing is left unparented.
            let (track_clip, track_transition) = if index == 0 {
                (clip, transition)
            } else {
                (
                    self.document.deep_clone(clip)?,
                    transition
                        .map(|id| self.document.deep_clone(id))
                        .transpose()?,
                )
            };

            let start = self.track_start(track, record_in, rate)?;
            let end = self.document.duration(track)? - start;

            if record_in < end {
                if self.options.ignore_timecode_mismatch {
                    record_in = end;
                } else {
                    return Err(Error::parse(format!(
                        "overlapping record in value: {} for clip {}",
                        record_in.value(),
                        self.document.try_get(track_clip)?.name()
                    )));
                }
            }

            // Record timecodes can be sparse — one track of a multi-track cut
            // read on its own leaves holes — and a hole in the record is a
            // gap on the track.
            let children = self.document.children_of(track)?.len();
            if record_in > end && children > 0 {
                let gap = self.document.insert(Node::Gap(otio_core::schema::Gap {
                    item: ItemData {
                        source_range: Some(TimeRange::new(
                            RationalTime::new(0.0, rate),
                            record_in - end,
                        )),
                        ..ItemData::new()
                    },
                }));
                self.document.append_child(track, gap)?;
                let duration = self.document.duration(gap)?;
                self.extend_track(track, duration)?;
            }

            if let Some(transition) = track_transition {
                if self.document.children_of(track)?.is_empty() {
                    return Err(Error::parse(
                        "transitions can't be at the very beginning of a track",
                    ));
                }
                self.document.append_child(track, transition)?;
            }
            self.document.append_child(track, track_clip)?;
            let duration = self.document.duration(track_clip)?;
            self.extend_track(track, duration)?;
        }

        Ok(())
    }

    /// Returns a track's start offset, giving it one if it has none yet.
    ///
    /// A track's `source_range` starts at minus the record time of its first
    /// event, so that adding each event's duration keeps the track's end in
    /// record time. Once the file is read the offset is dropped again if it
    /// says nothing.
    fn track_start(
        &mut self,
        track: NodeId,
        record_in: RationalTime,
        rate: f64,
    ) -> Result<RationalTime> {
        let existing = self
            .document
            .try_get(track)?
            .item()
            .expect("a track is an item")
            .source_range;
        if let Some(range) = existing {
            return Ok(range.start_time());
        }

        let zero = RationalTime::new(0.0, rate);
        let range = TimeRange::new(zero - record_in, zero);
        self.document
            .try_get_mut(track)?
            .item_mut()
            .expect("a track is an item")
            .source_range = Some(range);
        Ok(range.start_time())
    }

    /// Lengthens a track's own range by what was just added to it.
    fn extend_track(&mut self, track: NodeId, duration: RationalTime) -> Result<()> {
        let item = self
            .document
            .try_get_mut(track)?
            .item_mut()
            .expect("a track is an item");
        if let Some(range) = item.source_range {
            item.source_range = Some(range.duration_extended_by(duration));
        }
        Ok(())
    }

    /// Returns the tracks a channel shorthand names, creating any that are new.
    fn tracks_for_channel(&mut self, channel: &str) -> Result<Vec<NodeId>> {
        let names: Vec<String> = CHANNEL_MAP
            .iter()
            .find(|(code, _)| *code == channel)
            .map_or_else(
                || vec![channel.to_string()],
                |(_, names)| names.iter().map(|name| (*name).to_string()).collect(),
            );

        let mut tracks = Vec::with_capacity(names.len());
        for name in names {
            if let Some(track) = self.tracks_by_name.get(&name) {
                tracks.push(*track);
                continue;
            }

            let kind = if name.starts_with('A') {
                "Audio"
            } else {
                "Video"
            };
            let track = self.document.insert(Node::Track(Track {
                item: ItemData {
                    base: Base {
                        name: name.clone(),
                        metadata: AnyDictionary::new(),
                    },
                    ..ItemData::new()
                },
                children: Vec::new(),
                kind: kind.to_string(),
            }));
            self.document.append_child(self.stack, track)?;
            self.tracks_by_name.insert(name, track);
            tracks.push(track);
        }

        Ok(tracks)
    }

    /// Drops each track's own range where it says nothing the track does not.
    ///
    /// The range exists only to keep running totals while reading. A track
    /// starting at zero and as long as its contents is the same track without
    /// one, and leaving it set would put a redundant field in every document.
    fn settle_track_ranges(&mut self) -> Result<()> {
        for track in self.document.children_of(self.stack)? {
            let source_range = self
                .document
                .try_get(track)?
                .item()
                .expect("a track is an item")
                .source_range;
            let available = self.document.available_range(track)?;
            if source_range == Some(available) {
                self.document
                    .try_get_mut(track)?
                    .item_mut()
                    .expect("a track is an item")
                    .source_range = None;
            }
        }
        Ok(())
    }
}

/// Returns whether an edit type is a wipe, which is `W` and a three-digit code.
fn is_wipe(edit_type: &str) -> bool {
    let Some(code) = edit_type.strip_prefix('W') else {
        return false;
    };
    code.len() >= 3 && code.as_bytes()[..3].iter().all(u8::is_ascii_digit)
}

/// Reads the playback speed out of an `M2` comment.
///
/// The body is a name, then the speed in frames per second, then the timecode
/// the speed change starts at, each separated by whatever whitespace the
/// writing system felt like. Only the last two are fixed in shape, so read
/// from the end.
fn speed_of(motion: &str) -> Result<f64> {
    let malformed = || {
        Error::parse(format!(
            "could not read the speed of an M2 comment: {motion}"
        ))
    };

    // The timecode is eleven characters of digits and colons at the end.
    let bytes = motion.as_bytes();
    if bytes.len() < 11 {
        return Err(malformed());
    }
    let timecode_at = bytes.len() - 11;
    if !bytes[timecode_at..]
        .iter()
        .all(|byte| byte.is_ascii_digit() || *byte == b':')
    {
        return Err(malformed());
    }

    let head = motion[..timecode_at].trim_end();
    let digits = head
        .rfind(|character: char| !matches!(character, '0'..='9' | '.'))
        .map_or(0, |at| at + character_width(head, at));
    let speed = &head[digits..];
    let speed = match head[..digits].ends_with('-') {
        true => &head[digits - 1..],
        false => speed,
    };

    speed.parse().map_err(|_| malformed())
}

/// Returns how many bytes the character starting at `at` occupies.
fn character_width(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(1, char::len_utf8)
}

/// Reads the colour decision an event's comments state.
///
/// Stricter than the ALE reader's: an EDL states the values in a fixed shape,
/// and one that does not is more likely to be a file this adapter should
/// refuse than a value to guess at.
fn read_cdl(comments: &Comments) -> Result<Cdl> {
    if comments.asc_sop.is_none() && comments.asc_sat.is_none() {
        return Ok(Cdl::default());
    }

    let sop = match &comments.asc_sop {
        Some(text) => Some(
            parse_sop(text)
                .ok_or_else(|| Error::parse(format!("invalid ASC_SOP found: {text}")))?,
        ),
        // Upstream fills in the identity when only a saturation is stated, so
        // the metadata shape is the same either way.
        None => Some(Sop::default()),
    };
    let sat = match &comments.asc_sat {
        Some(text) => Some(
            text.trim()
                .parse()
                .map_err(|_| Error::parse(format!("invalid ASC_SAT found: {text}")))?,
        ),
        None => Some(1.0),
    };

    Ok(Cdl { sop, sat })
}

/// Parses `(a b c) (d e f) (g h i)`, with optional commas after the numbers.
fn parse_sop(text: &str) -> Option<Sop> {
    let mut rest = text;
    let mut triples = [[0.0; 3]; 3];

    for triple in &mut triples {
        rest = rest.trim_start();
        rest = rest.strip_prefix('(')?;
        for (index, value) in triple.iter_mut().enumerate() {
            let end = rest
                .find(|character: char| !matches!(character, '0'..='9' | '.' | '-' | '+'))
                .unwrap_or(rest.len());
            *value = rest[..end].parse().ok()?;
            rest = &rest[end..];
            rest = rest.strip_prefix(',').unwrap_or(rest);
            if index < 2 {
                // Upstream requires exactly one space between the numbers of
                // a triple, and files honour it.
                rest = rest.strip_prefix(' ')?;
            }
        }
        rest = rest.strip_prefix(')')?;
    }

    Some(Sop {
        slope: triples[0],
        offset: triples[1],
        power: triples[2],
    })
}

#[cfg(test)]
mod tests {
    use super::{is_wipe, parse_sop, speed_of};

    #[test]
    fn reads_a_speed_from_an_m2_comment() {
        assert!(
            (speed_of("Z686_5A          47.56               01:00:06:00").unwrap() - 47.56).abs()
                < 1e-9
        );
        assert!(
            (speed_of("test clip5 (speed)\t\t48.0\t\t\t00:00:00:00").unwrap() - 48.0).abs() < 1e-9
        );
        assert!((speed_of("reverse -24.0 00:00:00:00").unwrap() + 24.0).abs() < 1e-9);
    }

    #[test]
    fn a_wipe_is_w_and_three_digits() {
        assert!(is_wipe("W001"));
        assert!(!is_wipe("W1"));
        assert!(!is_wipe("D"));
    }

    #[test]
    fn reads_colour_decisions_with_and_without_commas() {
        let spaces = parse_sop("(0.1 0.2 0.3) (1.0 -0.0122 0.0305) (1.0 0.0 1.0)").expect("valid");
        assert_eq!(spaces.slope, [0.1, 0.2, 0.3]);
        assert_eq!(spaces.offset, [1.0, -0.0122, 0.0305]);

        let commas = parse_sop(
            "(1.1549, 1.1469, 1.1422)(-0.0678, -0.0555, -0.0323)(1.1325, 1.1351, 1.1221)",
        )
        .expect("valid");
        assert_eq!(commas.slope, [1.1549, 1.1469, 1.1422]);
    }
}

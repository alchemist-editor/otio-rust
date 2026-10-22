//! Live review over a sync session: strokes while they are drawn, review
//! comments committed to the session's ledger, and the host folding those
//! comments into the timeline.
//!
//! ```text
//!  reviewer                         server                        host
//!  ────────                         ──────                        ────
//!  send_partial ── Annotation.1/PARTIAL ──────────────────────► read_partial
//!   (while drawing; never stored)                                (draw it, drop it)
//!
//!  commit ─────── review_commit ──► ledger ── review_commit ──► fold
//!                                     │                           │ merges the commands
//!  ReviewCommitted ◄── review_committed                           │ into the Annotations
//!                                                                 ▼ track, then
//!  every client ◄──────────── patch ◄───────────────────────── publish
//! ```
//!
//! A finished annotation is an ordinary timeline change, so it reaches
//! everyone as a patch, as any edit does. Only two things travel otherwise:
//! strokes in progress, on a channel this module registers
//! ([`PARTIAL_CHANNEL`], latest-wins), in the form ORI's `PARTIAL` message
//! has; and review comments, on sync's own review ledger, which keeps each
//! one until the server acknowledges it.
//!
//! The host folds a comment in by uuid ([`annotations::merge_commands`]),
//! so its strokes join whatever the frame already holds, and records the
//! comment in the annotation clip's `metadata.alchemist.comments`, which is
//! how Alchemist's review design records whose strokes are whose. A comment
//! may add strokes, and replace its author's own, but never replace
//! someone else's ([`Error::NotPermitted`]); only the host and drivers, who
//! change the timeline directly, may.

use opentime::RationalTime;
use otio_core::json::{self, Number, Value};
use otio_core::schema::Node;
use otio_core::{Any, AnyDictionary, Document, NodeId};
use otio_ext_sync::session::{Channel, Delivery, Event, ReviewCommit, ReviewRelay, Session};

use crate::annotations::{self, Merged};
use crate::command::{self, Command};
use crate::{Error, Result};

/// The channel strokes in progress travel on. Latest-wins: a stroke's
/// newer state replaces an older one not yet sent.
pub const PARTIAL_CHANNEL: &str = "ori.partial";

/// The message kind of a stroke in progress, as ORI's wire names it:
/// `Annotation.1` `PARTIAL`.
pub const PARTIAL_KIND: &str = "Annotation.1/PARTIAL";

/// The metadata key, on an annotation clip, of what Alchemist records
/// about it: `comments`, and where the first comment was made.
pub const ALCHEMIST_KEY: &str = "alchemist";

/// A stroke, text or shape while it is being drawn, before pen-up: ORI's
/// `PartialAnnotation`. Peers show it and never store it; the finished
/// annotation arrives later as a timeline change or a review comment.
#[derive(Debug, Clone, PartialEq)]
pub struct Partial {
    /// The sync GUID of the media clip being drawn on.
    pub clip_guid: String,
    /// The source frame of that clip.
    pub frame: f64,
    /// The rate `frame` is counted at.
    pub fps: f64,
    /// Everything drawn so far, each message carrying the whole of it, as
    /// ORI's does.
    pub commands: Vec<Command>,
}

impl Partial {
    /// The message's payload, as ORI writes it: `clip_guid`, `frame`, `fps`
    /// and `events`, the commands.
    ///
    /// # Errors
    ///
    /// [`Error::NotEnabled`] if the extension is off, and what writing a
    /// command reports.
    pub fn to_json(&self) -> Result<String> {
        let events = commands_json(&self.commands)?;
        Ok(format!(
            "{{\"clip_guid\":{},\"frame\":{},\"fps\":{},\"events\":{events}}}",
            json::escape(&self.clip_guid),
            float(self.frame),
            float(self.fps),
        ))
    }

    /// Reads a payload [`Partial::to_json`] or ORI wrote. A command that
    /// leaves a field out takes ORI's default for it, as in
    /// [`command::from_json`].
    ///
    /// # Errors
    ///
    /// [`Error::NotEnabled`] if the extension is off, and [`Error::Json`]
    /// if it is not a partial annotation.
    pub fn from_json(text: &str) -> Result<Self> {
        crate::require_enabled()?;
        let value = json::parse(text).map_err(|error| Error::Json(error.to_string()))?;
        let clip_guid = value
            .get("clip_guid")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Json("a partial annotation needs a clip_guid".into()))?
            .to_string();
        let frame = value
            .get("frame")
            .and_then(number)
            .ok_or_else(|| Error::Json("a partial annotation needs a frame".into()))?;
        let fps = value.get("fps").and_then(number).unwrap_or(0.0);
        let commands = match value.get("events") {
            None | Some(Value::Null) => Vec::new(),
            Some(_) => command::from_json(&events_text(text)?)?,
        };
        Ok(Self {
            clip_guid,
            frame,
            fps,
            commands,
        })
    }
}

/// Registers [`PARTIAL_CHANNEL`] on `session`. [`send_partial`] does it
/// too; calling it again does nothing new.
///
/// # Errors
///
/// [`Error::Session`] if the session refuses.
pub fn register(session: &mut Session) -> Result<()> {
    session.register_channel(PARTIAL_CHANNEL, Delivery::LatestWins)?;
    Ok(())
}

/// Sends `partial` to everyone in the session as a stroke in progress.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and [`Error::Session`]
/// if the session has not joined.
pub fn send_partial(session: &mut Session, partial: &Partial) -> Result<()> {
    let payload = partial.to_json()?;
    register(session)?;
    session.send(
        Channel::Named(PARTIAL_CHANNEL.to_string()),
        PARTIAL_KIND,
        payload,
    )?;
    Ok(())
}

/// The stroke in progress `event` carries, if it is one: a message of kind
/// [`PARTIAL_KIND`], from this library or from ORI's RV and xStudio
/// plugins.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and [`Error::Json`] if
/// the event is a partial annotation that cannot be read.
pub fn read_partial(event: &Event) -> Result<Option<Partial>> {
    match event {
        Event::Message { kind, payload, .. } if kind == PARTIAL_KIND => {
            Partial::from_json(payload).map(Some)
        }
        _ => Ok(None),
    }
}

/// Where a review comment is made: Furnace's `anchor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    /// The session's id. [`commit`] fills it in when it is empty.
    pub session_id: String,
    /// The sequence the reviewer was looking at.
    pub sequence_id: String,
    /// The generation of that sequence they saw.
    pub captured_generation: i64,
    /// The sync GUID of the media clip.
    pub clip_guid: String,
    /// The source frame of that clip, which the annotation clip's start
    /// time becomes.
    pub source_time: RationalTime,
    /// Where that frame was in the sequence.
    pub captured_sequence_time: RationalTime,
    /// The media's key, if the program has one.
    pub media_key: Option<String>,
    /// The image's width and height in pixels, if known.
    pub image_size: Option<(u32, u32)>,
    /// When it was made, in Unix milliseconds.
    pub captured_at: i64,
}

impl Anchor {
    /// The anchor as Furnace's JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fingerprint = Vec::new();
        if let Some(key) = &self.media_key {
            fingerprint.push(format!("\"mediaKey\":{}", json::escape(key)));
        }
        if let Some((width, height)) = self.image_size {
            fingerprint.push(format!("\"imageWidth\":{width},\"imageHeight\":{height}"));
        }
        let fingerprint = if fingerprint.is_empty() {
            String::new()
        } else {
            format!(",\"mediaFingerprint\":{{{}}}", fingerprint.join(","))
        };
        format!(
            "{{\"sessionId\":{},\"sequenceId\":{},\"capturedGeneration\":{},\"clipGuid\":{},\"sourceTime\":{},\"capturedSequenceTime\":{}{fingerprint},\"capturedAt\":{}}}",
            json::escape(&self.session_id),
            json::escape(&self.sequence_id),
            self.captured_generation,
            json::escape(&self.clip_guid),
            time_json(self.source_time),
            time_json(self.captured_sequence_time),
            self.captured_at,
        )
    }

    /// Reads Furnace's JSON. Only `clipGuid` and `sourceTime` are needed;
    /// the rest default to empty, zero or unknown.
    ///
    /// # Errors
    ///
    /// [`Error::Json`] if it is not an anchor.
    pub fn from_json(text: &str) -> Result<Self> {
        let value = json::parse(text).map_err(|error| Error::Json(error.to_string()))?;
        let text_of = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let clip_guid = text_of("clipGuid");
        if clip_guid.is_empty() {
            return Err(Error::Json("a review anchor needs a clipGuid".into()));
        }
        let source_time = value
            .get("sourceTime")
            .and_then(time)
            .ok_or_else(|| Error::Json("a review anchor needs a sourceTime".into()))?;
        let fingerprint = value.get("mediaFingerprint");
        let field = |key: &str| fingerprint.and_then(|fingerprint| fingerprint.get(key));
        let size = |key: &str| {
            field(key)
                .and_then(number)
                .and_then(|size| whole(size).and_then(|size| u32::try_from(size).ok()))
        };
        Ok(Self {
            session_id: text_of("sessionId"),
            sequence_id: text_of("sequenceId"),
            captured_generation: value
                .get("capturedGeneration")
                .and_then(number)
                .and_then(whole)
                .unwrap_or(0),
            clip_guid,
            source_time,
            captured_sequence_time: value
                .get("capturedSequenceTime")
                .and_then(time)
                .unwrap_or_default(),
            media_key: field("mediaKey")
                .and_then(Value::as_str)
                .map(str::to_string),
            image_size: size("imageWidth").zip(size("imageHeight")),
            captured_at: value
                .get("capturedAt")
                .and_then(number)
                .and_then(whole)
                .unwrap_or(0),
        })
    }
}

/// Commits a review comment, with the strokes, text and shapes in
/// `commands`, to the session's ledger, and returns its mutation id: the
/// one given, or a new one if `client_mutation_id` is empty. The session
/// sends it again after a reconnect until the server acknowledges it.
///
/// `document` and `timeline` are this client's copy, checked first: the
/// commit may not replace commands, by uuid, that belong to someone else
/// on the anchored frame. That is the same rule the host's [`fold`]
/// applies, checked here so a reviewer learns at once rather than from the
/// host.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off; [`Error::NotPermitted`]
/// as above; [`Error::Session`] if the session has not joined, its
/// identity lacks the `review.comment.create` capability, or the id was
/// used for a different comment.
pub fn commit(
    session: &mut Session,
    document: &Document,
    timeline: NodeId,
    anchor: &Anchor,
    body_markdown: &str,
    commands: &[Command],
    client_mutation_id: &str,
) -> Result<String> {
    crate::require_enabled()?;
    let author = session.self_user_id().map(str::to_string);
    if let Some(clip) = anchored_clip(document, timeline, anchor)? {
        check_owner(document, clip, commands, author.as_deref())?;
    }
    let mut anchor = anchor.clone();
    if anchor.session_id.is_empty() {
        anchor.session_id = session.session_id().unwrap_or_default().to_string();
    }
    let annotation_commands = commands
        .iter()
        .map(|command| commands_json(std::slice::from_ref(command)).map(|list| unlist(&list)))
        .collect::<Result<Vec<_>>>()?;
    Ok(session.commit_review(ReviewCommit {
        client_mutation_id: client_mutation_id.to_string(),
        anchor: anchor.to_json(),
        body_markdown: body_markdown.to_string(),
        annotation_commands,
    })?)
}

/// What [`fold`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Folded {
    /// The annotation clip the comment went into.
    pub clip: NodeId,
    /// What merging its commands did.
    pub merged: Merged,
    /// Whether the comment was there already, at this revision or a later
    /// one, so nothing changed: Furnace sends its ledger again when a host
    /// reconnects.
    pub already: bool,
}

/// Folds a review comment the server relayed into `timeline`, on the host:
/// finds or adds the annotation track and the clip for the anchored media
/// clip and frame, merges the comment's commands in by uuid, and records
/// the comment in the clip's `metadata.alchemist.comments` (its id,
/// author, Markdown, the uuids of its commands, `state` `open` and its
/// revision). The clip's `note` becomes the comments' Markdown, one after
/// another, and its `status` `open`. The host then publishes the timeline,
/// which sends everyone the change.
///
/// Folding the same comment again changes nothing.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off; [`Error::Json`] if the
/// anchor or a command cannot be read; [`Error::NotPermitted`] if the
/// comment would replace commands that are not its author's, in which case
/// nothing changes; and what [`annotations::ensure_track`] reports.
pub fn fold(document: &mut Document, timeline: NodeId, relay: &ReviewRelay) -> Result<Folded> {
    crate::require_enabled()?;
    let anchor = Anchor::from_json(&relay.commit.anchor)?;
    let commands =
        command::from_json(&format!("[{}]", relay.commit.annotation_commands.join(",")))?;
    let author = relay.author_user_id.as_str();
    let comment_id = relay.ack.comment_id.as_str();

    if let Some(clip) = anchored_clip(document, timeline, &anchor)? {
        if recorded_revision(document, clip, comment_id)
            .is_some_and(|seen| seen >= relay.ack.revision)
        {
            return Ok(Folded {
                clip,
                merged: Merged::default(),
                already: true,
            });
        }
        check_owner(document, clip, &commands, Some(author))?;
    }

    let track = annotations::ensure_track(document, timeline)?;
    let clip = annotations::ensure_clip(document, track, &anchor.clip_guid, anchor.source_time)?;
    let merged = annotations::merge_commands(document, clip, &commands)?;

    let mut uuids: Vec<Any> = Vec::new();
    for command in &commands {
        if let Some(uuid) = command.uuid() {
            if !uuids.iter().any(|seen| seen.as_str() == Some(uuid)) {
                uuids.push(Any::String(uuid.to_string()));
            }
        }
    }
    let comment = AnyDictionary::from([
        ("commentId".to_string(), Any::String(comment_id.to_string())),
        ("authorUserId".to_string(), Any::String(author.to_string())),
        (
            "bodyMarkdown".to_string(),
            Any::String(relay.commit.body_markdown.clone()),
        ),
        ("commandUuids".to_string(), Any::Vector(uuids)),
        ("state".to_string(), Any::String("open".to_string())),
        (
            "revision".to_string(),
            Any::Int(i64::try_from(relay.ack.revision).unwrap_or(i64::MAX)),
        ),
    ]);
    let metadata = clip_metadata(document, clip)?;
    let alchemist = dictionary_entry(metadata, ALCHEMIST_KEY);
    let comments = match alchemist
        .entry("comments".to_string())
        .or_insert_with(|| Any::Vector(Vec::new()))
    {
        Any::Vector(comments) => comments,
        other => {
            *other = Any::Vector(Vec::new());
            match other {
                Any::Vector(comments) => comments,
                _ => unreachable!("made a list just above"),
            }
        }
    };
    match comments.iter_mut().find(|existing| {
        existing
            .as_dictionary()
            .and_then(|existing| existing.get("commentId"))
            .and_then(Any::as_str)
            == Some(comment_id)
    }) {
        Some(existing) => *existing = Any::Dictionary(comment),
        None => comments.push(Any::Dictionary(comment)),
    }
    let note = comments
        .iter()
        .filter_map(|comment| comment.as_dictionary()?.get("bodyMarkdown")?.as_str())
        .filter(|body| !body.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    alchemist
        .entry("capturedSequenceId".to_string())
        .or_insert_with(|| Any::String(anchor.sequence_id.clone()));
    alchemist
        .entry("capturedGeneration".to_string())
        .or_insert(Any::Int(anchor.captured_generation));
    alchemist
        .entry("capturedSequenceFrame".to_string())
        .or_insert_with(|| Any::Double(anchor.captured_sequence_time.value()));
    if !note.is_empty() {
        metadata.insert(annotations::NOTE_KEY.to_string(), Any::String(note));
    }
    metadata.insert(
        annotations::STATUS_KEY.to_string(),
        Any::String("open".to_string()),
    );
    Ok(Folded {
        clip,
        merged,
        already: false,
    })
}

/// Who the commands with `uuid` on an annotation clip belong to, by person
/// id: the author of the comment in `metadata.alchemist.comments` that
/// lists the uuid. `None` if no comment does, as for strokes drawn over
/// ORI's wire or read from a file.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and an otio-core error
/// for a clip not in the document.
pub fn owner_of(document: &Document, clip: NodeId, uuid: &str) -> Result<Option<String>> {
    crate::require_enabled()?;
    let Node::Clip(clip) = document.try_get(clip)? else {
        return Ok(None);
    };
    Ok(comments(&clip.item.base.metadata)
        .iter()
        .filter_map(Any::as_dictionary)
        .find(|comment| {
            comment
                .get("commandUuids")
                .and_then(Any::as_slice)
                .is_some_and(|uuids| uuids.iter().any(|seen| seen.as_str() == Some(uuid)))
        })
        .and_then(|comment| comment.get("authorUserId")?.as_str())
        .map(str::to_string))
}

/// The annotation clip `anchor` names, if the timeline has it yet.
fn anchored_clip(document: &Document, timeline: NodeId, anchor: &Anchor) -> Result<Option<NodeId>> {
    let Some(track) = annotations::find_track(document, timeline)? else {
        return Ok(None);
    };
    annotations::find_clip(
        document,
        track,
        &anchor.clip_guid,
        whole_frames(anchor.source_time.value()),
    )
}

/// Refuses `commands` if any replaces, by uuid, commands on `clip` that
/// are not `author`'s.
fn check_owner(
    document: &Document,
    clip: NodeId,
    commands: &[Command],
    author: Option<&str>,
) -> Result<()> {
    let present: Vec<&str> = annotations::commands(document, clip)?
        .into_iter()
        .filter_map(|id| command::uuid_of(document, id))
        .collect();
    for uuid in commands.iter().filter_map(Command::uuid) {
        if !present.contains(&uuid) {
            continue;
        }
        let owner = owner_of(document, clip, uuid)?;
        if owner.is_none() || owner.as_deref() != author {
            return Err(Error::NotPermitted {
                uuid: uuid.to_string(),
                owner,
            });
        }
    }
    Ok(())
}

/// The revision of comment `comment_id` recorded on `clip`, if it is.
fn recorded_revision(document: &Document, clip: NodeId, comment_id: &str) -> Option<u64> {
    let Some(Node::Clip(clip)) = document.get(clip) else {
        return None;
    };
    comments(&clip.item.base.metadata)
        .iter()
        .filter_map(Any::as_dictionary)
        .find(|comment| comment.get("commentId").and_then(Any::as_str) == Some(comment_id))
        .and_then(|comment| match comment.get("revision") {
            Some(Any::Int(revision)) => u64::try_from(*revision).ok(),
            Some(Any::UInt(revision)) => Some(*revision),
            _ => Some(0),
        })
}

fn comments(metadata: &AnyDictionary) -> &[Any] {
    metadata
        .get(ALCHEMIST_KEY)
        .and_then(Any::as_dictionary)
        .and_then(|alchemist| alchemist.get("comments"))
        .and_then(Any::as_slice)
        .unwrap_or_default()
}

fn clip_metadata(document: &mut Document, clip: NodeId) -> Result<&mut AnyDictionary> {
    match document.try_get_mut(clip)? {
        Node::Clip(clip) => Ok(&mut clip.item.base.metadata),
        node => Err(Error::WrongKind {
            expected: "an annotation clip",
            found: node.schema_name().to_string(),
        }),
    }
}

/// `dictionary[key]`, made an empty dictionary if it is not one.
fn dictionary_entry<'a>(dictionary: &'a mut AnyDictionary, key: &str) -> &'a mut AnyDictionary {
    let value = dictionary
        .entry(key.to_string())
        .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
    if !matches!(value, Any::Dictionary(_)) {
        *value = Any::Dictionary(AnyDictionary::new());
    }
    match value {
        Any::Dictionary(inner) => inner,
        _ => unreachable!("made a dictionary just above"),
    }
}

/// `commands` written as a compact JSON list.
fn commands_json(commands: &[Command]) -> Result<String> {
    crate::require_enabled()?;
    let mut scratch = Document::new();
    let ids = commands
        .iter()
        .map(|command| command.insert(&mut scratch))
        .collect::<Result<Vec<_>>>()?;
    command::to_json(&scratch, &ids, None)
}

/// The one element of a JSON list written with no spaces around it.
fn unlist(list: &str) -> String {
    list.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(list)
        .to_string()
}

/// The text of a partial's `events` list, as it was written.
fn events_text(payload: &str) -> Result<String> {
    let (scratch, value) =
        otio_core::from_str_any(payload).map_err(|error| Error::Json(error.to_string()))?;
    let events = value
        .as_dictionary()
        .and_then(|fields| fields.get("events"))
        .cloned()
        .unwrap_or(Any::Vector(Vec::new()));
    otio_core::to_string_with(&scratch, &events, &otio_core::WriteOptions::default())
        .map_err(Error::from)
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(Number::as_f64(*number)),
        _ => None,
    }
}

fn time(value: &Value) -> Option<RationalTime> {
    let rate = value.get("rate").and_then(number)?;
    let at = value.get("value").and_then(number)?;
    Some(RationalTime::new(at, rate))
}

fn time_json(time: RationalTime) -> String {
    format!(
        "{{\"value\":{},\"rate\":{}}}",
        float(time.value()),
        float(time.rate())
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "only whole numbers in range reach the cast"
)]
fn whole(value: f64) -> Option<i64> {
    (value.fract() == 0.0 && value.abs() < 9.0e15).then_some(value as i64)
}

/// A number as JSON: whole numbers without a fraction, as Furnace and
/// ORI's Python write them for frames and rates alike.
fn float(value: f64) -> String {
    match whole(value) {
        Some(whole) => whole.to_string(),
        None => format!("{value}"),
    }
}

/// A frame count the way ORI's Python takes one, `int(value)`: towards
/// zero.
#[allow(clippy::cast_possible_truncation, reason = "Python's int() truncates")]
fn whole_frames(value: f64) -> i64 {
    value as i64
}

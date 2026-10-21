//! Where annotations live on a timeline, and finding, adding and changing
//! them.
//!
//! The form written here is the one ORI's sync protocol and Alchemist's
//! review design both chose: a track named `Annotations` beside the media,
//! holding one one-frame clip for each annotated source frame of each media
//! clip.
//!
//! ```text
//!  Stack
//!  ├─ Track "V1"           ┌ Clip "shot"  metadata.sync.guid = "shot" ┐
//!  └─ Track "Annotations"  ├ Gap (0..10) ┬ Clip "Annotation_10" ──────┤
//!                                        │  source_range 10 + 1 frame │
//!                                        │  metadata:                 │
//!                                        │    clip_guid: "shot" ──────┘
//!                                        │    annotation_commands: [PaintStart, PaintPoint, …]
//!                                        │    sync.guid: …
//! ```
//!
//! The annotation clip's `source_range.start_time` is the annotated frame of
//! the media clip, and `metadata.clip_guid` names that clip by its sync
//! GUID. This follows ORI's `SyncManager` exactly, including two things that
//! look odd: a frame later than everything on the track is reached with a
//! gap, but one earlier is simply appended after the rest, so the track's
//! layout is not a timeline of the frames; and every lookup goes by
//! `clip_guid` and frame, never by position.
//!
//! [`list`] also reads the two older forms ORI has written: its review
//! export, where the track is named after the review and each clip names
//! the media in `annotated_clip_name`; and `AnnotationEffect.1`, an effect
//! on the media clip holding its commands. Neither is ever written.

use opentime::{RationalTime, TimeRange};
use otio_core::schema::{Clip, Gap, ItemData, MissingReference, Node, Track};
use otio_core::{Any, AnyDictionary, DEFAULT_MEDIA_KEY, Document, NodeId, TRACK_KIND_VIDEO};
use otio_ext_sync::guid;

use crate::command::{self, Command};
use crate::schema;
use crate::{Error, Result};

/// The name of the track annotations are written to.
pub const TRACK_NAME: &str = "Annotations";

/// The metadata key holding an annotation clip's commands.
pub const COMMANDS_KEY: &str = "annotation_commands";

/// The metadata key naming, by sync GUID, the media clip an annotation clip
/// annotates.
pub const CLIP_GUID_KEY: &str = "clip_guid";

/// The metadata key ORI's review export names the media clip by, by name.
pub const CLIP_NAME_KEY: &str = "annotated_clip_name";

/// The metadata key holding a review note, in Markdown.
pub const NOTE_KEY: &str = "note";

/// The metadata key holding a review status, such as `open`.
pub const STATUS_KEY: &str = "status";

/// The field of `AnnotationEffect.1` holding its commands.
const EFFECT_COMMANDS: &str = "commands";

/// One frame's annotations, as [`list`] finds them.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// The object holding the commands: an annotation clip, or an
    /// `AnnotationEffect.1`.
    pub holder: NodeId,
    /// Which form it is in.
    pub form: Form,
    /// What it annotates.
    pub target: Target,
    /// The annotated source frame, from the annotation clip's source range.
    /// `None` for an effect, which covers its whole clip.
    pub frame: Option<RationalTime>,
    /// Its commands, in order.
    pub commands: Vec<NodeId>,
    /// Its review note, if it has one.
    pub note: Option<String>,
    /// Its review status, if it has one.
    pub status: Option<String>,
}

impl Annotation {
    /// What the annotation says about itself, as a JSON object: `form`
    /// (`track` or `effect`), `clip_guid` (the annotated clip's sync GUID,
    /// or `null`), `clip_name` (its name where only that is known, as in
    /// ORI's review export, or `null`), `frame` (`{"value", "rate"}`, or
    /// `null` for an effect), `note` and `status` (text or `null`), and
    /// `command_count`. `document` is the one it was read from.
    #[must_use]
    pub fn to_json(&self, document: &Document) -> String {
        let quoted =
            |value: Option<&str>| value.map_or_else(|| "null".to_string(), otio_core::json::escape);
        let (clip_guid, clip_name) = match &self.target {
            Target::ClipGuid(guid) => (Some(guid.as_str()), None),
            Target::ClipName(name) => (None, Some(name.as_str())),
            Target::Item(item) => (
                guid::guid(document, *item),
                document
                    .get(*item)
                    .and_then(Node::base)
                    .map(|base| base.name.as_str()),
            ),
            Target::Unknown => (None, None),
        };
        let frame = self.frame.map_or_else(
            || "null".to_string(),
            |time| {
                format!(
                    "{{\"value\":{},\"rate\":{}}}",
                    number(time.value()),
                    number(time.rate())
                )
            },
        );
        format!(
            "{{\"form\":\"{}\",\"clip_guid\":{},\"clip_name\":{},\"frame\":{},\"note\":{},\"status\":{},\"command_count\":{}}}",
            match self.form {
                Form::Effect => "effect",
                Form::Track => "track",
            },
            quoted(clip_guid),
            quoted(clip_name),
            frame,
            quoted(self.note.as_deref()),
            quoted(self.status.as_deref()),
            self.commands.len()
        )
    }
}

/// A number as OTIO writes one: whole numbers with `.0`.
fn number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.1}")
    } else if value.is_finite() {
        format!("{value}")
    } else {
        "null".to_string()
    }
}

/// The forms annotations take on a timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Form {
    /// A one-frame clip on an annotation track: the form written here, and
    /// ORI's review export.
    Track,
    /// An `AnnotationEffect.1` on the media clip; read only.
    Effect,
}

/// What an annotation annotates.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// The media clip with this sync GUID.
    ClipGuid(String),
    /// The media clip with this name, as ORI's review export says.
    ClipName(String),
    /// The item an `AnnotationEffect.1` sits on.
    Item(NodeId),
    /// The annotation clip does not say.
    Unknown,
}

/// The first track of `timeline` whose name contains "annotation", in any
/// case, as ORI looks for it.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and
/// [`Error::WrongKind`] if `timeline` is not a timeline.
pub fn find_track(document: &Document, timeline: NodeId) -> Result<Option<NodeId>> {
    crate::require_enabled()?;
    let stack = stack_of(document, timeline)?;
    Ok(document.children_of(stack)?.into_iter().find(|child| {
        matches!(
            document.get(*child),
            Some(Node::Track(track)) if is_annotation_track_name(&track.item.base.name)
        )
    }))
}

/// The annotation track of `timeline`, added as a video track named
/// `Annotations` on top of its stack, with a new sync GUID, if
/// [`find_track`] finds none.
///
/// # Errors
///
/// As [`find_track`].
pub fn ensure_track(document: &mut Document, timeline: NodeId) -> Result<NodeId> {
    if let Some(track) = find_track(document, timeline)? {
        return Ok(track);
    }
    let stack = stack_of(document, timeline)?;
    let mut item = ItemData::new();
    item.base.name = TRACK_NAME.to_string();
    let track = document.insert(Node::Track(Track {
        item,
        children: Vec::new(),
        kind: TRACK_KIND_VIDEO.to_string(),
    }));
    guid::set_guid(document, track, guid::new_guid())?;
    document.append_child(stack, track)?;
    Ok(track)
}

/// The annotation clip on `track` for the media clip whose sync GUID is
/// `clip_guid`, at source frame `frame`, as ORI matches it: by
/// `metadata.clip_guid` and the whole number of frames in
/// `source_range.start_time`.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and
/// [`Error::WrongKind`] if `track` is not a track.
pub fn find_clip(
    document: &Document,
    track: NodeId,
    clip_guid: &str,
    frame: i64,
) -> Result<Option<NodeId>> {
    crate::require_enabled()?;
    let children = track_children(document, track)?;
    Ok(children.iter().copied().find(|child| {
        let Some(Node::Clip(clip)) = document.get(*child) else {
            return false;
        };
        clip.item
            .base
            .metadata
            .get(CLIP_GUID_KEY)
            .and_then(Any::as_str)
            == Some(clip_guid)
            && clip
                .item
                .source_range
                .is_some_and(|range| whole_frames(range.start_time().value()) == frame)
    }))
}

/// The annotation clip on `track` for `clip_guid` at `time`, added if
/// [`find_clip`] finds none.
///
/// A new clip is named `Annotation_<frame>`, lasts one frame at `time`'s
/// rate from `time`, holds an empty `annotation_commands` and `clip_guid`,
/// has a `MissingReference` as its media and a new sync GUID, and goes at
/// the end of the track. When `time` is later than the end of the track, a
/// gap with a new sync GUID fills the difference first, so the clip sits at
/// its own frame; when it is earlier, the clip is appended anyway. Both are
/// what ORI's `SyncManager` does.
///
/// # Errors
///
/// As [`find_clip`].
pub fn ensure_clip(
    document: &mut Document,
    track: NodeId,
    clip_guid: &str,
    time: RationalTime,
) -> Result<NodeId> {
    let frame = whole_frames(time.value());
    if let Some(clip) = find_clip(document, track, clip_guid, frame)? {
        return Ok(clip);
    }
    let end = track_end(document, track)?;
    #[allow(
        clippy::cast_precision_loss,
        reason = "frame counts are far below 2^53"
    )]
    if frame > end {
        let mut item = ItemData::new();
        item.source_range = Some(TimeRange::new(
            RationalTime::new(end as f64, time.rate()),
            RationalTime::new((frame - end) as f64, time.rate()),
        ));
        let gap = document.insert(Node::Gap(Gap { item }));
        guid::set_guid(document, gap, guid::new_guid())?;
        document.append_child(track, gap)?;
    }

    let reference = document.insert(Node::MissingReference(MissingReference::default()));
    let mut item = ItemData::new();
    item.base.name = format!("Annotation_{frame}");
    item.source_range = Some(TimeRange::new(time, RationalTime::new(1.0, time.rate())));
    item.base
        .metadata
        .insert(COMMANDS_KEY.to_string(), Any::Vector(Vec::new()));
    item.base.metadata.insert(
        CLIP_GUID_KEY.to_string(),
        Any::String(clip_guid.to_string()),
    );
    let clip = document.insert(Node::Clip(Clip {
        item,
        media_references: [(DEFAULT_MEDIA_KEY.to_string(), reference)].into(),
        active_media_reference_key: DEFAULT_MEDIA_KEY.to_string(),
    }));
    guid::set_guid(document, clip, guid::new_guid())?;
    document.append_child(track, clip)?;
    Ok(clip)
}

/// The commands `holder` holds, in order: an annotation clip's
/// `annotation_commands`, or an `AnnotationEffect.1`'s `commands`. Entries
/// that are not objects are left out.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and
/// [`otio_core::Error::StaleHandle`] if `holder` is not in the document.
pub fn commands(document: &Document, holder: NodeId) -> Result<Vec<NodeId>> {
    crate::require_enabled()?;
    Ok(command_values(document.try_get(holder)?)
        .map(objects)
        .unwrap_or_default())
}

/// Adds `commands` at the end of an annotation clip's commands, and returns
/// their handles.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and
/// [`Error::WrongKind`] if `clip` is not a clip.
pub fn add_commands(
    document: &mut Document,
    clip: NodeId,
    commands: &[Command],
) -> Result<Vec<NodeId>> {
    crate::require_enabled()?;
    command_list(document, clip)?;
    let ids = insert_all(document, commands)?;
    command_list(document, clip)?.extend(ids.iter().copied().map(Any::Object));
    Ok(ids)
}

/// Replaces every command whose `uuid` is `uuid` with `commands`, put where
/// the first of them was, and leaves the rest of the clip's commands alone.
/// Returns whether any command had that uuid; if none did, nothing changes.
///
/// A stroke's commands share a uuid, so this replaces a whole stroke.
///
/// # Errors
///
/// As [`add_commands`].
pub fn replace_commands(
    document: &mut Document,
    clip: NodeId,
    uuid: &str,
    commands: &[Command],
) -> Result<bool> {
    crate::require_enabled()?;
    let list = command_list(document, clip)?.clone();
    if !list
        .iter()
        .any(|value| value_uuid(document, value) == Some(uuid))
    {
        return Ok(false);
    }
    let mut new_ids = Some(insert_all(document, commands)?);
    let mut kept = Vec::with_capacity(list.len());
    let mut removed = Vec::new();
    for value in list {
        if value_uuid(document, &value) == Some(uuid) {
            if let Some(ids) = new_ids.take() {
                kept.extend(ids.into_iter().map(Any::Object));
            }
            removed.push(value);
        } else {
            kept.push(value);
        }
    }
    *command_list(document, clip)? = kept;
    drop_values(document, removed);
    Ok(true)
}

/// Removes every command whose `uuid` is one of `uuids`, and returns how
/// many it removed.
///
/// # Errors
///
/// As [`add_commands`].
pub fn remove_commands(document: &mut Document, clip: NodeId, uuids: &[&str]) -> Result<usize> {
    crate::require_enabled()?;
    let list = command_list(document, clip)?.clone();
    let (removed, kept): (Vec<Any>, Vec<Any>) = list
        .into_iter()
        .partition(|value| value_uuid(document, value).is_some_and(|uuid| uuids.contains(&uuid)));
    let count = removed.len();
    *command_list(document, clip)? = kept;
    drop_values(document, removed);
    Ok(count)
}

/// Replaces all of an annotation clip's commands with `commands`, as ORI's
/// `REPLACE_ANNOTATION_COMMANDS` does. An empty list clears the clip.
///
/// # Errors
///
/// As [`add_commands`].
pub fn set_commands(document: &mut Document, clip: NodeId, commands: &[Command]) -> Result<()> {
    crate::require_enabled()?;
    let old = std::mem::take(command_list(document, clip)?);
    let ids = insert_all(document, commands)?;
    *command_list(document, clip)? = ids.into_iter().map(Any::Object).collect();
    drop_values(document, old);
    Ok(())
}

/// Every annotation on `timeline`, in the order its tracks and items are
/// walked, in each of the forms ORI has written.
///
/// A clip anywhere under the timeline whose metadata holds
/// `annotation_commands` is an annotation clip, whatever its track is
/// called, which is how ORI's review export is read. An
/// `AnnotationEffect.1` on any item is one too.
///
/// # Errors
///
/// [`Error::NotEnabled`] if the extension is off, and
/// [`Error::WrongKind`] if `timeline` is not a timeline.
pub fn list(document: &Document, timeline: NodeId) -> Result<Vec<Annotation>> {
    crate::require_enabled()?;
    let stack = stack_of(document, timeline)?;
    let mut found = Vec::new();
    walk(document, stack, &mut found)?;
    Ok(found)
}

/// The annotations of the media clip whose sync GUID is `clip_guid`.
///
/// # Errors
///
/// As [`list`].
pub fn for_clip(document: &Document, timeline: NodeId, clip_guid: &str) -> Result<Vec<Annotation>> {
    let mut found = list(document, timeline)?;
    found.retain(|annotation| match &annotation.target {
        Target::ClipGuid(guid) => guid == clip_guid,
        Target::Item(item) => guid::guid(document, *item) == Some(clip_guid),
        _ => false,
    });
    Ok(found)
}

/// The annotations of the media clip whose sync GUID is `clip_guid`, at
/// source frame `frame`. An `AnnotationEffect.1` covers every frame of its
/// clip.
///
/// # Errors
///
/// As [`list`].
pub fn at_frame(
    document: &Document,
    timeline: NodeId,
    clip_guid: &str,
    frame: i64,
) -> Result<Vec<Annotation>> {
    let mut found = for_clip(document, timeline, clip_guid)?;
    found.retain(|annotation| {
        annotation
            .frame
            .is_none_or(|time| whole_frames(time.value()) == frame)
    });
    Ok(found)
}

/// The annotations holding work by the person whose `participant_hash` is
/// `participant`, each with only that person's commands: the strokes whose
/// `PaintStart` carries the hash, every command sharing a stroke's uuid.
///
/// # Errors
///
/// As [`list`].
pub fn by_participant(
    document: &Document,
    timeline: NodeId,
    participant: &str,
) -> Result<Vec<Annotation>> {
    let mut found = list(document, timeline)?;
    for annotation in &mut found {
        let theirs: Vec<&str> = annotation
            .commands
            .iter()
            .filter_map(|id| match Command::read(document, *id) {
                Ok(Command::PaintStart(start))
                    if start.participant_hash.as_deref() == Some(participant) =>
                {
                    command::uuid_of(document, *id)
                }
                _ => None,
            })
            .collect();
        annotation.commands.retain(|id| {
            command::uuid_of(document, *id).is_some_and(|uuid| theirs.contains(&uuid))
        });
    }
    found.retain(|annotation| !annotation.commands.is_empty());
    Ok(found)
}

/// The annotation `holder` holds, an annotation clip or an
/// `AnnotationEffect.1`, read as [`list`] reads it.
///
/// # Errors
///
/// [`Error::NotEnabled`] while the extension is off, [`Error::WrongKind`]
/// for an object that holds no annotation, and an otio-core error for one
/// not in the document.
pub fn describe(document: &Document, holder: NodeId) -> Result<Annotation> {
    crate::require_enabled()?;
    let node = document.try_get(holder)?;
    let owner = || {
        document
            .owner_of(holder)
            .map_or(Target::Unknown, Target::Item)
    };
    read(holder, node, owner).ok_or_else(|| Error::WrongKind {
        expected: "an annotation clip or an AnnotationEffect",
        found: node.schema_name().to_string(),
    })
}

/// The annotation `id` holds, if it holds one. An effect's target is
/// `owner`'s, which the caller knows from where it found the effect.
fn read(id: NodeId, node: &Node, owner: impl FnOnce() -> Target) -> Option<Annotation> {
    if node.schema_name() == schema::ANNOTATION_EFFECT {
        return Some(Annotation {
            holder: id,
            form: Form::Effect,
            target: owner(),
            frame: None,
            commands: command_values(node).map(objects).unwrap_or_default(),
            note: None,
            status: None,
        });
    }
    let Node::Clip(clip) = node else {
        return None;
    };
    let metadata = &clip.item.base.metadata;
    let Some(Any::Vector(values)) = metadata.get(COMMANDS_KEY) else {
        return None;
    };
    let target = match (metadata.get(CLIP_GUID_KEY), metadata.get(CLIP_NAME_KEY)) {
        (Some(Any::String(guid)), _) => Target::ClipGuid(guid.clone()),
        (_, Some(Any::String(name))) => Target::ClipName(name.clone()),
        _ => Target::Unknown,
    };
    Some(Annotation {
        holder: id,
        form: Form::Track,
        target,
        frame: clip.item.source_range.map(|range| range.start_time()),
        commands: objects(values),
        note: string(metadata, NOTE_KEY),
        status: string(metadata, STATUS_KEY),
    })
}

fn walk(document: &Document, id: NodeId, found: &mut Vec<Annotation>) -> Result<()> {
    let node = document.try_get(id)?;
    if let Some(item) = node.item() {
        for effect in &item.effects {
            let effect_node = document.try_get(*effect)?;
            if effect_node.schema_name() == schema::ANNOTATION_EFFECT {
                found.extend(read(*effect, effect_node, || Target::Item(id)));
            }
        }
    }
    if matches!(node, Node::Clip(_)) {
        found.extend(read(id, node, || Target::Unknown));
    }
    if let Some(children) = node.children() {
        for child in children.iter().copied() {
            walk(document, child, found)?;
        }
    }
    Ok(())
}

fn string(metadata: &AnyDictionary, key: &str) -> Option<String> {
    metadata.get(key).and_then(Any::as_str).map(str::to_string)
}

fn command_values(node: &Node) -> Option<&[Any]> {
    let value = if node.schema_name() == schema::ANNOTATION_EFFECT {
        node.base()?.extension_fields()?.get(EFFECT_COMMANDS)?
    } else {
        node.base()?.metadata.get(COMMANDS_KEY)?
    };
    match value {
        Any::Vector(values) => Some(values.as_slice()),
        _ => None,
    }
}

fn objects(values: &[Any]) -> Vec<NodeId> {
    values
        .iter()
        .filter_map(|value| match value {
            Any::Object(id) => Some(*id),
            _ => None,
        })
        .collect()
}

/// The command list of an annotation clip, made an empty list if it has
/// none or holds something else.
fn command_list(document: &mut Document, clip: NodeId) -> Result<&mut Vec<Any>> {
    let node = document.try_get_mut(clip)?;
    let Node::Clip(clip) = node else {
        return Err(Error::WrongKind {
            expected: "an annotation clip",
            found: node.schema_name().to_string(),
        });
    };
    let value = clip
        .item
        .base
        .metadata
        .entry(COMMANDS_KEY.to_string())
        .or_insert_with(|| Any::Vector(Vec::new()));
    if !matches!(value, Any::Vector(_)) {
        *value = Any::Vector(Vec::new());
    }
    match value {
        Any::Vector(values) => Ok(values),
        _ => unreachable!("made a list just above"),
    }
}

fn insert_all(document: &mut Document, commands: &[Command]) -> Result<Vec<NodeId>> {
    let mut ids = Vec::with_capacity(commands.len());
    for command in commands {
        match command.insert(document) {
            Ok(id) => ids.push(id),
            Err(error) => {
                for id in ids {
                    command::drop_object(document, id);
                }
                return Err(error);
            }
        }
    }
    Ok(ids)
}

fn drop_values(document: &mut Document, values: impl IntoIterator<Item = Any>) {
    for mut value in values {
        let mut held = Vec::new();
        value.visit_objects_mut(&mut |id: &mut NodeId| held.push(*id));
        for id in held {
            command::drop_object(document, id);
        }
    }
}

fn value_uuid<'a>(document: &'a Document, value: &'a Any) -> Option<&'a str> {
    match value {
        Any::Object(id) => command::uuid_of(document, *id),
        // ORI reads a command list of plain dictionaries too.
        Any::Dictionary(fields) => fields.get("uuid").and_then(Any::as_str),
        _ => None,
    }
}

fn is_annotation_track_name(name: &str) -> bool {
    name.to_lowercase().contains("annotation")
}

fn stack_of(document: &Document, timeline: NodeId) -> Result<NodeId> {
    match document.try_get(timeline)? {
        Node::Timeline(timeline) => timeline.tracks.ok_or(Error::WrongKind {
            expected: "a timeline with tracks",
            found: "a timeline with none".to_string(),
        }),
        node => Err(Error::WrongKind {
            expected: "a timeline",
            found: node.schema_name().to_string(),
        }),
    }
}

fn track_children(document: &Document, track: NodeId) -> Result<&[NodeId]> {
    match document.try_get(track)? {
        Node::Track(track) => Ok(&track.children),
        node => Err(Error::WrongKind {
            expected: "a track",
            found: node.schema_name().to_string(),
        }),
    }
}

/// The frame after everything on the track, by ORI's count: the sum of
/// every child's whole-frame duration.
fn track_end(document: &Document, track: NodeId) -> Result<i64> {
    Ok(track_children(document, track)?
        .iter()
        .filter_map(|child| document.get(*child)?.item()?.source_range)
        .map(|range| whole_frames(range.duration().value()))
        .sum())
}

/// A frame count the way ORI's Python takes one, `int(value)`: towards
/// zero.
#[allow(clippy::cast_possible_truncation, reason = "Python's int() truncates")]
fn whole_frames(value: f64) -> i64 {
    value as i64
}

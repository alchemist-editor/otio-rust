//! Multicam: synced angles held once on a timeline, and cuts that play them.
//!
//! **EXPERIMENTAL.** This is the otio-rust half of a prototype for upstream
//! issue #427, built to gather feedback on a proposal. It is not an approved
//! schema; see `docs/adr/0014-multicam-prototype.md`. The C++ half lives on a
//! branch of Jeff Hodges's OpenTimelineIO fork and writes the same JSON.
//!
//! Three things make up the model:
//!
//! - A **`Multicam.1`** is a [`Stack`](crate::schema::Stack) whose children are
//!   the angles, one `Stack` each, and which may carry a `global_start_time`
//!   (the source timecode at multicam time zero). An angle's name is its
//!   identity. An angle holds at most one video track and any number of audio
//!   tracks, all on the multicam's own time.
//! - A **`MulticamClip.1`** is an [`Item`](crate::Node::Item): one cut to one
//!   angle on one track. It names the multicam by its key on the timeline,
//!   the angle by name, and which of the angle's tracks of its own track's
//!   kind it plays (`angle_track`). Its `source_range` is in multicam time.
//! - A timeline holds its multicams in a **`multicams`** field, a dictionary
//!   from key to `Multicam`, the way a clip holds its media references.
//!
//! Both schemas are built in as subclasses (see
//! [`register_subclass`](crate::registry::register_subclass)): a multicam is a
//! `Stack` and a cut an `Item` everywhere else in the library, so timing,
//! editing and every adapter treat them as such. Their own fields, and the
//! timeline's `multicams`, are extension fields.

use std::collections::{BTreeMap, BTreeSet};

use opentime::{RationalTime, TimeRange};

use crate::arena::{Document, NodeId};
use crate::error::{Error, Result};
use crate::schema::{Base, Extension, Gap, ItemData, Node, Stack};
use crate::value::{Any, AnyDictionary};

/// The schema name of a multicam definition.
pub const MULTICAM: &str = "Multicam";

/// The schema name of a cut to one angle of a multicam.
pub const MULTICAM_CLIP: &str = "MulticamClip";

/// The timeline field holding multicam definitions by key.
pub const MULTICAMS_FIELD: &str = "multicams";

/// The metadata namespace flattening writes provenance under.
pub const METADATA_KEY: &str = "otio.multicam";

/// The subclasses the registry builds in: name, version, built-in base.
pub(crate) const SUBCLASSES: [(&str, u32, &str); 2] =
    [(MULTICAM, 1, "Stack"), (MULTICAM_CLIP, 1, "Item")];

/// What a cut plays: the fields a `MulticamClip` adds to an `Item`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cut {
    /// The multicam's key in the timeline's `multicams`.
    pub multicam: String,
    /// The name of the live angle.
    pub angle: String,
    /// Which of the angle's tracks plays, counting only tracks of the kind
    /// of the track holding the cut.
    pub angle_track: i64,
}

/// A cut resolved against its timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    /// The multicam definition.
    pub multicam: NodeId,
    /// The angle's stack.
    pub angle: NodeId,
    /// The angle track the cut plays.
    pub track: NodeId,
}

fn extension(node: &Node) -> Option<&AnyDictionary> {
    node.base()?.extension_fields()
}

/// Whether `node` is a multicam definition.
#[must_use]
pub fn is_multicam(node: &Node) -> bool {
    matches!(node, Node::Stack(_)) && node.schema_name() == MULTICAM
}

/// Whether `node` is a cut to a multicam angle.
#[must_use]
pub fn is_multicam_clip(node: &Node) -> bool {
    matches!(node, Node::Item(_)) && node.schema_name() == MULTICAM_CLIP
}

/// The cut a `MulticamClip` describes, or `None` for any other object.
#[must_use]
pub fn cut(node: &Node) -> Option<Cut> {
    if !is_multicam_clip(node) {
        return None;
    }
    let fields = extension(node);
    let string = |key: &str| {
        fields
            .and_then(|fields| fields.get(key))
            .and_then(Any::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let angle_track = fields
        .and_then(|fields| fields.get("angle_track"))
        .and_then(|value| match value {
            Any::Int(int) => Some(*int),
            Any::UInt(uint) => i64::try_from(*uint).ok(),
            _ => None,
        })
        .unwrap_or(0);
    Some(Cut {
        multicam: string("multicam"),
        angle: string("angle"),
        angle_track,
    })
}

fn subclass_base(name: &str, schema: &str, fields: AnyDictionary) -> Base {
    Base {
        name: name.to_string(),
        metadata: AnyDictionary::new(),
        extension: Some(Box::new(Extension {
            schema: Some((schema.to_string(), 1)),
            fields,
        })),
    }
}

fn cut_fields(cut: &Cut) -> AnyDictionary {
    let mut fields = AnyDictionary::new();
    fields.insert("multicam".into(), Any::String(cut.multicam.clone()));
    fields.insert("angle".into(), Any::String(cut.angle.clone()));
    fields.insert("angle_track".into(), Any::Int(cut.angle_track));
    fields
}

/// Adds an empty multicam definition to `document`.
pub fn new_multicam(
    document: &mut Document,
    name: &str,
    global_start_time: Option<RationalTime>,
) -> NodeId {
    let mut fields = AnyDictionary::new();
    if let Some(time) = global_start_time {
        fields.insert("global_start_time".into(), Any::RationalTime(time));
    }
    let mut item = ItemData::new();
    item.base = subclass_base(name, MULTICAM, fields);
    document.insert(Node::Stack(Stack {
        item,
        children: Vec::new(),
    }))
}

/// Adds a cut to `document`, not yet on any track.
pub fn new_multicam_clip(
    document: &mut Document,
    name: &str,
    cut: &Cut,
    source_range: TimeRange,
) -> NodeId {
    let mut item = ItemData::new();
    item.base = subclass_base(name, MULTICAM_CLIP, cut_fields(cut));
    item.source_range = Some(source_range);
    document.insert(Node::Item(item))
}

/// Replaces what a cut plays.
///
/// # Errors
///
/// [`Error::NotAnItem`] if `id` is not a `MulticamClip`.
pub fn set_cut(document: &mut Document, id: NodeId, cut: &Cut) -> Result<()> {
    let node = document.try_get_mut(id)?;
    if !is_multicam_clip(node) {
        return Err(Error::NotAnItem);
    }
    let fields = node
        .base_mut()
        .ok_or(Error::NotAnItem)?
        .extension_fields_mut();
    fields.extend(cut_fields(cut));
    Ok(())
}

/// A multicam's `global_start_time`, if it has one.
#[must_use]
pub fn global_start_time(node: &Node) -> Option<RationalTime> {
    match extension(node)?.get("global_start_time")? {
        Any::RationalTime(time) => Some(*time),
        _ => None,
    }
}

/// The angle stack named `name`, if the multicam has one.
///
/// # Errors
///
/// A stale handle.
pub fn angle(document: &Document, multicam: NodeId, name: &str) -> Result<Option<NodeId>> {
    for child in document.children_of(multicam)? {
        let node = document.try_get(child)?;
        if matches!(node, Node::Stack(_)) && node.name() == name {
            return Ok(Some(child));
        }
    }
    Ok(None)
}

/// The angle names of a multicam, in display order.
///
/// # Errors
///
/// A stale handle.
pub fn angle_names(document: &Document, multicam: NodeId) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for child in document.children_of(multicam)? {
        let node = document.try_get(child)?;
        if matches!(node, Node::Stack(_)) {
            names.push(node.name().to_string());
        }
    }
    Ok(names)
}

/// An angle's tracks of `kind` (`"Video"` or `"Audio"`), in order.
///
/// # Errors
///
/// A stale handle.
pub fn angle_tracks(document: &Document, angle: NodeId, kind: &str) -> Result<Vec<NodeId>> {
    let mut tracks = Vec::new();
    for child in document.children_of(angle)? {
        if let Node::Track(track) = document.try_get(child)? {
            if track.kind == kind {
                tracks.push(child);
            }
        }
    }
    Ok(tracks)
}

/// The multicam definitions a timeline holds, by key.
///
/// # Errors
///
/// A stale handle.
pub fn multicams(document: &Document, timeline: NodeId) -> Result<BTreeMap<String, NodeId>> {
    let node = document.try_get(timeline)?;
    let mut found = BTreeMap::new();
    if let Some(Any::Dictionary(entries)) = extension(node).and_then(|f| f.get(MULTICAMS_FIELD)) {
        for (key, value) in entries {
            if let Any::Object(id) = value {
                found.insert(key.clone(), *id);
            }
        }
    }
    Ok(found)
}

/// The multicam a timeline holds under `key`.
///
/// # Errors
///
/// A stale handle.
pub fn multicam(document: &Document, timeline: NodeId, key: &str) -> Result<Option<NodeId>> {
    Ok(multicams(document, timeline)?.get(key).copied())
}

/// Puts `multicam` on the timeline under `key`, or with `None` takes the key
/// off. A replaced or removed definition stays in the document.
///
/// # Errors
///
/// A stale handle, or [`Error::NotAComposition`] if `timeline` is not one.
pub fn set_multicam(
    document: &mut Document,
    timeline: NodeId,
    key: &str,
    multicam: Option<NodeId>,
) -> Result<()> {
    let node = document.try_get_mut(timeline)?;
    if !matches!(node, Node::Timeline(_)) {
        return Err(Error::NotAComposition {
            schema: node.schema_name().to_string(),
        });
    }
    let fields = node
        .base_mut()
        .ok_or(Error::NotAnItem)?
        .extension_fields_mut();
    let entry = fields
        .entry(MULTICAMS_FIELD.to_string())
        .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
    if let Any::Dictionary(entries) = entry {
        match multicam {
            Some(id) => {
                entries.insert(key.to_string(), Any::Object(id));
            }
            None => {
                entries.remove(key);
            }
        }
        if entries.is_empty() {
            fields.remove(MULTICAMS_FIELD);
        }
    }
    let base = document
        .try_get_mut(timeline)?
        .base_mut()
        .ok_or(Error::NotAnItem)?;
    if base
        .extension
        .as_ref()
        .is_some_and(|ext| ext.schema.is_none() && ext.fields.is_empty())
    {
        base.extension = None;
    }
    Ok(())
}

/// The kind of the track holding `id`, looking up through nested stacks.
fn track_kind_of(document: &Document, id: NodeId) -> Result<String> {
    let mut current = document.try_get(id)?.item().and_then(|item| item.parent);
    while let Some(parent) = current {
        let node = document.try_get(parent)?;
        if let Node::Track(track) = node {
            return Ok(track.kind.clone());
        }
        current = node.item().and_then(|item| item.parent);
    }
    Ok(crate::TRACK_KIND_VIDEO.to_string())
}

fn unresolved(reason: String) -> Error {
    Error::UnresolvedMulticam { reason }
}

/// Finds what a cut plays.
///
/// `kind` defaults to the kind of the track holding the cut.
///
/// # Errors
///
/// [`Error::UnresolvedMulticam`] naming the multicam, angle or angle track
/// that is missing; [`Error::NotAnItem`] if `id` is not a `MulticamClip`.
pub fn resolve(
    document: &Document,
    timeline: NodeId,
    id: NodeId,
    kind: Option<&str>,
) -> Result<Resolved> {
    let node = document.try_get(id)?;
    let cut = cut(node).ok_or(Error::NotAnItem)?;
    let name = node.name().to_string();
    let kind = match kind {
        Some(kind) => kind.to_string(),
        None => track_kind_of(document, id)?,
    };
    let multicam = multicam(document, timeline, &cut.multicam)?
        .ok_or_else(|| unresolved(format!("'{name}': no multicam with key '{}'", cut.multicam)))?;
    let angle = angle(document, multicam, &cut.angle)?.ok_or_else(|| {
        unresolved(format!(
            "'{name}': multicam '{}' has no angle '{}'",
            cut.multicam, cut.angle
        ))
    })?;
    let tracks = angle_tracks(document, angle, &kind)?;
    let track = usize::try_from(cut.angle_track)
        .ok()
        .and_then(|index| tracks.get(index).copied())
        .ok_or_else(|| {
            unresolved(format!(
                "'{name}': angle '{}' has {} {kind} track(s), angle_track is {}",
                cut.angle,
                tracks.len(),
                cut.angle_track
            ))
        })?;
    Ok(Resolved {
        multicam,
        angle,
        track,
    })
}

fn cuts_under(document: &Document, parent: NodeId) -> Result<Vec<NodeId>> {
    document.find_children(parent, None, false, &is_multicam_clip)
}

/// Lists everything wrong with a timeline's multicams; empty when valid.
///
/// Checks that angle names are unique within each multicam, that no
/// multicam reaches itself through the cuts in its angles, and that every
/// cut, on the timeline's tracks or inside a multicam, resolves.
///
/// # Errors
///
/// A stale handle.
pub fn validate(document: &Document, timeline: NodeId) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let definitions = multicams(document, timeline)?;

    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (key, &id) in &definitions {
        let names = angle_names(document, id)?;
        let mut seen = BTreeSet::new();
        let mut dupes = BTreeSet::new();
        for name in &names {
            if !seen.insert(name) {
                dupes.insert(name.clone());
            }
        }
        if !dupes.is_empty() {
            problems.push(format!(
                "multicam '{key}': duplicate angle names {:?}",
                dupes.into_iter().collect::<Vec<_>>()
            ));
        }
        let targets = cuts_under(document, id)?
            .into_iter()
            .filter_map(|cut_id| document.get(cut_id).and_then(cut))
            .map(|cut| cut.multicam)
            .collect();
        edges.insert(key.clone(), targets);
    }

    for key in definitions.keys() {
        let mut stack: Vec<&String> = edges.get(key).into_iter().flatten().collect();
        let mut seen = BTreeSet::new();
        while let Some(next) = stack.pop() {
            if next == key {
                problems.push(format!("multicam '{key}' contains itself"));
                break;
            }
            if seen.insert(next) {
                stack.extend(edges.get(next).into_iter().flatten());
            }
        }
    }

    let mut cuts = Vec::new();
    if let Node::Timeline(node) = document.try_get(timeline)? {
        if let Some(tracks) = node.tracks {
            cuts.extend(cuts_under(document, tracks)?);
        }
    }
    for &id in definitions.values() {
        cuts.extend(cuts_under(document, id)?);
    }
    for id in cuts {
        match resolve(document, timeline, id, None) {
            Ok(_) => {}
            Err(Error::UnresolvedMulticam { reason }) => problems.push(reason),
            Err(other) => return Err(other),
        }
    }
    Ok(problems)
}

fn gap(duration: RationalTime) -> Node {
    let mut item = ItemData::new();
    item.source_range = Some(TimeRange::new(
        RationalTime::new(0.0, duration.rate()),
        duration,
    ));
    Node::Gap(Gap { item })
}

const MAX_DEPTH: usize = 32;

/// The new items a cut plays, in the cut's time, not yet parented.
fn slice(
    document: &mut Document,
    timeline: NodeId,
    id: NodeId,
    report: &mut Vec<String>,
    depth: usize,
) -> Result<Vec<NodeId>> {
    let range = document.trimmed_range(id)?;
    let name = document.try_get(id)?.name().to_string();
    let the_cut = cut(document.try_get(id)?).unwrap_or_default();
    let resolved = match resolve(document, timeline, id, None) {
        Ok(resolved) => resolved,
        Err(Error::UnresolvedMulticam { reason }) => {
            report.push(format!("{reason} (replaced with a gap)"));
            return Ok(vec![document.insert(gap(range.duration()))]);
        }
        Err(other) => return Err(other),
    };
    if depth > MAX_DEPTH {
        report.push(format!("'{name}': multicam nesting too deep"));
        return Ok(vec![document.insert(gap(range.duration()))]);
    }

    let trimmed = crate::algorithm::track_trimmed_to_range(document, resolved.track, range)?;
    flatten_children(document, timeline, trimmed, report, depth + 1)?;
    let mut items = Vec::new();
    let mut played = RationalTime::new(0.0, range.duration().rate());
    while !document.children_of(trimmed)?.is_empty() {
        let child = document.remove_child(trimmed, 0)?;
        if !matches!(document.try_get(child)?, Node::Transition(_)) {
            played += document.duration(child)?;
        }
        items.push(child);
    }
    document.remove_recursive(trimmed)?;
    let shortfall = range.duration() - played;
    if shortfall.value() > 0.0 {
        items.push(document.insert(gap(shortfall)));
    }

    // effects and markers on the cut go to what it plays, when that is one item
    let (effects, markers) = {
        let item = document.try_get(id)?.item().ok_or(Error::NotAnItem)?;
        (item.effects.clone(), item.markers.clone())
    };
    if !effects.is_empty() || !markers.is_empty() {
        if let [only] = items[..] {
            if let Some(item) = document.try_get_mut(id)?.item_mut() {
                item.effects.clear();
                item.markers.clear();
            }
            let target = document
                .try_get_mut(only)?
                .item_mut()
                .ok_or(Error::NotAnItem)?;
            target.effects.extend(effects);
            target.markers.extend(markers);
        } else {
            report.push(format!(
                "'{name}': effects/markers on a cut spanning {} items were dropped",
                items.len()
            ));
        }
    }

    for &item in &items {
        if let Some(base) = document.try_get_mut(item)?.base_mut() {
            let mut from = AnyDictionary::new();
            from.insert("multicam".into(), Any::String(the_cut.multicam.clone()));
            from.insert("angle".into(), Any::String(the_cut.angle.clone()));
            from.insert("angle_track".into(), Any::Int(the_cut.angle_track));
            let entry = base
                .metadata
                .entry(METADATA_KEY.to_string())
                .or_insert_with(|| Any::Dictionary(AnyDictionary::new()));
            if let Any::Dictionary(entry) = entry {
                entry.insert("flattened_from".into(), Any::Dictionary(from));
            }
        }
    }
    Ok(items)
}

fn flatten_children(
    document: &mut Document,
    timeline: NodeId,
    composition: NodeId,
    report: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    let children = document.children_of(composition)?;
    let is_track = matches!(document.try_get(composition)?, Node::Track(_));
    for (index, child) in children.into_iter().enumerate().rev() {
        let node = document.try_get(child)?;
        if is_multicam_clip(node) {
            let items = slice(document, timeline, child, report, depth)?;
            let removed = document.remove_child(composition, index as i64)?;
            document.remove_recursive(removed)?;
            if is_track {
                for (offset, item) in items.into_iter().enumerate() {
                    document.insert_child(composition, (index + offset) as i64, item)?;
                }
            } else {
                let mut item = ItemData::new();
                item.base = Base::default();
                let track = document.insert(Node::Track(crate::schema::Track {
                    item,
                    children: Vec::new(),
                    kind: track_kind_of(document, composition)
                        .unwrap_or_else(|_| crate::TRACK_KIND_VIDEO.to_string()),
                }));
                for item in items {
                    document.append_child(track, item)?;
                }
                document.insert_child(composition, index as i64, track)?;
            }
        } else if node.children().is_some() {
            flatten_children(document, timeline, child, report, depth)?;
        }
    }
    Ok(())
}

/// Replaces every cut on a timeline's tracks with what its angle plays, and
/// takes the multicam definitions off the timeline.
///
/// A cut becomes the slice of its angle track it covers: one or more clips
/// and gaps, padded with a gap where the angle runs out. Cuts inside angles
/// are flattened too. Works in place; clone the timeline first to keep the
/// original. Returns what could not be carried over.
///
/// # Errors
///
/// A stale handle, or a trim that falls inside a transition.
pub fn flatten(document: &mut Document, timeline: NodeId) -> Result<Vec<String>> {
    let mut report = Vec::new();
    let tracks = match document.try_get(timeline)? {
        Node::Timeline(node) => node.tracks,
        other => {
            return Err(Error::NotAComposition {
                schema: other.schema_name().to_string(),
            });
        }
    };
    if let Some(tracks) = tracks {
        flatten_children(document, timeline, tracks, &mut report, 0)?;
    }
    for (key, id) in multicams(document, timeline)? {
        set_multicam(document, timeline, &key, None)?;
        document.remove_recursive(id)?;
    }
    Ok(report)
}

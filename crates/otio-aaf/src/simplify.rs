//! Upstream's `_simplify`: collapsing the nesting AAF has and OTIO does not
//! need.
//!
//! AAF wraps everything: a sequence in a slot in a mob, each clip's material
//! in a chain of mobs, each effect in a group around its inputs. Transcribed
//! as it stands, a simple edit comes out as tracks inside stacks inside
//! tracks. This pass pulls a composition's contents up into its parent
//! wherever the composition adds nothing — no effects, no metadata worth
//! keeping, no transitions a flattening would disturb — and drops what holds
//! nothing at all.
//!
//! It is ported as upstream writes it, including its choices about what is
//! lost along the way: metadata on a container that is flattened away, and
//! any offset its markers would need. Where upstream fails outright on a
//! shape no real file has been seen to have, noted at each place, this pass
//! leaves the shape as it is instead.

use std::collections::HashSet;

use opentime::{RationalTime, TimeRange};
use otio_core::algorithm::track_trimmed_to_range;
use otio_core::schema::Track;
use otio_core::{Any, AnyDictionary, Document, Error as OtioError, Node, NodeId};

use crate::error::Result;
use crate::transcribe::{item_fields, track_kind};

/// Simplifies what `root` holds, returning what should stand in its place.
///
/// A collection of one object becomes that object, so reading a file with a
/// single composition gives a timeline rather than a collection of one.
///
/// # Errors
///
/// Returns an error if a track or stack cannot be trimmed to its range,
/// which happens when an item in it does not say how long it is.
pub(crate) fn simplify(document: &mut Document, root: NodeId) -> Result<NodeId> {
    simplify_node(document, root)
}

/// Whether Python would find the object false: an empty composition or
/// collection. Everything else transcribed here is true.
fn is_falsy(document: &Document, id: NodeId) -> bool {
    match document.get(id) {
        Some(Node::Timeline(_)) | None => false,
        Some(node) => node.children().is_some_and(<[NodeId]>::is_empty),
    }
}

fn simplify_node(document: &mut Document, id: NodeId) -> Result<NodeId> {
    if is_falsy(document, id) {
        return Ok(id);
    }

    match document.try_get(id)? {
        Node::SerializableCollection(collection) => {
            let children = collection.children.clone();
            if let [only] = children[..] {
                return simplify_node(document, only);
            }
            for (index, child) in children.into_iter().enumerate() {
                let simplified = simplify_node(document, child)?;
                replace_child(document, id, index, simplified)?;
            }
            return Ok(id);
        }
        Node::Timeline(timeline) => {
            if let Some(tracks) = timeline.tracks {
                let simplified = simplify_node(document, tracks)?;
                if simplified != tracks && is_stack(document, simplified) {
                    if let Some(Node::Timeline(timeline)) = document.get_mut(id) {
                        timeline.tracks = Some(simplified);
                    }
                }
            }
            return Ok(id);
        }
        Node::Track(_) | Node::Stack(_) | Node::Composition(_) => {}
        _ => return Ok(id),
    }

    for (index, child) in document.children_of(id)?.into_iter().enumerate() {
        let simplified = simplify_node(document, child)?;
        replace_child(document, id, index, simplified)?;
    }

    if is_track(document, id) {
        flatten_track_children(document, id)?;
    } else if is_stack(document, id) {
        flatten_stack_children(document, id)?;
    }

    simplify_renderings(document, id)?;

    if is_redundant_container(document, id)? {
        return lift_only_child(document, id);
    }

    // A top-level stack may hold only tracks.
    if is_stack(document, id) && composition_parent(document, id).is_none() {
        ensure_stack_tracks(document, id)?;
    }
    Ok(id)
}

/// Pulls the contents of each flattenable track in a track up into it.
fn flatten_track_children(document: &mut Document, track: NodeId) -> Result<()> {
    let mut index = child_count(document, track).checked_sub(1);
    while let Some(at) = index {
        let original = document.children_of(track)?[at];
        if !track_item_can_flatten(document, original)? {
            index = at.checked_sub(1);
            continue;
        }

        let child = match source_range(document, original) {
            Some(range) => track_trimmed_to_range(document, original, range)?,
            None => original,
        };
        let effects = item_effects(document, child);
        let pulled = document.clear_children(child)?;
        if let [first] = pulled[..] {
            // Upstream moves the effects onto the one item; a transition
            // cannot hold them and fails there, so none is taken here.
            if let Some(item) = document.get_mut(first).and_then(Node::item_mut) {
                item.effects.extend(effects);
            }
            if has_time_effect(document, first) {
                fix_time_effect_duration(document, first, source_range(document, child))?;
            }
        }

        splice(document, track, at, &pulled)?;
        merge_markers_and_enabled(document, track, child);
        index = (at + pulled.len()).checked_sub(1);
    }
    Ok(())
}

/// Drops a stack's empty children, pulls up the contents of nested stacks,
/// and trims its tracks to its range.
fn flatten_stack_children(document: &mut Document, stack: NodeId) -> Result<()> {
    for (index, child) in document.children_of(stack)?.into_iter().enumerate().rev() {
        if !contains_something_valuable(document, child) {
            document.remove_child(stack, index as i64)?;
        }
    }

    let mut index = child_count(document, stack).checked_sub(1);
    while let Some(at) = index {
        let child = document.children_of(stack)?[at];
        let mut next = at.checked_sub(1);
        if stack_item_can_be_flattened(document, child) {
            // A track holding only a stack gives up that stack's tracks.
            let inner = if is_track(document, child) {
                document.children_of(child)?[0]
            } else {
                child
            };
            let pulled = document.clear_children(inner)?;
            splice(document, stack, at, &pulled)?;
            merge_markers_and_enabled(document, stack, inner);
            next = (at + pulled.len()).checked_sub(1);
        }
        index = next;
    }
    ensure_stack_tracks(document, stack)?;

    if let Some(range) = source_range(document, stack) {
        if !has_transitions(document, stack) {
            for (index, track) in document.children_of(stack)?.into_iter().enumerate() {
                let trimmed = track_trimmed_to_range(document, track, range)?;
                replace_child(document, stack, index, trimmed)?;
                for child in document.children_of(trimmed)? {
                    trim_markers(document, child)?;
                }
            }
            let duration = range.duration();
            set_source_range(
                document,
                stack,
                TimeRange::new(RationalTime::new(0.0, duration.rate()), duration),
            );
        }
    }
    Ok(())
}

/// Simplifies what an effect renders to, which upstream keeps in the
/// effect's `AAF` metadata.
fn simplify_renderings(document: &mut Document, id: NodeId) -> Result<()> {
    for effect in item_effects(document, id) {
        let rendering = match aaf_metadata(document, effect).and_then(|aaf| aaf.get("Rendering")) {
            Some(Any::Object(rendering)) => *rendering,
            _ => continue,
        };
        if is_falsy(document, rendering) {
            continue;
        }
        let simplified = simplify_node(document, rendering)?;
        if let Some(Any::Dictionary(aaf)) = document
            .get_mut(effect)
            .and_then(Node::base_mut)
            .and_then(|base| base.metadata.get_mut("AAF"))
        {
            aaf.insert("Rendering".to_owned(), Any::Object(simplified));
        }
    }
    Ok(())
}

/// Upstream's `_is_redundant_container`: a composition of one child that
/// says nothing the child does not.
///
/// A top-level track is kept, because a timeline's stack should hold
/// tracks, unless what it holds is itself a track. Upstream also lifts a
/// lone transition and then fails on it; one is left where it is here.
fn is_redundant_container(document: &Document, id: NodeId) -> Result<bool> {
    let Some(children) = document.try_get(id)?.children() else {
        return Ok(false);
    };
    if !matches!(
        document.try_get(id)?,
        Node::Track(_) | Node::Stack(_) | Node::Composition(_)
    ) {
        return Ok(false);
    }
    let [only] = children[..] else {
        return Ok(false);
    };
    if valuable_metadata(document, id) || document.try_get(only)?.item().is_none() {
        return Ok(false);
    }
    let top_level_track = is_track(document, id)
        && composition_parent(document, id).is_some_and(|parent| {
            is_stack(document, parent) && composition_parent(document, parent).is_none()
        });
    Ok(!top_level_track || is_track(document, only))
}

/// Replaces a redundant container with a copy of its only child, carrying
/// over what the container said about it.
fn lift_only_child(document: &mut Document, id: NodeId) -> Result<NodeId> {
    let only = document.children_of(id)?[0];
    let result = document.deep_clone(only)?;
    let (enabled, markers, effects, range) = match document.try_get(id)?.item() {
        Some(item) => (
            item.enabled,
            item.markers.clone(),
            item.effects.clone(),
            item.source_range,
        ),
        None => return Ok(id),
    };
    if let Some(item) = document.get_mut(result).and_then(Node::item_mut) {
        if !enabled {
            item.enabled = false;
        }
        item.markers.extend(markers);
        item.effects.extend(effects);
    }

    // Keep the container's length, if it has one.
    if let Some(range) = range {
        let combined = match document.trimmed_range(result) {
            Ok(own) => TimeRange::new(own.start_time() + range.start_time(), range.duration()),
            Err(OtioError::NoAvailableRange { .. }) => range,
            Err(error) => return Err(error.into()),
        };
        set_source_range(document, result, combined);
    }
    Ok(result)
}

/// Upstream's `_ensure_stack_tracks`: wraps each child of a stack that is
/// not a track in a track of its own.
fn ensure_stack_tracks(document: &mut Document, stack: NodeId) -> Result<()> {
    for (index, child) in document.children_of(stack)?.into_iter().enumerate() {
        if is_track(document, child) {
            continue;
        }
        document.remove_child(stack, index as i64)?;
        let kind = match aaf_metadata(document, child).and_then(|aaf| aaf.get("MediaKind")) {
            Some(Any::String(kind)) if !kind.is_empty() => track_kind(Some(kind)),
            _ => otio_core::TRACK_KIND_VIDEO.to_owned(),
        };
        let mut fields = item_fields();
        fields.base.name = document
            .try_get(child)?
            .base()
            .map(|base| base.name.clone())
            .unwrap_or_default();
        let track = document.insert(Node::Track(Track {
            item: fields,
            children: Vec::new(),
            kind,
        }));
        document.append_child(track, child)?;
        document.insert_child(stack, index as i64, track)?;
    }
    Ok(())
}

/// Upstream's `_track_item_can_flatten`.
fn track_item_can_flatten(document: &Document, id: NodeId) -> Result<bool> {
    if !is_track(document, id) || valuable_metadata(document, id) {
        return Ok(false);
    }
    if child_count(document, id) == 1 {
        return Ok(true);
    }
    Ok(!has_effects(document, id) && !has_transitions(document, id))
}

/// Upstream's `_stack_item_can_be_flatten`: a stack, or a track holding
/// only a stack, with no effects or metadata worth keeping on either.
fn stack_item_can_be_flattened(document: &Document, id: NodeId) -> bool {
    if has_effects(document, id) || valuable_metadata(document, id) {
        return false;
    }
    if is_stack(document, id) {
        return true;
    }
    if !is_track(document, id) {
        return false;
    }
    match document.get(id).and_then(Node::children) {
        Some(&[only]) => {
            is_stack(document, only)
                && !has_effects(document, only)
                && !valuable_metadata(document, only)
        }
        _ => false,
    }
}

/// Upstream's `_contains_something_valuable`: effects, markers, metadata
/// worth keeping, or anything other than gaps and empty compositions.
fn contains_something_valuable(document: &Document, id: NodeId) -> bool {
    let Some(node) = document.get(id) else {
        return false;
    };
    if let Some(item) = node.item() {
        if !item.effects.is_empty() || !item.markers.is_empty() {
            return true;
        }
    }
    if valuable_metadata(document, id) {
        return true;
    }
    match node {
        Node::Track(_) | Node::Stack(_) | Node::Composition(_) => node
            .children()
            .unwrap_or_default()
            .iter()
            .any(|&child| contains_something_valuable(document, child)),
        Node::Gap(_) => false,
        _ => true,
    }
}

/// Upstream's `_valuable_metadata`: a composition mob's user comments.
fn valuable_metadata(document: &Document, id: NodeId) -> bool {
    let Some(aaf) = aaf_metadata(document, id) else {
        return false;
    };
    matches!(aaf.get("ClassName"), Some(Any::String(name)) if name == "CompositionMob")
        && aaf.get("UserComments").is_some_and(truthy)
}

/// Whether an item has effects.
fn has_effects(document: &Document, id: NodeId) -> bool {
    document
        .get(id)
        .and_then(Node::item)
        .is_some_and(|item| !item.effects.is_empty())
}

/// Upstream's `_has_time_effect`: an effect whose AAF operation warps time.
fn has_time_effect(document: &Document, id: NodeId) -> bool {
    item_effects(document, id).into_iter().any(|effect| {
        let operation = aaf_metadata(document, effect).and_then(|aaf| aaf.get("Operation"));
        match operation {
            Some(Any::Dictionary(operation)) => operation.get("IsTimeWarp").is_some_and(truthy),
            _ => false,
        }
    })
}

/// Upstream's `_has_transitions`: a transition directly in the track, or in
/// any of the stack's tracks.
fn has_transitions(document: &Document, id: NodeId) -> bool {
    let tracks = if is_track(document, id) {
        vec![id]
    } else {
        document.children_of(id).unwrap_or_default()
    };
    tracks.into_iter().any(|track| {
        document
            .get(track)
            .and_then(Node::children)
            .unwrap_or_default()
            .iter()
            .any(|&child| matches!(document.get(child), Some(Node::Transition(_))))
    })
}

/// Upstream's `_trim_markers`: drops the markers that start outside the
/// item.
///
/// Upstream reads the item's own range, and fails on an item without one
/// that has markers; the range it would have is used here instead.
fn trim_markers(document: &mut Document, id: NodeId) -> Result<()> {
    let markers = match document.get(id).and_then(Node::item) {
        Some(item) if !item.markers.is_empty() => item.markers.clone(),
        _ => return Ok(()),
    };
    let range = match source_range(document, id) {
        Some(range) => range,
        None => document.trimmed_range(id)?,
    };
    let outside: HashSet<NodeId> = markers
        .into_iter()
        .filter(|&marker| match document.get(marker) {
            Some(Node::Marker(marker)) => !range.contains_time(marker.marked_range.start_time()),
            _ => false,
        })
        .collect();
    if let Some(item) = document.get_mut(id).and_then(Node::item_mut) {
        item.markers.retain(|marker| !outside.contains(marker));
    }
    Ok(())
}

/// Upstream's `_fix_time_effect_duration`: gives an item under a time
/// effect the duration of the track it came out of.
///
/// Upstream keeps the item's own start, and fails on an item without a
/// range of its own; the range it would have is used here instead.
fn fix_time_effect_duration(
    document: &mut Document,
    id: NodeId,
    range: Option<TimeRange>,
) -> Result<()> {
    let Some(range) = range else {
        return Ok(());
    };
    let start = match source_range(document, id) {
        Some(own) => own.start_time(),
        None => document.trimmed_range(id)?.start_time(),
    };
    set_source_range(document, id, TimeRange::new(start, range.duration()));
    Ok(())
}

/// Moves a flattened container's markers onto its parent, and disables the
/// parent if the container was disabled.
fn merge_markers_and_enabled(document: &mut Document, parent: NodeId, child: NodeId) {
    let (markers, enabled) = match document.get(child).and_then(Node::item) {
        Some(item) => (item.markers.clone(), item.enabled),
        None => return,
    };
    if let Some(item) = document.get_mut(parent).and_then(Node::item_mut) {
        item.markers.extend(markers);
        item.enabled = item.enabled && enabled;
    }
}

/// Python's `parent[at:at + 1] = replacements`.
fn splice(
    document: &mut Document,
    parent: NodeId,
    at: usize,
    replacements: &[NodeId],
) -> Result<()> {
    document.remove_child(parent, at as i64)?;
    for (offset, &child) in replacements.iter().enumerate() {
        document.insert_child(parent, (at + offset) as i64, child)?;
    }
    Ok(())
}

/// Python's `parent[at] = replacement`, which does nothing when the
/// replacement is already there.
fn replace_child(
    document: &mut Document,
    parent: NodeId,
    at: usize,
    replacement: NodeId,
) -> Result<()> {
    if document.children_of(parent)?[at] == replacement {
        return Ok(());
    }
    document.remove_child(parent, at as i64)?;
    document.insert_child(parent, at as i64, replacement)?;
    Ok(())
}

/// The composition an object sits in, as upstream's `parent()` reports it.
///
/// A collection holds objects without being their parent upstream.
fn composition_parent(document: &Document, id: NodeId) -> Option<NodeId> {
    let parent = document.get(id)?.parent()?;
    matches!(
        document.get(parent),
        Some(Node::Track(_) | Node::Stack(_) | Node::Composition(_))
    )
    .then_some(parent)
}

fn is_track(document: &Document, id: NodeId) -> bool {
    matches!(document.get(id), Some(Node::Track(_)))
}

fn is_stack(document: &Document, id: NodeId) -> bool {
    matches!(document.get(id), Some(Node::Stack(_)))
}

fn child_count(document: &Document, id: NodeId) -> usize {
    document
        .get(id)
        .and_then(Node::children)
        .map_or(0, <[NodeId]>::len)
}

fn source_range(document: &Document, id: NodeId) -> Option<TimeRange> {
    document.get(id).and_then(Node::item)?.source_range
}

fn set_source_range(document: &mut Document, id: NodeId, range: TimeRange) {
    if let Some(item) = document.get_mut(id).and_then(Node::item_mut) {
        item.source_range = Some(range);
    }
}

fn item_effects(document: &Document, id: NodeId) -> Vec<NodeId> {
    document
        .get(id)
        .and_then(Node::item)
        .map(|item| item.effects.clone())
        .unwrap_or_default()
}

fn aaf_metadata(document: &Document, id: NodeId) -> Option<&AnyDictionary> {
    match document.get(id)?.base()?.metadata.get("AAF") {
        Some(Any::Dictionary(aaf)) => Some(aaf),
        _ => None,
    }
}

/// Python's truth value of a metadata value.
fn truthy(value: &Any) -> bool {
    match value {
        Any::Null => false,
        Any::Bool(value) => *value,
        Any::Int(value) => *value != 0,
        Any::UInt(value) => *value != 0,
        Any::Double(value) => *value != 0.0,
        Any::String(value) => !value.is_empty(),
        Any::Vector(values) => !values.is_empty(),
        Any::Dictionary(values) => !values.is_empty(),
        _ => true,
    }
}

/// Drops every object the root no longer reaches.
///
/// Transcription leaves what it builds and then discards in the document's
/// arena, and flattening leaves the containers it empties and the copies it
/// trims from. Nothing reaches them, but the arena is public, so they are
/// removed rather than left for a caller to trip over.
pub(crate) fn retain_reachable(document: &mut Document, root: NodeId) {
    let mut reached = HashSet::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        if !reached.insert(id) {
            continue;
        }
        let Some(node) = document.get(id) else {
            continue;
        };
        if let Some(item) = node.item() {
            pending.extend(&item.effects);
            pending.extend(&item.markers);
        }
        if let Some(children) = node.children() {
            pending.extend(children);
        }
        match node {
            Node::Clip(clip) => pending.extend(clip.media_references.values()),
            Node::Timeline(timeline) => pending.extend(timeline.tracks),
            _ => {}
        }
        let mut held = node.clone();
        held.visit_held_objects_mut(&mut |id| pending.push(*id));
    }
    let unreached: Vec<NodeId> = document
        .iter()
        .map(|(id, _)| id)
        .filter(|id| !reached.contains(id))
        .collect();
    for id in unreached {
        document.remove(id);
    }
}

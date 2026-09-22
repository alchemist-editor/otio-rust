//! The object model for C: what kind an object is, and the fields it carries.
//!
//! Objects are never exposed as structs. A caller holds an [`OtioNode`] and
//! asks the document about it, which is what keeps the ABI stable while the
//! Rust types behind it change, and what keeps a C caller from holding a
//! pointer into an arena that the next edit may move.

use std::ffi::c_char;

use otio_core::Document;
use otio_core::schema::{
    Base, Clip, Composable, Composition, EffectData, ExternalReference, Gap, GeneratorReference,
    ImageSequenceReference, ItemData, Marker, MediaReferenceData, MissingFramePolicy,
    MissingReference, Node, SerializableCollection, Stack, Timeline, Track, Transition,
};

use crate::buffer::OtioBuffer;
use crate::handle::{
    OtioDocument, OtioNode, document, document_mut, optional_node, optional_text, text, write_out,
};
use crate::status::{Fault, OtioStatus, Outcome, guard};
use crate::time::{OtioRationalTime, OtioTimeRange};
use crate::value::{OtioBox2d, OtioColor};

/// What kind of object a handle names.
///
/// These follow the schemas of upstream OpenTimelineIO 0.19.0. An object whose
/// schema this library does not know reads as `OTIO_NODE_KIND_UNKNOWN_SCHEMA`
/// and keeps its fields verbatim.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioNodeKind {
    /// A bare `Item`.
    Item = 0,
    /// A `Clip`.
    Clip = 1,
    /// A `Gap`.
    Gap = 2,
    /// A `Track`.
    Track = 3,
    /// A `Stack`.
    Stack = 4,
    /// A `Timeline`.
    Timeline = 5,
    /// A `Transition`.
    Transition = 6,
    /// A `Marker`.
    Marker = 7,
    /// An `Effect`.
    Effect = 8,
    /// A `TimeEffect`.
    TimeEffect = 9,
    /// A `LinearTimeWarp`.
    LinearTimeWarp = 10,
    /// A `FreezeFrame`.
    FreezeFrame = 11,
    /// An `ExternalReference`.
    ExternalReference = 12,
    /// A `MissingReference`.
    MissingReference = 13,
    /// A `GeneratorReference`.
    GeneratorReference = 14,
    /// An `ImageSequenceReference`.
    ImageSequenceReference = 15,
    /// A `SerializableCollection`.
    SerializableCollection = 16,
    /// A `SerializableObject`.
    SerializableObject = 17,
    /// A `SerializableObjectWithMetadata`.
    SerializableObjectWithMetadata = 18,
    /// A `Composable`.
    Composable = 19,
    /// A bare `Composition`.
    Composition = 20,
    /// A bare `MediaReference`.
    MediaReference = 21,
    /// An object whose schema this library does not know.
    UnknownSchema = 22,
    /// A schema added to the core since this ABI was written.
    ///
    /// Nothing produces this today. It exists so that a core that grows a new
    /// object reports something honest to a caller built against this header
    /// rather than a kind that means something else.
    Other = 23,
}

/// What to show for a frame an image sequence is missing.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioMissingFramePolicy {
    /// Treat the gap as an error.
    Error = 0,
    /// Show black.
    Black = 1,
    /// Hold the previous frame.
    Hold = 2,
}

/// The numbers that say how an image sequence is laid out on disk.
///
/// The three parts of a frame's filename are strings, so they are read and
/// written by their own calls rather than sitting in here.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioImageSequence {
    /// The first frame's number.
    pub start_frame: i64,
    /// How much the frame number advances per frame.
    pub frame_step: i64,
    /// The sequence's frame rate.
    pub rate: f64,
    /// How many digits the frame number is padded to.
    pub frame_zero_padding: i64,
    /// What to do about a missing frame.
    pub missing_frame_policy: OtioMissingFramePolicy,
}

impl From<MissingFramePolicy> for OtioMissingFramePolicy {
    fn from(policy: MissingFramePolicy) -> Self {
        match policy {
            MissingFramePolicy::Error => Self::Error,
            MissingFramePolicy::Black => Self::Black,
            MissingFramePolicy::Hold => Self::Hold,
        }
    }
}

impl From<OtioMissingFramePolicy> for MissingFramePolicy {
    fn from(policy: OtioMissingFramePolicy) -> Self {
        match policy {
            OtioMissingFramePolicy::Error => Self::Error,
            OtioMissingFramePolicy::Black => Self::Black,
            OtioMissingFramePolicy::Hold => Self::Hold,
        }
    }
}

// ---------------------------------------------------------------------------
// Borrowing an object out of a document
// ---------------------------------------------------------------------------

pub(crate) fn node(source: &Document, id: OtioNode) -> Outcome<&Node> {
    Ok(source.try_get(id.to_id())?)
}

pub(crate) fn node_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut Node> {
    Ok(target.try_get_mut(id.to_id())?)
}

/// Reports that an object is not the kind the call needs.
fn wrong_kind(node: &Node, needs: &str) -> Fault {
    Fault::new(
        OtioStatus::CoreError,
        format!("a {} is not {needs}", node.schema_name()),
    )
}

fn base_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut Base> {
    let node = node_mut(target, id)?;
    // `base()` and `base_mut()` agree on which objects have one, so the
    // message is built from the same node before the mutable borrow starts.
    if node.base().is_none() {
        return Err(wrong_kind(node, "an object with a name"));
    }
    Ok(node.base_mut().expect("checked just above"))
}

fn item(source: &Document, id: OtioNode) -> Outcome<&ItemData> {
    let node = node(source, id)?;
    node.item().ok_or_else(|| wrong_kind(node, "an item"))
}

fn item_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut ItemData> {
    let node = node_mut(target, id)?;
    if node.item().is_none() {
        return Err(wrong_kind(node, "an item"));
    }
    Ok(node.item_mut().expect("checked just above"))
}

fn media(source: &Document, id: OtioNode) -> Outcome<&MediaReferenceData> {
    let node = node(source, id)?;
    node.media()
        .ok_or_else(|| wrong_kind(node, "a media reference"))
}

fn media_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut MediaReferenceData> {
    let node = node_mut(target, id)?;
    match node {
        Node::ExternalReference(reference) => Ok(&mut reference.media),
        Node::MissingReference(reference) => Ok(&mut reference.media),
        Node::GeneratorReference(reference) => Ok(&mut reference.media),
        Node::ImageSequenceReference(reference) => Ok(&mut reference.media),
        Node::MediaReference(data) => Ok(data),
        other => Err(wrong_kind(other, "a media reference")),
    }
}

fn effect(source: &Document, id: OtioNode) -> Outcome<&EffectData> {
    match node(source, id)? {
        Node::Effect(effect) | Node::TimeEffect(effect) => Ok(effect),
        Node::LinearTimeWarp { effect, .. } | Node::FreezeFrame { effect, .. } => Ok(effect),
        other => Err(wrong_kind(other, "an effect")),
    }
}

fn effect_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut EffectData> {
    match node_mut(target, id)? {
        Node::Effect(effect) | Node::TimeEffect(effect) => Ok(effect),
        Node::LinearTimeWarp { effect, .. } | Node::FreezeFrame { effect, .. } => Ok(effect),
        other => Err(wrong_kind(other, "an effect")),
    }
}

fn clip(source: &Document, id: OtioNode) -> Outcome<&Clip> {
    match node(source, id)? {
        Node::Clip(clip) => Ok(clip),
        other => Err(wrong_kind(other, "a clip")),
    }
}

fn clip_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut Clip> {
    match node_mut(target, id)? {
        Node::Clip(clip) => Ok(clip),
        other => Err(wrong_kind(other, "a clip")),
    }
}

fn timeline(source: &Document, id: OtioNode) -> Outcome<&Timeline> {
    match node(source, id)? {
        Node::Timeline(timeline) => Ok(timeline),
        other => Err(wrong_kind(other, "a timeline")),
    }
}

fn timeline_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut Timeline> {
    match node_mut(target, id)? {
        Node::Timeline(timeline) => Ok(timeline),
        other => Err(wrong_kind(other, "a timeline")),
    }
}

fn transition(source: &Document, id: OtioNode) -> Outcome<&Transition> {
    match node(source, id)? {
        Node::Transition(transition) => Ok(transition),
        other => Err(wrong_kind(other, "a transition")),
    }
}

fn transition_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut Transition> {
    match node_mut(target, id)? {
        Node::Transition(transition) => Ok(transition),
        other => Err(wrong_kind(other, "a transition")),
    }
}

fn marker(source: &Document, id: OtioNode) -> Outcome<&Marker> {
    match node(source, id)? {
        Node::Marker(marker) => Ok(marker),
        other => Err(wrong_kind(other, "a marker")),
    }
}

fn marker_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut Marker> {
    match node_mut(target, id)? {
        Node::Marker(marker) => Ok(marker),
        other => Err(wrong_kind(other, "a marker")),
    }
}

/// Adds an object to a document and reports its handle.
fn insert(target: *mut OtioDocument, out_node: *mut OtioNode, build: Node) -> Outcome<()> {
    let target = unsafe { document_mut(target) }?;
    let id = target.insert(build);
    unsafe { write_out(out_node, OtioNode::from_id(id), "out_node") }
}

/// Builds the name-and-metadata fields from a name that may be absent.
fn named(name: Option<&str>) -> Base {
    Base {
        name: name.unwrap_or_default().to_string(),
        ..Base::default()
    }
}

/// Builds item fields with upstream's defaults and the given name.
fn named_item(name: Option<&str>) -> ItemData {
    ItemData {
        base: named(name),
        ..ItemData::new()
    }
}

// ---------------------------------------------------------------------------
// Constructors
// ---------------------------------------------------------------------------

/// Creates a clip. `name` may be null for an unnamed one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::Clip(Clip {
                item: named_item(name),
                ..Clip::default()
            }),
        )
    })
}

/// Creates a gap.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_gap_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::Gap(Gap {
                item: named_item(name),
            }),
        )
    })
}

/// Creates a bare item: something that occupies time without saying what fills
/// it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(target, out_node, Node::Item(named_item(name)))
    })
}

/// Creates a track. `kind` may be null, which means `"Video"`, as upstream's
/// default does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_track_new(
    target: *mut OtioDocument,
    name: *const c_char,
    kind: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let kind = unsafe { optional_text(kind, "kind") }?.unwrap_or("Video");
        insert(
            target,
            out_node,
            Node::Track(Track {
                item: named_item(name),
                children: Vec::new(),
                kind: kind.to_string(),
            }),
        )
    })
}

/// Creates a stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_stack_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::Stack(Stack {
                item: named_item(name),
                children: Vec::new(),
            }),
        )
    })
}

/// Creates a bare composition: children with no layout of its own.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composition_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::Composition(Composition {
                item: named_item(name),
                children: Vec::new(),
            }),
        )
    })
}

/// Creates a composable: something that sits in a composition and nothing
/// more.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_composable_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::Composable(Composable {
                base: named(name),
                parent: None,
            }),
        )
    })
}

/// Creates a timeline, with an empty stack named `"tracks"` already in it.
///
/// Upstream's `Timeline()` builds that stack in its constructor, and its own
/// tests append to a fresh timeline's tracks without making one first, so a
/// timeline from here arrives the same way rather than leaving every binding
/// to invent the difference. Replace it with [`otio_timeline_set_tracks`] to
/// use a stack of your own; the one built here is thrown away with the
/// document.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_timeline_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let target = unsafe { document_mut(target) }?;
        let tracks = target.insert(Node::Stack(Stack {
            item: named_item(Some("tracks")),
            children: Vec::new(),
        }));
        let id = target.insert(Node::Timeline(Timeline {
            base: named(name),
            tracks: Some(tracks),
            global_start_time: None,
        }));
        // The stack's parent is the timeline, the way it would be had the
        // caller handed one over with `otio_timeline_set_tracks`.
        target.try_get_mut(tracks)?.set_parent(Some(id));
        unsafe { write_out(out_node, OtioNode::from_id(id), "out_node") }
    })
}

/// Creates a transition. Its offsets start at zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_new(
    target: *mut OtioDocument,
    name: *const c_char,
    transition_type: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let transition_type = unsafe { optional_text(transition_type, "transition_type") }?;
        insert(
            target,
            out_node,
            Node::Transition(Transition {
                base: named(name),
                transition_type: transition_type.unwrap_or_default().to_string(),
                enabled: true,
                ..Transition::default()
            }),
        )
    })
}

/// Creates a marker covering `marked_range`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_new(
    target: *mut OtioDocument,
    name: *const c_char,
    marked_range: OtioTimeRange,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::Marker(Marker {
                base: named(name),
                marked_range: marked_range.into(),
                ..Marker::default()
            }),
        )
    })
}

/// Creates an effect. `effect_name` is the effect's own name, such as
/// `"Blur"`, which is separate from the object's name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_new(
    target: *mut OtioDocument,
    name: *const c_char,
    effect_name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let effect_name = unsafe { optional_text(effect_name, "effect_name") }?;
        insert(
            target,
            out_node,
            Node::Effect(EffectData {
                base: named(name),
                effect_name: effect_name.unwrap_or_default().to_string(),
                enabled: true,
            }),
        )
    })
}

/// Creates a time effect: an effect that alters timing and has no parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_time_effect_new(
    target: *mut OtioDocument,
    name: *const c_char,
    effect_name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let effect_name = unsafe { optional_text(effect_name, "effect_name") }?;
        insert(
            target,
            out_node,
            Node::TimeEffect(EffectData {
                base: named(name),
                effect_name: effect_name.unwrap_or_default().to_string(),
                enabled: true,
            }),
        )
    })
}

/// Creates a constant-rate speed change. A `time_scalar` of 2.0 plays twice as
/// fast.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_linear_time_warp_new(
    target: *mut OtioDocument,
    name: *const c_char,
    time_scalar: f64,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::LinearTimeWarp {
                effect: EffectData {
                    base: named(name),
                    effect_name: "LinearTimeWarp".to_string(),
                    enabled: true,
                },
                time_scalar,
            },
        )
    })
}

/// Creates a freeze frame: a hold on a single frame.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_freeze_frame_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::FreezeFrame {
                effect: EffectData {
                    base: named(name),
                    effect_name: "FreezeFrame".to_string(),
                    enabled: true,
                },
                time_scalar: 0.0,
            },
        )
    })
}

/// Creates a media reference pointing at a URL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_external_reference_new(
    target: *mut OtioDocument,
    name: *const c_char,
    target_url: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let target_url = unsafe { optional_text(target_url, "target_url") }?;
        insert(
            target,
            out_node,
            Node::ExternalReference(ExternalReference {
                media: MediaReferenceData {
                    base: named(name),
                    ..MediaReferenceData::default()
                },
                target_url: target_url.unwrap_or_default().to_string(),
            }),
        )
    })
}

/// Creates a media reference for media known to exist somewhere unknown.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_missing_reference_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::MissingReference(MissingReference {
                media: MediaReferenceData {
                    base: named(name),
                    ..MediaReferenceData::default()
                },
            }),
        )
    })
}

/// Creates a media reference for generated media, such as colour bars.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_generator_reference_new(
    target: *mut OtioDocument,
    name: *const c_char,
    generator_kind: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        let generator_kind = unsafe { optional_text(generator_kind, "generator_kind") }?;
        insert(
            target,
            out_node,
            Node::GeneratorReference(GeneratorReference {
                media: MediaReferenceData {
                    base: named(name),
                    ..MediaReferenceData::default()
                },
                generator_kind: generator_kind.unwrap_or_default().to_string(),
                parameters: otio_core::AnyDictionary::new(),
            }),
        )
    })
}

/// Creates a media reference for a numbered sequence of image files.
///
/// The filename parts and the numbers start empty and at zero; set them with
/// [`otio_image_sequence_reference_set_numbers`] and the calls beside it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::ImageSequenceReference(ImageSequenceReference {
                media: MediaReferenceData {
                    base: named(name),
                    ..MediaReferenceData::default()
                },
                ..ImageSequenceReference::default()
            }),
        )
    })
}

/// Creates a serializable collection: a group of objects with no timing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_serializable_collection_new(
    target: *mut OtioDocument,
    name: *const c_char,
    out_node: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?;
        insert(
            target,
            out_node,
            Node::SerializableCollection(SerializableCollection {
                base: named(name),
                children: Vec::new(),
            }),
        )
    })
}

// ---------------------------------------------------------------------------
// Anything at all
// ---------------------------------------------------------------------------

/// Returns which kind an object is.
pub(crate) fn kind_of(node: &Node) -> OtioNodeKind {
    match node {
        Node::Item(_) => OtioNodeKind::Item,
        Node::Clip(_) => OtioNodeKind::Clip,
        Node::Gap(_) => OtioNodeKind::Gap,
        Node::Track(_) => OtioNodeKind::Track,
        Node::Stack(_) => OtioNodeKind::Stack,
        Node::Timeline(_) => OtioNodeKind::Timeline,
        Node::Transition(_) => OtioNodeKind::Transition,
        Node::Marker(_) => OtioNodeKind::Marker,
        Node::Effect(_) => OtioNodeKind::Effect,
        Node::TimeEffect(_) => OtioNodeKind::TimeEffect,
        Node::LinearTimeWarp { .. } => OtioNodeKind::LinearTimeWarp,
        Node::FreezeFrame { .. } => OtioNodeKind::FreezeFrame,
        Node::ExternalReference(_) => OtioNodeKind::ExternalReference,
        Node::MissingReference(_) => OtioNodeKind::MissingReference,
        Node::GeneratorReference(_) => OtioNodeKind::GeneratorReference,
        Node::ImageSequenceReference(_) => OtioNodeKind::ImageSequenceReference,
        Node::SerializableCollection(_) => OtioNodeKind::SerializableCollection,
        Node::SerializableObject => OtioNodeKind::SerializableObject,
        Node::SerializableObjectWithMetadata(_) => OtioNodeKind::SerializableObjectWithMetadata,
        Node::Composable(_) => OtioNodeKind::Composable,
        Node::Composition(_) => OtioNodeKind::Composition,
        Node::MediaReference(_) => OtioNodeKind::MediaReference,
        Node::Unknown(_) => OtioNodeKind::UnknownSchema,
        // `Node` is `#[non_exhaustive]`, so a core that gains a schema still
        // compiles against this crate. Saying so is better than guessing
        // which existing kind it resembles.
        _ => OtioNodeKind::Other,
    }
}

/// Returns what kind of object a handle names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_kind(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_kind: *mut OtioNodeKind,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let kind = kind_of(node(source, node_handle)?);
        unsafe { write_out(out_kind, kind, "out_kind") }
    })
}

/// Returns the schema name an object serializes as, such as `"Clip"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_schema_name(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_name: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let name = node(source, node_handle)?.schema_name();
        unsafe { write_out(out_name, OtioBuffer::from_str(name), "out_name") }
    })
}

/// Returns the schema version an object serializes as.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_schema_version(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_version: *mut u32,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let version = node(source, node_handle)?.schema_version();
        unsafe { write_out(out_version, version, "out_version") }
    })
}

/// Returns an object's name, which is empty for an object that has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_name(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_name: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let name = node(source, node_handle)?.name();
        unsafe { write_out(out_name, OtioBuffer::from_str(name), "out_name") }
    })
}

/// Sets an object's name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_set_name(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    name: *const c_char,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { text(name, "name") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        base_mut(target, node_handle)?.name = name;
        Ok(())
    })
}

/// Returns the composition an object sits in.
///
/// Reports `OTIO_STATUS_NO_VALUE` for an object that is in none, which is what
/// a freshly created one is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_parent(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_parent: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let parent = node(source, node_handle)?
            .parent()
            .ok_or_else(|| Fault::no_value("the parent"))?;
        unsafe { write_out(out_parent, OtioNode::from_id(parent), "out_parent") }
    })
}

/// Returns whether an object covers what is beneath it when its composition is
/// flattened.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_visible(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_visible: *mut bool,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let visible = node(source, node_handle)?.visible();
        unsafe { write_out(out_visible, visible, "out_visible") }
    })
}

/// Returns whether an object sits over its neighbours rather than beside them.
///
/// Only a transition does, which is why one does not advance the playhead when
/// a track is laid out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_node_overlapping(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_overlapping: *mut bool,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let overlapping = node(source, node_handle)?.overlapping();
        unsafe { write_out(out_overlapping, overlapping, "out_overlapping") }
    })
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// Returns the portion of its media an item uses.
///
/// Reports `OTIO_STATUS_NO_VALUE` for an item that takes all of it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_source_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let range = item(source, node_handle)?
            .source_range
            .ok_or_else(|| Fault::no_value("the source range"))?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Sets the portion of its media an item uses.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_set_source_range(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    range: OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        item_mut(target, node_handle)?.source_range = Some(range.into());
        Ok(())
    })
}

/// Clears an item's source range, so that it takes all of its media.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_clear_source_range(
    target: *mut OtioDocument,
    node_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        item_mut(target, node_handle)?.source_range = None;
        Ok(())
    })
}

/// Returns whether an item contributes to its composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_enabled(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_enabled: *mut bool,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let enabled = item(source, node_handle)?.enabled;
        unsafe { write_out(out_enabled, enabled, "out_enabled") }
    })
}

/// Sets whether an item contributes to its composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_set_enabled(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    enabled: bool,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        item_mut(target, node_handle)?.enabled = enabled;
        Ok(())
    })
}

/// Returns an item's display tint, and the name that goes with it.
///
/// `out_name` may be null if the name is not wanted. Reports
/// `OTIO_STATUS_NO_VALUE` for an untinted item.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_color(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_color: *mut OtioColor,
    out_name: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let color = item(source, node_handle)?
            .color
            .as_ref()
            .ok_or_else(|| Fault::no_value("the colour"))?;
        if !out_name.is_null() {
            unsafe { write_out(out_name, OtioBuffer::from_str(&color.name), "out_name") }?;
        }
        unsafe { write_out(out_color, OtioColor::from(color), "out_color") }
    })
}

/// Sets an item's display tint. `name` may be null for an unnamed colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_set_color(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    color: OtioColor,
    name: *const c_char,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?.unwrap_or_default();
        let target = unsafe { document_mut(target) }?;
        item_mut(target, node_handle)?.color = Some(color.to_color(name));
        Ok(())
    })
}

/// Clears an item's display tint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_clear_color(
    target: *mut OtioDocument,
    node_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        item_mut(target, node_handle)?.color = None;
        Ok(())
    })
}

/// Returns how many markers an item carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_marker_count(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_count: *mut usize,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let count = item(source, node_handle)?.markers.len();
        unsafe { write_out(out_count, count, "out_count") }
    })
}

/// Returns one of an item's markers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_marker_at(
    source: *const OtioDocument,
    node_handle: OtioNode,
    index: usize,
    out_marker: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let markers = &item(source, node_handle)?.markers;
        let marker = markers
            .get(index)
            .ok_or_else(|| Fault::invalid(format!("no marker at index {index}")))?;
        unsafe { write_out(out_marker, OtioNode::from_id(*marker), "out_marker") }
    })
}

/// Adds a marker to an item.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_append_marker(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    marker_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        // Check the marker before taking the mutable borrow of the item, so
        // that a bad handle fails without changing anything.
        marker(target, marker_handle)?;
        item_mut(target, node_handle)?
            .markers
            .push(marker_handle.to_id());
        Ok(())
    })
}

/// Removes one of an item's markers, and returns it.
///
/// The marker stays in the document; remove it with
/// `otio_document_remove_recursive`
/// if nothing else holds it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_remove_marker(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    index: usize,
    out_marker: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let markers = &mut item_mut(target, node_handle)?.markers;
        if index >= markers.len() {
            return Err(Fault::invalid(format!("no marker at index {index}")));
        }
        let removed = markers.remove(index);
        unsafe { write_out(out_marker, OtioNode::from_id(removed), "out_marker") }
    })
}

/// Returns how many effects an item carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_effect_count(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_count: *mut usize,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let count = item(source, node_handle)?.effects.len();
        unsafe { write_out(out_count, count, "out_count") }
    })
}

/// Returns one of an item's effects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_effect_at(
    source: *const OtioDocument,
    node_handle: OtioNode,
    index: usize,
    out_effect: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let effects = &item(source, node_handle)?.effects;
        let found = effects
            .get(index)
            .ok_or_else(|| Fault::invalid(format!("no effect at index {index}")))?;
        unsafe { write_out(out_effect, OtioNode::from_id(*found), "out_effect") }
    })
}

/// Adds an effect to an item. Effects apply in the order they are added.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_append_effect(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    effect_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        effect(target, effect_handle)?;
        item_mut(target, node_handle)?
            .effects
            .push(effect_handle.to_id());
        Ok(())
    })
}

/// Removes one of an item's effects, and returns it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_item_remove_effect(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    index: usize,
    out_effect: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let effects = &mut item_mut(target, node_handle)?.effects;
        if index >= effects.len() {
            return Err(Fault::invalid(format!("no effect at index {index}")));
        }
        let removed = effects.remove(index);
        unsafe { write_out(out_effect, OtioNode::from_id(removed), "out_effect") }
    })
}

// ---------------------------------------------------------------------------
// Clips
// ---------------------------------------------------------------------------

/// Returns which of a clip's media references is in use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_active_media_reference_key(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_key: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let key = &clip(source, node_handle)?.active_media_reference_key;
        unsafe { write_out(out_key, OtioBuffer::from_str(key), "out_key") }
    })
}

/// Sets which of a clip's media references is in use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_set_active_media_reference_key(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    key: *const c_char,
) -> OtioStatus {
    guard(|| {
        let key = unsafe { text(key, "key") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        clip_mut(target, node_handle)?.active_media_reference_key = key;
        Ok(())
    })
}

/// Returns how many media references a clip holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_media_reference_count(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_count: *mut usize,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let count = clip(source, node_handle)?.media_references.len();
        unsafe { write_out(out_count, count, "out_count") }
    })
}

/// Returns the key of one of a clip's media references.
///
/// The references are ordered by key, so walking the indices walks them in a
/// stable order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_media_reference_key_at(
    source: *const OtioDocument,
    node_handle: OtioNode,
    index: usize,
    out_key: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let key = clip(source, node_handle)?
            .media_references
            .keys()
            .nth(index)
            .ok_or_else(|| Fault::invalid(format!("no media reference at index {index}")))?;
        unsafe { write_out(out_key, OtioBuffer::from_str(key), "out_key") }
    })
}

/// Returns one of a clip's media references.
///
/// A null `key` means the active one. Reports `OTIO_STATUS_NO_VALUE` if the
/// key names nothing, which for the active key is a clip with no media.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_media_reference(
    source: *const OtioDocument,
    node_handle: OtioNode,
    key: *const c_char,
    out_reference: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let key = unsafe { optional_text(key, "key") }?;
        let source = unsafe { document(source) }?;
        let clip = clip(source, node_handle)?;
        let key = key.unwrap_or(clip.active_media_reference_key.as_str());
        let reference = clip
            .media_references
            .get(key)
            .ok_or_else(|| Fault::no_value(&format!("media reference '{key}'")))?;
        unsafe {
            write_out(
                out_reference,
                OtioNode::from_id(*reference),
                "out_reference",
            )
        }
    })
}

/// Sets one of a clip's media references, adding it if the key is new.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_set_media_reference(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    key: *const c_char,
    reference: OtioNode,
) -> OtioStatus {
    guard(|| {
        let key = unsafe { text(key, "key") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        media(target, reference)?;
        clip_mut(target, node_handle)?
            .media_references
            .insert(key, reference.to_id());
        Ok(())
    })
}

/// Removes one of a clip's media references, and returns it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_clip_remove_media_reference(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    key: *const c_char,
    out_reference: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let key = unsafe { text(key, "key") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        let removed = clip_mut(target, node_handle)?
            .media_references
            .remove(&key)
            .ok_or_else(|| Fault::no_value(&format!("media reference '{key}'")))?;
        unsafe { write_out(out_reference, OtioNode::from_id(removed), "out_reference") }
    })
}

// ---------------------------------------------------------------------------
// Tracks, timelines, transitions and markers
// ---------------------------------------------------------------------------

/// Returns what a track carries, such as `"Video"` or `"Audio"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_track_kind(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_kind: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        match node(source, node_handle)? {
            Node::Track(track) => unsafe {
                write_out(out_kind, OtioBuffer::from_str(&track.kind), "out_kind")
            },
            other => Err(wrong_kind(other, "a track")),
        }
    })
}

/// Sets what a track carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_track_set_kind(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    kind: *const c_char,
) -> OtioStatus {
    guard(|| {
        let kind = unsafe { text(kind, "kind") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        match node_mut(target, node_handle)? {
            Node::Track(track) => {
                track.kind = kind;
                Ok(())
            }
            other => Err(wrong_kind(other, "a track")),
        }
    })
}

/// Returns the stack holding a timeline's tracks.
///
/// Reports `OTIO_STATUS_NO_VALUE` for a timeline that has none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_timeline_tracks(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_tracks: *mut OtioNode,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let tracks = timeline(source, node_handle)?
            .tracks
            .ok_or_else(|| Fault::no_value("the tracks stack"))?;
        unsafe { write_out(out_tracks, OtioNode::from_id(tracks), "out_tracks") }
    })
}

/// Sets the stack holding a timeline's tracks.
///
/// The stack's parent is set to the timeline, as upstream's does.
///
/// Passing `otio_node_none` puts a fresh empty stack there rather than
/// nothing, because that is what upstream's setter does: its own
/// `test_timeline.py` sets `tracks` to `None` and then asserts that
/// `tl.tracks` is still a `Stack`. A timeline with no tracks at all is not a
/// thing a caller can reach through upstream's API, so it is not one they can
/// reach through this one.
///
/// Whatever stack was there is not destroyed. It stays in the document,
/// parentless, so it can be put somewhere else; dropping it is a separate
/// [`otio_document_remove`] call. That is the same bargain
/// [`otio_composition_detach_child`] makes, and leaving its parent pointing at
/// the timeline instead would mean an object claiming a parent that has
/// disowned it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_timeline_set_tracks(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    tracks: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let displaced = timeline(target, node_handle)?.tracks;
        let stack = match optional_node(tracks) {
            Some(stack) => {
                match node(target, tracks)? {
                    Node::Stack(_) => {}
                    other => return Err(wrong_kind(other, "a stack")),
                }
                stack
            }
            None => target.insert(Node::Stack(Stack {
                item: named_item(Some("tracks")),
                children: Vec::new(),
            })),
        };
        if let Some(displaced) = displaced.filter(|displaced| *displaced != stack) {
            target.try_get_mut(displaced)?.set_parent(None);
        }
        target
            .try_get_mut(stack)?
            .set_parent(Some(node_handle.to_id()));
        timeline_mut(target, node_handle)?.tracks = Some(stack);
        Ok(())
    })
}

/// Returns where a timeline begins, such as `01:00:00:00`.
///
/// Reports `OTIO_STATUS_NO_VALUE` for a timeline that does not say.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_timeline_global_start_time(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_time: *mut OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let time = timeline(source, node_handle)?
            .global_start_time
            .ok_or_else(|| Fault::no_value("the global start time"))?;
        unsafe { write_out(out_time, time.into(), "out_time") }
    })
}

/// Sets where a timeline begins.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_timeline_set_global_start_time(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    time: OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        timeline_mut(target, node_handle)?.global_start_time = Some(time.into());
        Ok(())
    })
}

/// Clears where a timeline begins.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_timeline_clear_global_start_time(
    target: *mut OtioDocument,
    node_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        timeline_mut(target, node_handle)?.global_start_time = None;
        Ok(())
    })
}

/// Returns how far a transition reaches into the item before it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_in_offset(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_offset: *mut OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let offset = transition(source, node_handle)?.in_offset;
        unsafe { write_out(out_offset, offset.into(), "out_offset") }
    })
}

/// Sets how far a transition reaches into the item before it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_set_in_offset(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    offset: OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        transition_mut(target, node_handle)?.in_offset = offset.into();
        Ok(())
    })
}

/// Returns how far a transition reaches into the item after it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_out_offset(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_offset: *mut OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let offset = transition(source, node_handle)?.out_offset;
        unsafe { write_out(out_offset, offset.into(), "out_offset") }
    })
}

/// Sets how far a transition reaches into the item after it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_set_out_offset(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    offset: OtioRationalTime,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        transition_mut(target, node_handle)?.out_offset = offset.into();
        Ok(())
    })
}

/// Returns the kind of transition, such as `"SMPTE_Dissolve"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_type(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_type: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let value = &transition(source, node_handle)?.transition_type;
        unsafe { write_out(out_type, OtioBuffer::from_str(value), "out_type") }
    })
}

/// Sets the kind of transition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_set_type(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    transition_type: *const c_char,
) -> OtioStatus {
    guard(|| {
        let value = unsafe { text(transition_type, "transition_type") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        transition_mut(target, node_handle)?.transition_type = value;
        Ok(())
    })
}

/// Returns whether a transition is applied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_enabled(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_enabled: *mut bool,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let enabled = transition(source, node_handle)?.enabled;
        unsafe { write_out(out_enabled, enabled, "out_enabled") }
    })
}

/// Sets whether a transition is applied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_transition_set_enabled(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    enabled: bool,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        transition_mut(target, node_handle)?.enabled = enabled;
        Ok(())
    })
}

/// Returns the span a marker covers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_marked_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let range = marker(source, node_handle)?.marked_range;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Sets the span a marker covers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_set_marked_range(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    range: OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        marker_mut(target, node_handle)?.marked_range = range.into();
        Ok(())
    })
}

/// Returns a marker's note.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_comment(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_comment: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let comment = &marker(source, node_handle)?.comment;
        unsafe { write_out(out_comment, OtioBuffer::from_str(comment), "out_comment") }
    })
}

/// Sets a marker's note.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_set_comment(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    comment: *const c_char,
) -> OtioStatus {
    guard(|| {
        let comment = unsafe { text(comment, "comment") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        marker_mut(target, node_handle)?.comment = comment;
        Ok(())
    })
}

/// Returns a marker's tint, and the name that goes with it.
///
/// `out_name` may be null if the name is not wanted. Reports
/// `OTIO_STATUS_NO_VALUE` for an untinted marker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_color(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_color: *mut OtioColor,
    out_name: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let color = marker(source, node_handle)?
            .color
            .as_ref()
            .ok_or_else(|| Fault::no_value("the colour"))?;
        if !out_name.is_null() {
            unsafe { write_out(out_name, OtioBuffer::from_str(&color.name), "out_name") }?;
        }
        unsafe { write_out(out_color, OtioColor::from(color), "out_color") }
    })
}

/// Sets a marker's tint. `name` may be null for an unnamed colour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_marker_set_color(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    color: OtioColor,
    name: *const c_char,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { optional_text(name, "name") }?.unwrap_or_default();
        let target = unsafe { document_mut(target) }?;
        marker_mut(target, node_handle)?.color = Some(color.to_color(name));
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

/// Returns an effect's own name, such as `"LinearTimeWarp"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_effect_name(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_name: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let name = &effect(source, node_handle)?.effect_name;
        unsafe { write_out(out_name, OtioBuffer::from_str(name), "out_name") }
    })
}

/// Sets an effect's own name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_set_effect_name(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    effect_name: *const c_char,
) -> OtioStatus {
    guard(|| {
        let name = unsafe { text(effect_name, "effect_name") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        effect_mut(target, node_handle)?.effect_name = name;
        Ok(())
    })
}

/// Returns whether an effect is applied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_enabled(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_enabled: *mut bool,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let enabled = effect(source, node_handle)?.enabled;
        unsafe { write_out(out_enabled, enabled, "out_enabled") }
    })
}

/// Sets whether an effect is applied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_set_enabled(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    enabled: bool,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        effect_mut(target, node_handle)?.enabled = enabled;
        Ok(())
    })
}

/// Returns a speed change's multiplier.
///
/// Only a `LinearTimeWarp` and a `FreezeFrame` have one; anything else reports
/// `OTIO_STATUS_CORE_ERROR`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_time_scalar(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_scalar: *mut f64,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let scalar = match node(source, node_handle)? {
            Node::LinearTimeWarp { time_scalar, .. } | Node::FreezeFrame { time_scalar, .. } => {
                *time_scalar
            }
            other => return Err(wrong_kind(other, "an effect with a time scalar")),
        };
        unsafe { write_out(out_scalar, scalar, "out_scalar") }
    })
}

/// Sets a speed change's multiplier.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_effect_set_time_scalar(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    scalar: f64,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        match node_mut(target, node_handle)? {
            Node::LinearTimeWarp { time_scalar, .. } | Node::FreezeFrame { time_scalar, .. } => {
                *time_scalar = scalar;
                Ok(())
            }
            other => Err(wrong_kind(other, "an effect with a time scalar")),
        }
    })
}

// ---------------------------------------------------------------------------
// Media references
// ---------------------------------------------------------------------------

/// Returns the span of media a reference says is available.
///
/// Reports `OTIO_STATUS_NO_VALUE` for a reference that does not say.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_media_reference_available_range(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_range: *mut OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let range = media(source, node_handle)?
            .available_range
            .ok_or_else(|| Fault::no_value("the available range"))?;
        unsafe { write_out(out_range, range.into(), "out_range") }
    })
}

/// Sets the span of media a reference says is available.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_media_reference_set_available_range(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    range: OtioTimeRange,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        media_mut(target, node_handle)?.available_range = Some(range.into());
        Ok(())
    })
}

/// Clears the span of media a reference says is available.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_media_reference_clear_available_range(
    target: *mut OtioDocument,
    node_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        media_mut(target, node_handle)?.available_range = None;
        Ok(())
    })
}

/// Returns the image bounds a reference says its media has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_media_reference_available_image_bounds(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_bounds: *mut OtioBox2d,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let bounds = media(source, node_handle)?
            .available_image_bounds
            .ok_or_else(|| Fault::no_value("the available image bounds"))?;
        unsafe { write_out(out_bounds, bounds.into(), "out_bounds") }
    })
}

/// Sets the image bounds a reference says its media has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_media_reference_set_available_image_bounds(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    bounds: OtioBox2d,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        media_mut(target, node_handle)?.available_image_bounds = Some(bounds.into());
        Ok(())
    })
}

/// Clears the image bounds a reference says its media has.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_media_reference_clear_available_image_bounds(
    target: *mut OtioDocument,
    node_handle: OtioNode,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        media_mut(target, node_handle)?.available_image_bounds = None;
        Ok(())
    })
}

/// Returns where an external reference's media lives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_external_reference_target_url(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_url: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        match node(source, node_handle)? {
            Node::ExternalReference(reference) => unsafe {
                write_out(
                    out_url,
                    OtioBuffer::from_str(&reference.target_url),
                    "out_url",
                )
            },
            other => Err(wrong_kind(other, "an external reference")),
        }
    })
}

/// Sets where an external reference's media lives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_external_reference_set_target_url(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    url: *const c_char,
) -> OtioStatus {
    guard(|| {
        let url = unsafe { text(url, "url") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        match node_mut(target, node_handle)? {
            Node::ExternalReference(reference) => {
                reference.target_url = url;
                Ok(())
            }
            other => Err(wrong_kind(other, "an external reference")),
        }
    })
}

/// Returns which generator a generator reference names, such as
/// `"SMPTEBars"`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_generator_reference_kind(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_kind: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        match node(source, node_handle)? {
            Node::GeneratorReference(reference) => unsafe {
                write_out(
                    out_kind,
                    OtioBuffer::from_str(&reference.generator_kind),
                    "out_kind",
                )
            },
            other => Err(wrong_kind(other, "a generator reference")),
        }
    })
}

/// Sets which generator a generator reference names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_generator_reference_set_kind(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    kind: *const c_char,
) -> OtioStatus {
    guard(|| {
        let kind = unsafe { text(kind, "kind") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        match node_mut(target, node_handle)? {
            Node::GeneratorReference(reference) => {
                reference.generator_kind = kind;
                Ok(())
            }
            other => Err(wrong_kind(other, "a generator reference")),
        }
    })
}

fn image_sequence(source: &Document, id: OtioNode) -> Outcome<&ImageSequenceReference> {
    match node(source, id)? {
        Node::ImageSequenceReference(reference) => Ok(reference),
        other => Err(wrong_kind(other, "an image sequence reference")),
    }
}

fn image_sequence_mut(target: &mut Document, id: OtioNode) -> Outcome<&mut ImageSequenceReference> {
    match node_mut(target, id)? {
        Node::ImageSequenceReference(reference) => Ok(reference),
        other => Err(wrong_kind(other, "an image sequence reference")),
    }
}

/// Returns the numbers describing how an image sequence is laid out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_numbers(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_numbers: *mut OtioImageSequence,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let reference = image_sequence(source, node_handle)?;
        let numbers = OtioImageSequence {
            start_frame: reference.start_frame,
            frame_step: reference.frame_step,
            rate: reference.rate,
            frame_zero_padding: reference.frame_zero_padding,
            missing_frame_policy: reference.missing_frame_policy.into(),
        };
        unsafe { write_out(out_numbers, numbers, "out_numbers") }
    })
}

/// Sets the numbers describing how an image sequence is laid out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_set_numbers(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    numbers: OtioImageSequence,
) -> OtioStatus {
    guard(|| {
        let target = unsafe { document_mut(target) }?;
        let reference = image_sequence_mut(target, node_handle)?;
        reference.start_frame = numbers.start_frame;
        reference.frame_step = numbers.frame_step;
        reference.rate = numbers.rate;
        reference.frame_zero_padding = numbers.frame_zero_padding;
        reference.missing_frame_policy = numbers.missing_frame_policy.into();
        Ok(())
    })
}

/// Returns the directory an image sequence's frames sit in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_target_url_base(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_url_base: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let value = &image_sequence(source, node_handle)?.target_url_base;
        unsafe { write_out(out_url_base, OtioBuffer::from_str(value), "out_url_base") }
    })
}

/// Sets the directory an image sequence's frames sit in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_set_target_url_base(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    url_base: *const c_char,
) -> OtioStatus {
    guard(|| {
        let value = unsafe { text(url_base, "url_base") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        image_sequence_mut(target, node_handle)?.target_url_base = value;
        Ok(())
    })
}

/// Returns the part of each frame's filename before the frame number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_name_prefix(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_prefix: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let value = &image_sequence(source, node_handle)?.name_prefix;
        unsafe { write_out(out_prefix, OtioBuffer::from_str(value), "out_prefix") }
    })
}

/// Sets the part of each frame's filename before the frame number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_set_name_prefix(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    prefix: *const c_char,
) -> OtioStatus {
    guard(|| {
        let value = unsafe { text(prefix, "prefix") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        image_sequence_mut(target, node_handle)?.name_prefix = value;
        Ok(())
    })
}

/// Returns the part of each frame's filename after the frame number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_name_suffix(
    source: *const OtioDocument,
    node_handle: OtioNode,
    out_suffix: *mut OtioBuffer,
) -> OtioStatus {
    guard(|| {
        let source = unsafe { document(source) }?;
        let value = &image_sequence(source, node_handle)?.name_suffix;
        unsafe { write_out(out_suffix, OtioBuffer::from_str(value), "out_suffix") }
    })
}

/// Sets the part of each frame's filename after the frame number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_image_sequence_reference_set_name_suffix(
    target: *mut OtioDocument,
    node_handle: OtioNode,
    suffix: *const c_char,
) -> OtioStatus {
    guard(|| {
        let value = unsafe { text(suffix, "suffix") }?.to_string();
        let target = unsafe { document_mut(target) }?;
        image_sequence_mut(target, node_handle)?.name_suffix = value;
        Ok(())
    })
}

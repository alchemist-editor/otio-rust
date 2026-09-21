//! The OTIO object model.
//!
//! Every object is a variant of [`Node`], and every object lives in a
//! [`Document`](crate::Document) arena. Fields that in upstream's C++ are
//! owning pointers are [`NodeId`] handles here.
//!
//! The inheritance upstream expresses with C++ base classes is expressed here
//! by embedding: an item's shared fields live in [`ItemData`], which itself
//! embeds [`Base`]. Accessors on [`Node`] reach through that nesting so
//! callers rarely have to.

use std::collections::BTreeMap;

use opentime::{RationalTime, TimeRange};

use crate::arena::NodeId;
use crate::value::{AnyDictionary, Box2d, Color};

/// Fields shared by every object that has a name and metadata.
///
/// Upstream calls this `SerializableObjectWithMetadata`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Base {
    /// The object's name. May be empty.
    pub name: String,
    /// Free-form metadata, preserved verbatim across a round trip.
    pub metadata: AnyDictionary,
}

/// Fields shared by everything that can sit in a composition and occupy time.
///
/// Upstream splits these across `Composable` and `Item`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemData {
    /// Name and metadata.
    pub base: Base,
    /// The composition holding this item, if any.
    ///
    /// Not serialized: it is rebuilt from the nesting when a document is read.
    pub parent: Option<NodeId>,
    /// The portion of the item's media that this item uses.
    pub source_range: Option<TimeRange>,
    /// Effects applied to this item, in order.
    pub effects: Vec<NodeId>,
    /// Markers on this item.
    pub markers: Vec<NodeId>,
    /// Whether the item contributes to its composition.
    pub enabled: bool,
    /// A display tint for editorial tools.
    pub color: Option<Color>,
}

impl ItemData {
    /// Constructs item fields with upstream's defaults: enabled, untinted, and
    /// with no source range.
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// Fields shared by every media reference.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaReferenceData {
    /// Name and metadata.
    pub base: Base,
    /// The span of media available, if known.
    pub available_range: Option<TimeRange>,
    /// The image bounds of the media, if known.
    pub available_image_bounds: Option<Box2d>,
}

/// Fields shared by every effect.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EffectData {
    /// Name and metadata.
    pub base: Base,
    /// The name of the effect, such as `"LinearTimeWarp"`.
    pub effect_name: String,
    /// Whether the effect is applied.
    pub enabled: bool,
}

impl EffectData {
    /// Constructs effect fields that are enabled, as upstream defaults to.
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// A piece of media used for a span of time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Clip {
    /// Item fields.
    pub item: ItemData,
    /// The media this clip can draw from, keyed by name.
    pub media_references: BTreeMap<String, NodeId>,
    /// Which entry of `media_references` is in use.
    pub active_media_reference_key: String,
}

/// An empty span of time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gap {
    /// Item fields.
    pub item: ItemData,
}

/// A sequence of items laid end to end.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    /// Item fields.
    pub item: ItemData,
    /// The items on this track, in order.
    pub children: Vec<NodeId>,
    /// What the track carries, such as `"Video"` or `"Audio"`.
    pub kind: String,
}

/// A set of items layered over the same span of time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Stack {
    /// Item fields.
    pub item: ItemData,
    /// The layers, bottom first.
    pub children: Vec<NodeId>,
}

/// A whole edit: a stack of tracks with a start time.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Timeline {
    /// Name and metadata.
    pub base: Base,
    /// The stack holding the timeline's tracks.
    pub tracks: Option<NodeId>,
    /// Where the timeline begins, such as `01:00:00:00`.
    pub global_start_time: Option<RationalTime>,
}

/// A dissolve or wipe between two neighbouring items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transition {
    /// Name and metadata.
    pub base: Base,
    /// The composition holding this transition, if any. Not serialized.
    pub parent: Option<NodeId>,
    /// How far the transition reaches into the preceding item.
    pub in_offset: RationalTime,
    /// How far the transition reaches into the following item.
    pub out_offset: RationalTime,
    /// The kind of transition, such as `"SMPTE_Dissolve"`.
    pub transition_type: String,
    /// Whether the transition is applied.
    pub enabled: bool,
}

/// A labelled point or span on an item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marker {
    /// Name and metadata.
    pub base: Base,
    /// A display tint.
    pub color: Option<Color>,
    /// The span the marker covers.
    pub marked_range: TimeRange,
    /// A free-form note.
    pub comment: String,
}

/// Media stored at a URL.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExternalReference {
    /// Media reference fields.
    pub media: MediaReferenceData,
    /// Where the media lives.
    pub target_url: String,
}

/// Media that is known to exist but whose location is not.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MissingReference {
    /// Media reference fields.
    pub media: MediaReferenceData,
}

/// Media produced by a generator, such as colour bars or a slug.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeneratorReference {
    /// Media reference fields.
    pub media: MediaReferenceData,
    /// Which generator, such as `"SMPTEBars"`.
    pub generator_kind: String,
    /// Generator-specific settings.
    pub parameters: AnyDictionary,
}

/// What to show when a frame of an image sequence is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingFramePolicy {
    /// Treat the gap as an error.
    #[default]
    Error,
    /// Show black.
    Black,
    /// Hold the previous frame.
    Hold,
}

impl MissingFramePolicy {
    /// Returns the policy's name as it appears in JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Black => "black",
            Self::Hold => "hold",
        }
    }

    /// Parses a policy from its name in JSON.
    ///
    /// Not `FromStr`: an unrecognised name is not an error here. Upstream
    /// falls back to the default rather than refusing the file.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "error" => Some(Self::Error),
            "black" => Some(Self::Black),
            "hold" => Some(Self::Hold),
            _ => None,
        }
    }
}

/// Media stored as a numbered sequence of image files.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageSequenceReference {
    /// Media reference fields.
    pub media: MediaReferenceData,
    /// The directory holding the frames.
    pub target_url_base: String,
    /// The part of each filename before the frame number.
    pub name_prefix: String,
    /// The part of each filename after the frame number.
    pub name_suffix: String,
    /// The first frame's number.
    pub start_frame: i64,
    /// How much the frame number advances per frame.
    pub frame_step: i64,
    /// The sequence's frame rate.
    pub rate: f64,
    /// How many digits the frame number is padded to.
    pub frame_zero_padding: i64,
    /// What to do about missing frames.
    pub missing_frame_policy: MissingFramePolicy,
}

/// An arbitrary group of objects, with no timing of its own.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SerializableCollection {
    /// Name and metadata.
    pub base: Base,
    /// The collected objects.
    pub children: Vec<NodeId>,
}

/// Something that can sit in a composition, with no timing of its own.
///
/// Upstream's `Composable` is the base class `Item` and `Transition` derive
/// from, and it is registered as a schema, so a file may carry one. Its only
/// serialized fields are its name and metadata; the parent is a link the
/// composition sets, and is rebuilt from the nesting when a file is read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composable {
    /// Name and metadata.
    pub base: Base,
    /// The composition holding this object, if any. Not serialized.
    pub parent: Option<NodeId>,
}

/// A composition with no layout of its own.
///
/// Upstream's `Composition` is the base class `Track` and `Stack` derive
/// from, and it is registered as a schema, so a file may carry one. It
/// serializes as an item plus its children, exactly as its subclasses do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composition {
    /// Timing, name and metadata.
    pub item: ItemData,
    /// The children it holds.
    pub children: Vec<NodeId>,
}

/// An object whose schema this library does not know.
///
/// Its contents are kept verbatim so that reading and rewriting a file
/// produced by a third-party plugin does not discard the plugin's data. This
/// is what makes the format extensible in practice.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnknownSchema {
    /// The schema name as it appeared in the file.
    pub original_schema_name: String,
    /// The schema version as it appeared in the file.
    pub original_schema_version: u32,
    /// Every field of the object, untouched.
    pub data: AnyDictionary,
}

/// Any object in a document.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Node {
    /// A bare item: something that occupies time but says nothing about what
    /// fills it.
    ///
    /// Upstream registers `Item` as a schema in its own right, and its `fill`
    /// algorithm builds one when fitting a clip into a gap with a time warp,
    /// so this is not only a base class.
    Item(ItemData),
    /// A clip.
    Clip(Clip),
    /// A gap.
    Gap(Gap),
    /// A track.
    Track(Track),
    /// A stack.
    Stack(Stack),
    /// A timeline.
    Timeline(Timeline),
    /// A transition.
    Transition(Transition),
    /// A marker.
    Marker(Marker),
    /// A plain effect.
    Effect(EffectData),
    /// An effect that alters timing but has no parameters of its own.
    TimeEffect(EffectData),
    /// A constant-rate speed change.
    LinearTimeWarp {
        /// Effect fields.
        effect: EffectData,
        /// The speed multiplier: 2.0 plays twice as fast.
        time_scalar: f64,
    },
    /// A hold on a single frame.
    FreezeFrame {
        /// Effect fields.
        effect: EffectData,
        /// Always zero for a freeze frame.
        time_scalar: f64,
    },
    /// Media at a URL.
    ExternalReference(ExternalReference),
    /// Media whose location is unknown.
    MissingReference(MissingReference),
    /// Generated media.
    GeneratorReference(GeneratorReference),
    /// A numbered image sequence.
    ImageSequenceReference(ImageSequenceReference),
    /// A group of objects.
    SerializableCollection(SerializableCollection),
    /// An object with no fields at all.
    ///
    /// This and the four variants below it are upstream's base classes.
    /// Upstream registers each as a schema in its own right, and its Python
    /// API lets a caller build one directly — its own `test_composable.py`
    /// starts by constructing a bare `Composable` — so they are objects a
    /// file can legitimately contain, not only rungs on an inheritance
    /// ladder.
    SerializableObject,
    /// An object carrying only a name and metadata.
    SerializableObjectWithMetadata(Base),
    /// Something that can sit in a composition, carrying only a name and
    /// metadata of its own.
    Composable(Composable),
    /// A bare composition: an item holding children, with no layout of its
    /// own.
    ///
    /// A `Track` lays its children end to end and a `Stack` starts them
    /// together; this says neither, so it is read and written but cannot be
    /// asked where its children sit.
    Composition(Composition),
    /// A bare media reference: somewhere media might be, without saying
    /// where.
    MediaReference(MediaReferenceData),
    /// An object of an unrecognized schema, preserved verbatim.
    Unknown(UnknownSchema),
}

impl Node {
    /// Returns the schema name this object serializes as.
    ///
    /// For an unknown schema, this is the name it was read with.
    #[must_use]
    pub fn schema_name(&self) -> &str {
        match self {
            Self::Item(_) => "Item",
            Self::Clip(_) => "Clip",
            Self::Gap(_) => "Gap",
            Self::Track(_) => "Track",
            Self::Stack(_) => "Stack",
            Self::Timeline(_) => "Timeline",
            Self::Transition(_) => "Transition",
            Self::Marker(_) => "Marker",
            Self::Effect(_) => "Effect",
            Self::TimeEffect(_) => "TimeEffect",
            Self::LinearTimeWarp { .. } => "LinearTimeWarp",
            Self::FreezeFrame { .. } => "FreezeFrame",
            Self::ExternalReference(_) => "ExternalReference",
            Self::MissingReference(_) => "MissingReference",
            Self::GeneratorReference(_) => "GeneratorReference",
            Self::ImageSequenceReference(_) => "ImageSequenceReference",
            Self::SerializableCollection(_) => "SerializableCollection",
            Self::SerializableObject => "SerializableObject",
            Self::SerializableObjectWithMetadata(_) => "SerializableObjectWithMetadata",
            Self::Composable(_) => "Composable",
            Self::Composition(_) => "Composition",
            Self::MediaReference(_) => "MediaReference",
            Self::Unknown(unknown) => &unknown.original_schema_name,
        }
    }

    /// Returns the schema version this object serializes as.
    ///
    /// These match upstream OpenTimelineIO 0.19.0. For an unknown schema, this
    /// is the version it was read with.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        match self {
            Self::Clip(_) => 2,
            Self::Marker(_) => 3,
            Self::Unknown(unknown) => unknown.original_schema_version,
            _ => 1,
        }
    }

    /// Borrows the object's name and metadata, if it has them.
    ///
    /// An unknown schema does not, since its fields are held verbatim.
    #[must_use]
    pub const fn base(&self) -> Option<&Base> {
        match self {
            Self::Item(item) => Some(&item.base),
            Self::Clip(clip) => Some(&clip.item.base),
            Self::Gap(gap) => Some(&gap.item.base),
            Self::Track(track) => Some(&track.item.base),
            Self::Stack(stack) => Some(&stack.item.base),
            Self::Timeline(timeline) => Some(&timeline.base),
            Self::Transition(transition) => Some(&transition.base),
            Self::Marker(marker) => Some(&marker.base),
            Self::Effect(effect) | Self::TimeEffect(effect) => Some(&effect.base),
            Self::LinearTimeWarp { effect, .. } | Self::FreezeFrame { effect, .. } => {
                Some(&effect.base)
            }
            Self::ExternalReference(reference) => Some(&reference.media.base),
            Self::MissingReference(reference) => Some(&reference.media.base),
            Self::GeneratorReference(reference) => Some(&reference.media.base),
            Self::ImageSequenceReference(reference) => Some(&reference.media.base),
            Self::SerializableCollection(collection) => Some(&collection.base),
            Self::SerializableObjectWithMetadata(base) => Some(base),
            Self::Composable(composable) => Some(&composable.base),
            Self::Composition(composition) => Some(&composition.item.base),
            Self::MediaReference(media) => Some(&media.base),
            Self::SerializableObject | Self::Unknown(_) => None,
        }
    }

    /// Returns the object's name, or the empty string if it has none.
    #[must_use]
    pub fn name(&self) -> &str {
        self.base().map_or("", |base| base.name.as_str())
    }

    /// Borrows the item fields, if this object is an item.
    ///
    /// Timelines, markers, effects and media references are not items.
    #[must_use]
    pub const fn item(&self) -> Option<&ItemData> {
        match self {
            Self::Item(item) => Some(item),
            Self::Clip(clip) => Some(&clip.item),
            Self::Gap(gap) => Some(&gap.item),
            Self::Track(track) => Some(&track.item),
            Self::Stack(stack) => Some(&stack.item),
            Self::Composition(composition) => Some(&composition.item),
            _ => None,
        }
    }

    /// Borrows the object's name and metadata mutably, if it has them.
    ///
    /// As [`Node::base`]: an unknown schema has neither, since its fields are
    /// held verbatim.
    pub const fn base_mut(&mut self) -> Option<&mut Base> {
        match self {
            Self::Item(item) => Some(&mut item.base),
            Self::Clip(clip) => Some(&mut clip.item.base),
            Self::Gap(gap) => Some(&mut gap.item.base),
            Self::Track(track) => Some(&mut track.item.base),
            Self::Stack(stack) => Some(&mut stack.item.base),
            Self::Timeline(timeline) => Some(&mut timeline.base),
            Self::Transition(transition) => Some(&mut transition.base),
            Self::Marker(marker) => Some(&mut marker.base),
            Self::Effect(effect) | Self::TimeEffect(effect) => Some(&mut effect.base),
            Self::LinearTimeWarp { effect, .. } | Self::FreezeFrame { effect, .. } => {
                Some(&mut effect.base)
            }
            Self::ExternalReference(reference) => Some(&mut reference.media.base),
            Self::MissingReference(reference) => Some(&mut reference.media.base),
            Self::GeneratorReference(reference) => Some(&mut reference.media.base),
            Self::ImageSequenceReference(reference) => Some(&mut reference.media.base),
            Self::SerializableCollection(collection) => Some(&mut collection.base),
            Self::SerializableObjectWithMetadata(base) => Some(base),
            Self::Composable(composable) => Some(&mut composable.base),
            Self::Composition(composition) => Some(&mut composition.item.base),
            Self::MediaReference(media) => Some(&mut media.base),
            Self::SerializableObject | Self::Unknown(_) => None,
        }
    }

    /// Borrows the item fields mutably, if this object is an item.
    pub const fn item_mut(&mut self) -> Option<&mut ItemData> {
        match self {
            Self::Item(item) => Some(item),
            Self::Clip(clip) => Some(&mut clip.item),
            Self::Gap(gap) => Some(&mut gap.item),
            Self::Track(track) => Some(&mut track.item),
            Self::Stack(stack) => Some(&mut stack.item),
            Self::Composition(composition) => Some(&mut composition.item),
            _ => None,
        }
    }

    /// Borrows the effect fields, if this object is an effect.
    ///
    /// The time warps are effects too, so a caller asking what an effect is
    /// called does not have to know which kind it is holding.
    #[must_use]
    pub const fn effect(&self) -> Option<&EffectData> {
        match self {
            Self::Effect(effect) | Self::TimeEffect(effect) => Some(effect),
            Self::LinearTimeWarp { effect, .. } | Self::FreezeFrame { effect, .. } => Some(effect),
            _ => None,
        }
    }

    /// Borrows the effect fields mutably, if this object is an effect.
    pub const fn effect_mut(&mut self) -> Option<&mut EffectData> {
        match self {
            Self::Effect(effect) | Self::TimeEffect(effect) => Some(effect),
            Self::LinearTimeWarp { effect, .. } | Self::FreezeFrame { effect, .. } => Some(effect),
            _ => None,
        }
    }

    /// Borrows the media reference fields, if this object is a media
    /// reference.
    #[must_use]
    pub const fn media(&self) -> Option<&MediaReferenceData> {
        match self {
            Self::ExternalReference(reference) => Some(&reference.media),
            Self::MissingReference(reference) => Some(&reference.media),
            Self::GeneratorReference(reference) => Some(&reference.media),
            Self::ImageSequenceReference(reference) => Some(&reference.media),
            Self::MediaReference(media) => Some(media),
            _ => None,
        }
    }

    /// Returns this object's children, if it holds any.
    ///
    /// Tracks, stacks and serializable collections do.
    #[must_use]
    pub fn children(&self) -> Option<&[NodeId]> {
        match self {
            Self::Track(track) => Some(&track.children),
            Self::Stack(stack) => Some(&stack.children),
            Self::SerializableCollection(collection) => Some(&collection.children),
            Self::Composition(composition) => Some(&composition.children),
            _ => None,
        }
    }

    /// Returns whether this object covers what is beneath it when its
    /// composition is flattened.
    ///
    /// A gap is deliberately not visible: that is what lets a lower track show
    /// through. A disabled item is not visible either. Everything else is.
    #[must_use]
    pub const fn visible(&self) -> bool {
        match self {
            Self::Gap(_) => false,
            Self::Item(item) => item.enabled,
            Self::Clip(Clip { item, .. })
            | Self::Track(Track { item, .. })
            | Self::Stack(Stack { item, .. })
            | Self::Composition(Composition { item, .. }) => item.enabled,
            _ => true,
        }
    }

    /// Returns whether this object sits over its neighbours rather than
    /// beside them.
    ///
    /// Only a transition does, and it is why a transition does not advance the
    /// playhead when a track's children are laid out.
    #[must_use]
    pub const fn overlapping(&self) -> bool {
        matches!(self, Self::Transition(_))
    }

    /// Runs `f` on every handle this object holds, its parent included.
    ///
    /// This is the one place that knows where a `NodeId` can hide, so anything
    /// that has to rewrite handles wholesale — moving a subtree into another
    /// document, for instance — goes through it rather than re-deriving the
    /// list and missing one. Metadata counts: it may hold whole objects.
    pub fn visit_links_mut(&mut self, f: &mut impl FnMut(&mut NodeId)) {
        if let Some(base) = self.base_mut() {
            for value in base.metadata.values_mut() {
                value.visit_objects_mut(f);
            }
        }
        if let Self::GeneratorReference(reference) = self {
            for value in reference.parameters.values_mut() {
                value.visit_objects_mut(f);
            }
        }

        if let Some(item) = self.item_mut() {
            if let Some(parent) = item.parent.as_mut() {
                f(parent);
            }
            for effect in &mut item.effects {
                f(effect);
            }
            for marker in &mut item.markers {
                f(marker);
            }
        }

        match self {
            Self::Transition(transition) => {
                if let Some(parent) = transition.parent.as_mut() {
                    f(parent);
                }
            }
            Self::Composable(composable) => {
                if let Some(parent) = composable.parent.as_mut() {
                    f(parent);
                }
            }
            Self::Clip(clip) => {
                for reference in clip.media_references.values_mut() {
                    f(reference);
                }
            }
            Self::Timeline(timeline) => {
                if let Some(tracks) = timeline.tracks.as_mut() {
                    f(tracks);
                }
            }
            _ => {}
        }

        match self {
            Self::Track(track) => {
                for child in &mut track.children {
                    f(child);
                }
            }
            Self::Stack(stack) => {
                for child in &mut stack.children {
                    f(child);
                }
            }
            Self::Composition(composition) => {
                for child in &mut composition.children {
                    f(child);
                }
            }
            Self::SerializableCollection(collection) => {
                for child in &mut collection.children {
                    f(child);
                }
            }
            _ => {}
        }
    }

    /// Returns the composition this object sits in, if it has one.
    #[must_use]
    pub const fn parent(&self) -> Option<NodeId> {
        match self {
            Self::Item(item) => item.parent,
            Self::Clip(clip) => clip.item.parent,
            Self::Gap(gap) => gap.item.parent,
            Self::Track(track) => track.item.parent,
            Self::Stack(stack) => stack.item.parent,
            Self::Transition(transition) => transition.parent,
            Self::Composable(composable) => composable.parent,
            Self::Composition(composition) => composition.item.parent,
            _ => None,
        }
    }

    /// Sets the composition this object sits in.
    ///
    /// Has no effect on objects that cannot sit in one.
    pub const fn set_parent(&mut self, parent: Option<NodeId>) {
        match self {
            Self::Item(item) => item.parent = parent,
            Self::Clip(clip) => clip.item.parent = parent,
            Self::Gap(gap) => gap.item.parent = parent,
            Self::Track(track) => track.item.parent = parent,
            Self::Stack(stack) => stack.item.parent = parent,
            Self::Transition(transition) => transition.parent = parent,
            Self::Composable(composable) => composable.parent = parent,
            Self::Composition(composition) => composition.item.parent = parent,
            _ => {}
        }
    }
}

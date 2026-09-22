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

use opentime::{RationalTime, TimeRange, TimeTransform};

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
    /// What the object carries beyond its built-in schema: the schema of a
    /// subclass it is an instance of, and that subclass's fields.
    ///
    /// `None` for almost every object. See [`Extension`].
    pub extension: Option<Box<Extension>>,
}

impl Base {
    /// Name and metadata, with no extension.
    #[must_use]
    pub const fn new(name: String, metadata: AnyDictionary) -> Self {
        Self {
            name,
            metadata,
            extension: None,
        }
    }

    /// Borrows the fields an extension holds, if there is one.
    #[must_use]
    pub fn extension_fields(&self) -> Option<&AnyDictionary> {
        self.extension.as_deref().map(|extension| &extension.fields)
    }

    /// Borrows the extension's fields mutably, giving the object an empty
    /// extension first if it has none.
    pub fn extension_fields_mut(&mut self) -> &mut AnyDictionary {
        &mut self.extension.get_or_insert_with(Box::default).fields
    }
}

/// What a built-in object carries when it is an instance of a schema
/// derived from its own, or holds fields its schema does not have.
///
/// Upstream lets a program subclass a concrete class such as `Clip` and
/// register the subclass as a schema of its own (its Python API does it with
/// `register_type`). Its C++ holds such an object as an ordinary `Clip`
/// whose type record names the subclass, and whose extra fields sit in the
/// "dynamic fields" every object has. This is the same thing: the object
/// stays the built-in variant of [`Node`], so compositions, algorithms,
/// adapters and bindings all treat it as the built-in it is, and this
/// records the name and version it is written under and the fields it
/// carries beyond the built-in's.
///
/// A program that has not registered the subclass reads its objects as an
/// [`UnknownSchema`], as upstream does. See
/// [`register_subclass`](crate::registry::register_subclass).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extension {
    /// The schema name and version the object serializes under in place of
    /// its built-in schema's, or `None` to keep the built-in's.
    pub schema: Option<(String, u32)>,
    /// Fields beyond the built-in schema's own, by name. Upstream calls
    /// these dynamic fields; they are written before any of the built-in's.
    pub fields: AnyDictionary,
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

/// The `kind` of a track carrying picture.
///
/// Upstream keeps these two strings as `Track.Kind.Video` and
/// `Track.Kind.Audio`; they are the values a file actually holds, so anything
/// that has to tell one track from another compares against them.
pub const TRACK_KIND_VIDEO: &str = "Video";

/// The `kind` of a track carrying sound.
pub const TRACK_KIND_AUDIO: &str = "Audio";

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
    /// Returns `None` for a name this library does not know. Upstream refuses
    /// the whole file in that case rather than guessing, and so does the
    /// deserializer here.
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

impl ImageSequenceReference {
    /// How long one image of the sequence is shown for.
    #[must_use]
    pub fn frame_duration(&self) -> RationalTime {
        RationalTime::new(self.frame_step as f64, self.rate)
    }

    /// The last frame number in the sequence.
    ///
    /// A sequence with no available range is one frame long, so its last
    /// frame is its first.
    #[must_use]
    pub fn end_frame(&self) -> i64 {
        let Some(range) = self.media.available_range else {
            return self.start_frame;
        };
        // One is taken off because the range of frame numbers is inclusive.
        self.start_frame + i64::from(range.duration().to_frames_at_rate(self.rate)) - 1
    }

    /// How many images the sequence holds.
    #[must_use]
    pub fn number_of_images_in_sequence(&self) -> i64 {
        let Some(range) = self.media.available_range else {
            return 0;
        };
        // Every `frame_step`th frame has an image, so the images arrive at a
        // slower rate than the frames do.
        let playback_rate = self.rate / self.frame_step as f64;
        i64::from(range.duration().to_frames_at_rate(playback_rate))
    }

    /// The frame number shown at `time`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidTimeRange`](crate::Error::InvalidTimeRange) if the sequence has no available
    /// range, or `time` falls outside it.
    pub fn frame_for_time(&self, time: RationalTime) -> crate::Result<i64> {
        let range = self
            .media
            .available_range
            .filter(|range| range.contains_time(time))
            .ok_or(crate::Error::InvalidTimeRange)?;
        let offset = (time - range.start_time()).to_frames_at_rate(self.rate);
        Ok(self.start_frame + i64::from(offset))
    }

    /// The URL of the `image_number`th image, counting from zero.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IllegalIndex`](crate::Error::IllegalIndex) if the sequence has no images at all,
    /// or `image_number` is past its last one.
    pub fn target_url_for_image_number(&self, image_number: i64) -> crate::Result<String> {
        // A sequence with no rate or no duration holds no images at all.
        // Upstream reports each of those separately, and its own tests
        // compare the wording, so both messages are kept as it has them.
        if self.rate == 0.0 {
            return Err(crate::Error::NoImagesInSequence {
                reason: "Zero rate sequence has no frames.",
            });
        }
        if self
            .media
            .available_range
            .is_none_or(|range| range.duration().value() == 0.0)
        {
            return Err(crate::Error::NoImagesInSequence {
                reason: "Zero duration sequences has no frames.",
            });
        }
        let count = self.number_of_images_in_sequence();
        if image_number >= count {
            return Err(crate::Error::IllegalIndex {
                index: image_number,
                len: usize::try_from(count).unwrap_or(0),
            });
        }

        let frame = self.start_frame + image_number * self.frame_step;
        let digits = frame.unsigned_abs().to_string();
        let padding = usize::try_from(self.frame_zero_padding)
            .unwrap_or(0)
            .saturating_sub(digits.len());
        let sign = if frame < 0 { "-" } else { "" };
        // A base that does not already end in a slash gets one, so that the
        // prefix does not run into the directory name.
        let separator = if self.target_url_base.is_empty() || self.target_url_base.ends_with('/') {
            ""
        } else {
            "/"
        };
        Ok(format!(
            "{}{separator}{}{sign}{}{digits}{}",
            self.target_url_base,
            self.name_prefix,
            "0".repeat(padding),
            self.name_suffix
        ))
    }

    /// When the `image_number`th image is shown, counting from zero.
    ///
    /// # Errors
    ///
    /// Returns [`Error::IllegalIndex`](crate::Error::IllegalIndex) if `image_number` is past the
    /// sequence's last image.
    pub fn presentation_time_for_image_number(
        &self,
        image_number: i64,
    ) -> crate::Result<RationalTime> {
        let count = self.number_of_images_in_sequence();
        if image_number >= count {
            return Err(crate::Error::IllegalIndex {
                index: image_number,
                len: usize::try_from(count).unwrap_or(0),
            });
        }
        let start = self
            .media
            .available_range
            .ok_or(crate::Error::InvalidTimeRange)?
            .start_time();
        let transform = TimeTransform::new(start, image_number as f64, -1.0);
        Ok(transform.applied_to_time(self.frame_duration()))
    }
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

/// An object of a schema defined at run time rather than built in.
///
/// Upstream lets a program define schemas of its own — its Python API does it
/// with `register_type` — and in its C++ such an object is a plain
/// `SerializableObject` (or `SerializableObjectWithMetadata`) whose data sits
/// in a dictionary of "dynamic fields" under a schema name and version it was
/// given. This is the same thing: a name, a version, optionally the name and
/// metadata every `SerializableObjectWithMetadata` has, and a field map. See
/// [`crate::registry`] for how a schema comes to be read as one.
///
/// Nothing about it is tied to the language that defined the schema: a
/// program that did not register it reads the same object as an
/// [`UnknownSchema`], and writes it back unchanged.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DynamicObject {
    /// The schema name it serializes under.
    pub schema_name: String,
    /// The schema version it serializes under.
    pub schema_version: u32,
    /// Its name and metadata, when its schema derives from upstream's
    /// `SerializableObjectWithMetadata`; `None` when it derives from bare
    /// `SerializableObject`.
    pub base: Option<Base>,
    /// Every other field, by name. Upstream calls these dynamic fields.
    pub fields: AnyDictionary,
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
    /// An object of a schema registered at run time; see [`DynamicObject`].
    Dynamic(DynamicObject),
}

impl Node {
    /// Returns the schema name this object serializes as.
    ///
    /// For an unknown schema, this is the name it was read with; for an
    /// instance of a subclass, the subclass's name (see [`Extension`]).
    #[must_use]
    pub fn schema_name(&self) -> &str {
        match self.subclass_schema() {
            Some((name, _)) => name,
            None => self.built_in_schema_name(),
        }
    }

    /// Returns the schema name of the variant itself, ignoring any subclass
    /// the object is an instance of.
    ///
    /// That is the built-in schema that decides how the object behaves. For
    /// an unknown schema and one registered at run time, which have no
    /// variant of their own, it is the same as [`Node::schema_name`].
    #[must_use]
    pub fn built_in_schema_name(&self) -> &str {
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
            Self::Dynamic(dynamic) => &dynamic.schema_name,
        }
    }

    /// Returns the schema version this object serializes as.
    ///
    /// These match upstream OpenTimelineIO 0.19.0. For an unknown schema, this
    /// is the version it was read with; for an instance of a subclass, the
    /// subclass's version.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        match self.subclass_schema() {
            Some((_, version)) => *version,
            None => self.built_in_schema_version(),
        }
    }

    /// The schema a built-in object is an instance of a subclass of, if it
    /// is one: the name and version its [`Extension`] records.
    ///
    /// A dynamic object's name and version are its own already.
    #[must_use]
    pub fn subclass_schema(&self) -> Option<&(String, u32)> {
        match self {
            Self::Dynamic(_) | Self::Unknown(_) => None,
            _ => self.base()?.extension.as_deref()?.schema.as_ref(),
        }
    }

    /// Returns the schema version of the variant itself, ignoring any
    /// subclass the object is an instance of.
    #[must_use]
    pub const fn built_in_schema_version(&self) -> u32 {
        match self {
            Self::Clip(_) => 2,
            Self::Marker(_) => 3,
            Self::Unknown(unknown) => unknown.original_schema_version,
            Self::Dynamic(dynamic) => dynamic.schema_version,
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
            Self::Dynamic(dynamic) => dynamic.base.as_ref(),
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
            Self::Dynamic(dynamic) => dynamic.base.as_mut(),
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

    /// Borrows the media reference fields mutably, if this object is a media
    /// reference.
    pub const fn media_mut(&mut self) -> Option<&mut MediaReferenceData> {
        match self {
            Self::ExternalReference(reference) => Some(&mut reference.media),
            Self::MissingReference(reference) => Some(&mut reference.media),
            Self::GeneratorReference(reference) => Some(&mut reference.media),
            Self::ImageSequenceReference(reference) => Some(&mut reference.media),
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
        self.visit_held_objects_mut(f);

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

    /// Runs `f` on every object this one owns.
    ///
    /// That is [`Node::visit_links_mut`] without the parent link: children,
    /// a timeline's stack, an item's effects and markers, a clip's media
    /// references, and whatever metadata or a generator's parameters hold.
    pub fn visit_owned(&self, f: &mut impl FnMut(NodeId)) {
        if let Some(base) = self.base() {
            for value in base.metadata.values() {
                value.visit_objects(f);
            }
            for value in base
                .extension_fields()
                .into_iter()
                .flat_map(|fields| fields.values())
            {
                value.visit_objects(f);
            }
        }
        if let Self::GeneratorReference(reference) = self {
            for value in reference.parameters.values() {
                value.visit_objects(f);
            }
        }
        if let Some(item) = self.item() {
            item.effects.iter().copied().for_each(&mut *f);
            item.markers.iter().copied().for_each(&mut *f);
        }
        match self {
            Self::Clip(clip) => clip.media_references.values().copied().for_each(&mut *f),
            Self::Timeline(timeline) => timeline.tracks.into_iter().for_each(&mut *f),
            _ => {}
        }
        if let Some(children) = self.children() {
            children.iter().copied().for_each(f);
        }
    }

    /// Runs `f` on every handle this object holds in a free-form dictionary.
    ///
    /// That is its metadata, a subclass's extension fields, a generator
    /// reference's parameters, and the fields of a run-time or unknown
    /// schema: all may hold whole objects. Unlike the handles in
    /// [`Node::visit_links_mut`], these are owned rather than referred to, so
    /// a deep copy has to copy what they point at.
    pub fn visit_held_objects_mut(&mut self, f: &mut impl FnMut(&mut NodeId)) {
        if let Some(base) = self.base_mut() {
            for value in base.metadata.values_mut() {
                value.visit_objects_mut(f);
            }
            if let Some(extension) = base.extension.as_deref_mut() {
                for value in extension.fields.values_mut() {
                    value.visit_objects_mut(f);
                }
            }
        }
        let held = match self {
            Self::GeneratorReference(reference) => Some(&mut reference.parameters),
            // A run-time schema's fields and an unknown schema's data are
            // free-form too, and just as able to hold whole objects.
            Self::Dynamic(dynamic) => Some(&mut dynamic.fields),
            Self::Unknown(unknown) => Some(&mut unknown.data),
            _ => None,
        };
        if let Some(held) = held {
            for value in held.values_mut() {
                value.visit_objects_mut(f);
            }
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

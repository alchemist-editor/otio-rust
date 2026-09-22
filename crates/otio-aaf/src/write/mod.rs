//! Writing a timeline as an AAF file: a port of upstream's `aaf_writer.py`.
//!
//! Upstream's adapter writes through pyaaf2, and this writes through the
//! `aaf` crate's port of pyaaf2's write path, which produces the bytes
//! pyaaf2 produces for the same operations in the same order. So this module
//! is written to make the same operations in the same order as upstream's
//! writer: each function here is one of upstream's, named in its
//! documentation, and does what that function does, step for step. Where
//! upstream's code would fail on an input, this returns an error rather
//! than picking a behaviour of its own.
//!
//! The pieces are:
//!
//! - this module: `write_to_file`'s outline, `_stackify_nested_groups`,
//!   `validate_metadata`, `_gather_clip_mob_ids`, and `AAFFileTranscriber`,
//!   which owns the file and the mobs shared between tracks;
//! - [`track`]: the `_TrackTranscriber` classes, which turn each item on a
//!   track into AAF components and the source mobs behind them;
//! - [`descriptor`]: the essence descriptors a file mob carries;
//! - [`markers`]: a track's markers, as an event slot of descriptive
//!   markers;
//! - [`py`]: the Python semantics all of that leans on.
//!
//! Upstream's pre- and post-write hooks are a way to run Python plugins with
//! the pyaaf2 file in hand. There is no such plugin mechanism here, so they
//! are not run; everything else `write_to_file` does is.

mod descriptor;
mod markers;
mod py;
mod track;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use aaf::write::{AafWriter, Clock, IdSource, ObjRef, Rational, Timestamp};
use aaf::{Auid, MobId};
use opentime::RationalTime;
use otio_core::{Any, Document, Node, NodeId};

use self::py::{aaf_metadata, first_truthy};
use crate::adapter::WriteOptions;
use crate::error::Error;

/// Where a file's times and identifiers come from.
///
/// pyaaf2 reads the clock and draws a random UUID as it builds a file: when
/// the file is made, for each new mob, and, in upstream's writer, for each
/// marker it dates. With the same sources a timeline is written to the same
/// bytes every time, which is what the tests rely on and what a caller that
/// wants reproducible files can rely on too.
///
/// The sources are shared, not copied: options cloned from these, and every
/// file written with them, draw from the one clock and the one sequence of
/// identifiers, so no two files written with a [`SequentialIds`] get the
/// same identifiers.
///
/// [`SequentialIds`]: aaf::write::SequentialIds
#[derive(Clone)]
pub struct Sources {
    clock: Arc<Mutex<dyn Clock + Send>>,
    ids: Arc<Mutex<dyn IdSource + Send>>,
}

impl Sources {
    /// Sources that read `clock` and draw from `ids`.
    ///
    /// ```
    /// use aaf::write::{SequentialIds, SteppingClock, Timestamp};
    ///
    /// let start = Timestamp::parse_iso("2024-05-06T07:08:09").unwrap();
    /// let sources = otio_aaf::Sources::new(SteppingClock::new(start), SequentialIds::new(0));
    /// ```
    pub fn new(clock: impl Clock + Send + 'static, ids: impl IdSource + Send + 'static) -> Self {
        Self {
            clock: Arc::new(Mutex::new(clock)),
            ids: Arc::new(Mutex::new(ids)),
        }
    }
}

impl std::fmt::Debug for Sources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources").finish_non_exhaustive()
    }
}

/// A [`Sources`] clock, as the writer takes one.
struct SharedClock(Arc<Mutex<dyn Clock + Send>>);

impl Clock for SharedClock {
    fn now(&mut self) -> Timestamp {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).now()
    }
}

/// A [`Sources`] identifier source, as the writer takes one.
struct SharedIds(Arc<Mutex<dyn IdSource + Send>>);

impl IdSource for SharedIds {
    fn uuid4(&mut self) -> Auid {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .uuid4()
    }
}

/// Why a write stopped, before it becomes an [`Error`].
///
/// The writer underneath fails with `aaf::Error` and the timeline with
/// `otio_core::Error`, and both have to become this crate's error the
/// writing way: an `aaf::Error` here is [`Error::Write`], where on reading
/// it is [`Error::Aaf`]. Collecting them here lets `?` do that.
pub(crate) enum Fail {
    Aaf(aaf::Error),
    Otio(otio_core::Error),
    Other(Error),
}

impl From<aaf::Error> for Fail {
    fn from(error: aaf::Error) -> Self {
        Self::Aaf(error)
    }
}

impl From<otio_core::Error> for Fail {
    fn from(error: otio_core::Error) -> Self {
        Self::Otio(error)
    }
}

impl From<Fail> for Error {
    fn from(fail: Fail) -> Self {
        match fail {
            Fail::Aaf(error) => Self::Write(error),
            Fail::Otio(error) => Self::Otio(error),
            Fail::Other(error) => error,
        }
    }
}

/// Upstream would have failed here too.
pub(crate) fn unwritable(what: String) -> Fail {
    Fail::Other(Error::Unwritable(what))
}

/// Upstream raises `NotSupportedError` here.
pub(crate) fn unsupported(what: String) -> Fail {
    Fail::Other(Error::Unsupported(what))
}

/// A rate, as pyaaf2 stores a Python float: `AAFRational(rate)`.
pub(crate) fn rational(rate: f64) -> Result<Rational, Fail> {
    Rational::from_f64(rate).ok_or_else(|| unwritable(format!("{rate} is not a rate")))
}

/// Upstream's `write_to_file`, without the hooks: writes `document`, which
/// must hold a timeline, as an AAF.
pub(crate) fn write(document: &Document, options: &WriteOptions) -> crate::Result<Vec<u8>> {
    write_timeline(document, options).map_err(Error::from)
}

fn write_timeline(document: &Document, options: &WriteOptions) -> Result<Vec<u8>, Fail> {
    if options.embed_essence {
        // Embedding means importing DNxHD and WAV essence, or copying it out
        // of another AAF, which needs media decoding this crate does not do.
        return Err(unsupported(
            "embedding essence is not implemented; write with embed_essence off".to_owned(),
        ));
    }

    // `aaf2.open(filepath, "w")`, which reads the clock and draws the file's
    // identifier before anything else happens.
    let mut aaf_options = aaf::write::WriteOptions::default();
    if let Some(sources) = &options.sources {
        aaf_options.clock = Box::new(SharedClock(Arc::clone(&sources.clock)));
        aaf_options.ids = Box::new(SharedIds(Arc::clone(&sources.ids)));
    }
    if let Some(platform) = &options.platform {
        aaf_options.platform.clone_from(platform);
    }
    let f = AafWriter::with_options(aaf_options)?;

    let input = document
        .root()
        .ok_or_else(|| unsupported("Currently only supporting top level Timeline".to_owned()))?;
    let Node::Timeline(input_timeline) = document.try_get(input)? else {
        return Err(unsupported(
            "Currently only supporting top level Timeline".to_owned(),
        ));
    };
    let global_start_time = input_timeline.global_start_time;

    // Upstream deep-copies the timeline before reshaping it, and so does
    // this, by copying the document it is in.
    let mut copy = document.clone();
    stackify_nested_groups(&mut copy, input)?;
    validate_metadata(&copy, input)?;

    let mut file = FileTranscriber::new(&copy, input, f, options)?;

    let mut default_edit_rate: Option<f64> = None;
    for track in tracks_of(&copy, input)? {
        // A track must have something on it to give it an edit rate.
        if copy.children_of(track)?.is_empty() {
            continue;
        }
        let t = file.track_transcriber(track)?;
        if default_edit_rate.is_none_or(|rate| rate == 0.0) {
            default_edit_rate = Some(t.edit_rate);
        }
        for child in copy.children_of(track)? {
            if let Some(result) = file.transcribe(&t, child)? {
                file.f.append(t.sequence, "Components", result)?;
            }
        }
        file.transcribe_aaf_descriptive_markers(&t)?;
    }

    // Always add a timecode track to the main composition mob. Upstream says
    // DaVinci Resolve needs one.
    if default_edit_rate.is_some_and(|rate| rate != 0.0) || global_start_time.is_some() {
        file.add_timecode(global_start_time, default_edit_rate)?;
    }

    Ok(file.f.finish()?)
}

/// The tracks of a timeline: the children of its stack.
fn tracks_of(document: &Document, timeline: NodeId) -> Result<Vec<NodeId>, Fail> {
    match document.try_get(timeline)? {
        Node::Timeline(t) => match t.tracks {
            Some(stack) => Ok(document.children_of(stack)?),
            None => Ok(Vec::new()),
        },
        _ => Err(unsupported(
            "Currently only supporting top level Timeline".to_owned(),
        )),
    }
}

/// Every descendant of an item, in the order OTIO's `find_children` gives.
pub(crate) fn find_children(document: &Document, id: NodeId) -> Result<Vec<NodeId>, Fail> {
    Ok(document.find_children(id, None, false, &|_| true)?)
}

/// Upstream's `_stackify_nested_groups`: every track that sits directly in a
/// track goes into a stack of its own, because AAF nests only through an
/// outer container.
///
/// Upstream walks each track's descendants once, by position in that walk,
/// and moves nested tracks while it goes. The position it inserts the new
/// stack at is the one in the walk, not in the track, and a track nested two
/// deep is not a child of the track it removes it from, which Python refuses;
/// both are kept.
fn stackify_nested_groups(document: &mut Document, timeline: NodeId) -> Result<(), Fail> {
    for track in tracks_of(document, timeline)? {
        if document.try_get(track)?.children().is_none() {
            // Upstream calls `find_children` on it, which only a
            // composition has.
            return Err(unwritable(format!(
                "a {} in the timeline's stack has no children to search",
                document.try_get(track)?.schema_name()
            )));
        }
        for (i, child) in find_children(document, track)?.into_iter().enumerate() {
            let is_nested = matches!(document.try_get(child)?, Node::Track(_));
            let parent_is_stack = document
                .try_get(child)?
                .parent()
                .map(|p| document.try_get(p).map(|n| matches!(n, Node::Stack(_))))
                .transpose()?
                .unwrap_or(false);
            if is_nested && !parent_is_stack {
                let stack = document.insert(Node::Stack(otio_core::schema::Stack {
                    item: otio_core::schema::ItemData::new(),
                    ..Default::default()
                }));
                document
                    .detach_child(track, child)
                    .map_err(|_| unwritable("a track nested in a nested track".to_owned()))?;
                document.append_child(stack, child)?;
                document.insert_child(track, i64::try_from(i).unwrap_or(i64::MAX), stack)?;
            }
        }
    }
    Ok(())
}

/// Whether upstream counts an item as a gap: a gap, or a clip of slug.
///
/// A clip made by any other generator is refused, as upstream refuses it.
pub(crate) fn is_considered_gap(document: &Document, id: NodeId) -> Result<bool, Fail> {
    match document.try_get(id)? {
        Node::Gap(_) => Ok(true),
        Node::Clip(_) => match document.try_get(media_reference(document, id)?)? {
            Node::GeneratorReference(generator) if generator.generator_kind == "Slug" => Ok(true),
            Node::GeneratorReference(generator) => Err(unsupported(format!(
                "AAF adapter does not support generator references of kind '{}'",
                generator.generator_kind
            ))),
            _ => Ok(false),
        },
        _ => Ok(false),
    }
}

/// A clip's active media reference: Python's `clip.media_reference`.
pub(crate) fn media_reference(document: &Document, clip: NodeId) -> Result<NodeId, Fail> {
    let Node::Clip(c) = document.try_get(clip)? else {
        return Err(unwritable(
            "media of something that is not a clip".to_owned(),
        ));
    };
    c.media_references
        .get(&c.active_media_reference_key)
        .copied()
        .ok_or_else(|| {
            Fail::Otio(otio_core::Error::NoActiveMediaReference {
                key: c.active_media_reference_key.clone(),
            })
        })
}

/// An object's metadata, empty for one that has none.
pub(crate) fn metadata_of(
    document: &Document,
    id: NodeId,
) -> Result<&otio_core::AnyDictionary, Fail> {
    static EMPTY: std::sync::OnceLock<otio_core::AnyDictionary> = std::sync::OnceLock::new();
    Ok(document
        .try_get(id)?
        .base()
        .map_or_else(|| EMPTY.get_or_init(Default::default), |b| &b.metadata))
}

/// How upstream's messages name an item: its name, then its Python type.
fn describe(document: &Document, id: NodeId) -> String {
    let node = document.get(id);
    let schema = node.map_or("object", Node::schema_name);
    format!(
        "{}<class 'opentimelineio._otio.{schema}'> {schema}",
        node.map_or("", Node::name)
    )
}

/// One of upstream's `__check`s: a value looked up along a path, and what
/// went wrong looking it up or comparing it.
struct Check {
    errors: Vec<String>,
}

impl Check {
    /// Looks the value up; `look` returns it, or why it is not there.
    fn new<T>(
        document: &Document,
        id: NodeId,
        path: &str,
        look: impl FnOnce() -> Result<T, String>,
    ) -> (Option<T>, Self) {
        match look() {
            Ok(value) => (Some(value), Self { errors: Vec::new() }),
            Err(why) => (
                None,
                Self {
                    errors: vec![format!(
                        "{}.{path} does not exist, {why}",
                        describe(document, id)
                    )],
                },
            ),
        }
    }

    /// `.equals(expected)` on a rate.
    fn rate_equals(
        document: &Document,
        id: NodeId,
        path: &str,
        look: impl FnOnce() -> Result<f64, String>,
        expected: Option<f64>,
    ) -> Self {
        let (value, mut check) = Self::new(document, id, path, look);
        if let Some(value) = value {
            // Python compares a float with `None` as unequal.
            if expected != Some(value) {
                check.errors.push(format!(
                    "{}.{path} not equal to {} (expected) != {} (actual)",
                    describe(document, id),
                    expected.map_or_else(|| "None".to_owned(), crate::py::python_float),
                    crate::py::python_float(value)
                ));
            }
        }
        check
    }
}

/// Upstream's `validate_metadata`: every item has to share the timeline's
/// rate, every clip has to say how much media it has, and every transition
/// has to carry the AAF metadata the writer builds it from.
fn validate_metadata(document: &Document, timeline: NodeId) -> Result<(), Fail> {
    let stack = match document.try_get(timeline)? {
        Node::Timeline(t) => t.tracks,
        _ => None,
    };
    let err = |e: otio_core::Error| e.to_string();
    let rate_of = |id: Option<NodeId>| -> Result<f64, String> {
        match id {
            Some(id) => document.duration(id).map(RationalTime::rate).map_err(err),
            None => Ok(RationalTime::default().rate()),
        }
    };

    let (edit_rate, first) = Check::new(document, timeline, "duration().rate", || rate_of(stack));
    let mut errors = first.errors;
    // Upstream checks the timeline's rate twice and reports both.
    errors.extend(
        Check::new(document, timeline, "duration().rate", || rate_of(stack))
            .1
            .errors,
    );

    let children = match stack {
        Some(stack) => find_children(document, stack)?,
        None => Vec::new(),
    };
    for child in children {
        let mut checks = Vec::new();
        if is_considered_gap(document, child)? {
            checks = vec![Check::rate_equals(
                document,
                child,
                "duration().rate",
                || rate_of(Some(child)),
                edit_rate,
            )];
        }
        match document.try_get(child)? {
            Node::Clip(_) => {
                let available = || -> Result<opentime::TimeRange, String> {
                    let media = media_reference(document, child)
                        .map_err(|_| "the clip has no active media reference".to_owned())?;
                    document
                        .try_get(media)
                        .map_err(err)?
                        .media()
                        .and_then(|m| m.available_range)
                        .ok_or_else(|| "'NoneType' object has no attribute 'duration'".to_owned())
                };
                checks = vec![
                    Check::rate_equals(
                        document,
                        child,
                        "duration().rate",
                        || rate_of(Some(child)),
                        edit_rate,
                    ),
                    Check::rate_equals(
                        document,
                        child,
                        "media_reference.available_range.duration.rate",
                        || available().map(|r| r.duration().rate()),
                        edit_rate,
                    ),
                    Check::rate_equals(
                        document,
                        child,
                        "media_reference.available_range.start_time.rate",
                        || available().map(|r| r.start_time().rate()),
                        edit_rate,
                    ),
                ];
            }
            Node::Transition(transition) => {
                // `metadata['AAF'][...]`, which raises `KeyError` naming the
                // first key missing.
                let lookup = |path: &[&str]| -> Result<(), String> {
                    let mut current = &transition.base.metadata;
                    for (i, key) in std::iter::once(&"AAF").chain(path).enumerate() {
                        match current.get(*key) {
                            Some(Any::Dictionary(d)) => current = d,
                            Some(_) if i == path.len() => return Ok(()),
                            Some(other) => {
                                return Err(format!(
                                    "a {} cannot be indexed by '{}'",
                                    other.type_name(),
                                    path[i]
                                ));
                            }
                            None => return Err(format!("'{key}'")),
                        }
                    }
                    Ok(())
                };
                checks = vec![
                    Check::rate_equals(
                        document,
                        child,
                        "duration().rate",
                        || rate_of(Some(child)),
                        edit_rate,
                    ),
                    Check::new(document, child, "metadata['AAF']['PointList']", || {
                        lookup(&["PointList"])
                    })
                    .1,
                    Check::new(
                        document,
                        child,
                        "metadata['AAF']['OperationGroup']['Operation']['DataDefinition']['Name']",
                        || lookup(&["OperationGroup", "Operation", "DataDefinition", "Name"]),
                    )
                    .1,
                    Check::new(
                        document,
                        child,
                        "metadata['AAF']['OperationGroup']['Operation']['Description']",
                        || lookup(&["OperationGroup", "Operation", "Description"]),
                    )
                    .1,
                    Check::new(
                        document,
                        child,
                        "metadata['AAF']['OperationGroup']['Operation']['Name']",
                        || lookup(&["OperationGroup", "Operation", "Name"]),
                    )
                    .1,
                    Check::new(document, child, "metadata['AAF']['CutPoint']", || {
                        lookup(&["CutPoint"])
                    })
                    .1,
                ];
            }
            _ => {}
        }
        for check in checks {
            errors.extend(check.errors);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(Fail::Other(Error::Invalid(errors)))
    }
}

/// What a clip's master mob is filed under: the MobID upstream found for it.
///
/// Upstream keys its master and tape mobs by that value, which is a string
/// when it came from metadata and a MobID when it came from a file or was
/// made up, so two spellings of one MobID are two keys there and here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MobKey {
    Text(String),
    Id(MobId),
}

impl MobKey {
    /// `aaf2.mobid.MobID(key)`.
    fn mob_id(&self) -> Result<MobId, Fail> {
        match self {
            Self::Id(id) => Ok(*id),
            Self::Text(text) => text
                .parse()
                .map_err(|e: aaf::ParseMobIdError| unwritable(e.to_string())),
        }
    }
}

/// Upstream's `AAFFileTranscriber`: the file being written, the composition
/// the timeline becomes, and the master and tape mobs clips share.
struct FileTranscriber<'a> {
    document: &'a Document,
    /// The file: pyaaf2's `f`.
    f: AafWriter,
    options: &'a WriteOptions,
    composition_mob: ObjRef,
    unique_mastermobs: HashMap<MobKey, ObjRef>,
    unique_tapemobs: HashMap<MobKey, ObjRef>,
    clip_mob_ids: HashMap<NodeId, MobKey>,
}

/// Avid's extended marker colour, which pyaaf2 does not define.
const COMMENT_MARKER_COLOR_EXTENDED: &str = "e96e6d45-c383-11d3-a069-006094eb75cb";
/// The `RGBColor` record the extended colour is.
const RGB_COLOR: &str = "e96e6d43-c383-11d3-a069-006094eb75cb";

impl<'a> FileTranscriber<'a> {
    /// `AAFFileTranscriber.__init__`.
    fn new(
        document: &'a Document,
        timeline: NodeId,
        mut f: AafWriter,
        options: &'a WriteOptions,
    ) -> Result<Self, Fail> {
        // `_register_marker_extended_color`, with the values upstream took
        // from files Media Composer exported. It is a no-op when the class
        // has the property already, as `register_propertydef` is here.
        f.register_propertydef(
            "CommentMarker",
            "CommentMarkerColorExtended",
            crate::py::auid(COMMENT_MARKER_COLOR_EXTENDED),
            Some(0xffda),
            crate::py::auid(RGB_COLOR),
            false,
            false,
        )?;

        let composition_mob = f.create("CompositionMob")?;
        f.set(composition_mob, "Name", document.try_get(timeline)?.name())?;
        f.set(composition_mob, "UsageCode", "Usage_TopLevel")?;
        f.add_mob(composition_mob)?;

        let mut me = Self {
            document,
            f,
            options,
            composition_mob,
            unique_mastermobs: HashMap::new(),
            unique_tapemobs: HashMap::new(),
            clip_mob_ids: HashMap::new(),
        };
        me.clip_mob_ids = me.gather_clip_mob_ids(timeline)?;

        me.transcribe_user_comments(timeline, composition_mob)?;
        me.transcribe_mob_attributes(timeline, composition_mob)?;
        Ok(me)
    }

    /// `_gather_clip_mob_ids`: a MobID for every clip that is not a gap,
    /// tried from the clip's metadata, its media's metadata and the AAF its
    /// media is, in that order or with the file first, and, if allowed, made
    /// up.
    fn gather_clip_mob_ids(&mut self, timeline: NodeId) -> Result<HashMap<NodeId, MobKey>, Fail> {
        #[derive(Clone, Copy)]
        enum Strategy {
            ClipMetadata,
            MediaMetadata,
            AafFile,
            Empty,
        }
        let mut strategies = vec![
            Strategy::ClipMetadata,
            Strategy::MediaMetadata,
            Strategy::AafFile,
        ];
        if self.options.prefer_file_mob_id {
            strategies.retain(|s| !matches!(s, Strategy::AafFile));
            strategies.insert(0, Strategy::AafFile);
        }
        if self.options.use_empty_mob_ids {
            strategies.push(Strategy::Empty);
        }

        let from_metadata = |id: NodeId| -> Result<Option<MobKey>, Fail> {
            let aaf = aaf_metadata(metadata_of(self.document, id)?)?;
            Ok(
                first_truthy([aaf.get("MobID"), aaf.get("SourceID")]).map(|v| match v {
                    Any::String(s) => MobKey::Text(s.clone()),
                    // pyaaf2 would take `str()` of anything else, and then refuse
                    // it for not being 64 digits long.
                    other => MobKey::Text(py::py_str(Some(other))),
                }),
            )
        };

        let mut ids = HashMap::new();
        for clip in self.document.find_clips(timeline)? {
            if is_considered_gap(self.document, clip)? {
                continue;
            }
            let mut found = None;
            for strategy in &strategies {
                found = match strategy {
                    Strategy::ClipMetadata => from_metadata(clip)?,
                    Strategy::MediaMetadata => {
                        from_metadata(media_reference(self.document, clip)?)?
                    }
                    Strategy::AafFile => self.mob_id_from_aaf_file(clip)?.map(MobKey::Id),
                    Strategy::Empty => Some(MobKey::Id(self.f.new_mob_id())),
                };
                if found.is_some() {
                    break;
                }
            }
            let key = found.ok_or_else(|| {
                unwritable(format!(
                    "Cannot find mob ID for clip '{}'",
                    self.document.try_get(clip).map_or("", Node::name)
                ))
            })?;
            ids.insert(clip, key);
        }
        Ok(ids)
    }

    /// `_from_aaf_file`: if the clip's media is an AAF file on disk with one
    /// master mob, that mob's MobID.
    ///
    /// Like upstream, this takes the media's URL as a path, as it is, and so
    /// only finds a file named by a plain path, relative to the working
    /// directory or absolute; a `file://` URL is not one.
    fn mob_id_from_aaf_file(&self, clip: NodeId) -> Result<Option<MobId>, Fail> {
        let Node::ExternalReference(media) = self
            .document
            .try_get(media_reference(self.document, clip)?)?
        else {
            return Ok(None);
        };
        let path = std::path::Path::new(&media.target_url);
        if !(path.is_file() && media.target_url.ends_with("aaf")) {
            return Ok(None);
        }
        let file = std::fs::File::open(path).map_err(|e| Fail::Other(Error::Io(e)))?;
        let mut aaf = aaf::Aaf::open(file).map_err(|e| Fail::Other(Error::Aaf(e)))?;
        let masters = aaf
            .mobs_of("MasterMob")
            .map_err(|e| Fail::Other(Error::Aaf(e)))?;
        if let [master] = masters.as_slice() {
            return aaf.mob_id(master).map_err(|e| Fail::Other(Error::Aaf(e)));
        }
        Ok(None)
    }

    /// `_transcribe_user_comments`: an item's `UserComments` onto a mob.
    ///
    /// Integers and strings are kept as they are and floats as rationals;
    /// upstream logs and skips anything else, and this skips it.
    fn transcribe_user_comments(&mut self, item: NodeId, mob: ObjRef) -> Result<(), Fail> {
        let comments = aaf_metadata(metadata_of(self.document, item)?)?.get_dict("UserComments")?;
        for (key, value) in comments.iter() {
            match value {
                Any::Bool(_) | Any::Int(_) | Any::UInt(_) | Any::String(_) => {
                    let value = py::to_write_value(value)?.expect("not None");
                    self.f.set_tagged_value(mob, "UserComments", key, value)?;
                }
                Any::Double(_) => {
                    let value = py::py_rational(value)?;
                    self.f.set_tagged_value(mob, "UserComments", key, value)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `_transcribe_mob_attributes`: an item's `MobAttributeList` onto a mob,
    /// as `_transcribe_user_comments` does, except that upstream refuses a
    /// value of any other kind rather than skipping it.
    fn transcribe_mob_attributes(&mut self, item: NodeId, mob: ObjRef) -> Result<(), Fail> {
        let attributes =
            aaf_metadata(metadata_of(self.document, item)?)?.get_dict("MobAttributeList")?;
        for (key, value) in attributes.iter() {
            match value {
                Any::Bool(_) | Any::Int(_) | Any::UInt(_) | Any::String(_) => {
                    let value = py::to_write_value(value)?.expect("not None");
                    self.f
                        .set_tagged_value(mob, "MobAttributeList", key, value)?;
                }
                Any::Double(_) => {
                    let value = py::py_rational(value)?;
                    self.f
                        .set_tagged_value(mob, "MobAttributeList", key, value)?;
                }
                other => {
                    return Err(unwritable(format!(
                        "Unsupported mob attribute type '{}' for key '{key}'.",
                        other.type_name()
                    )));
                }
            }
        }
        Ok(())
    }

    /// The MobID upstream found for a clip.
    fn mob_key(&self, clip: NodeId) -> Result<MobKey, Fail> {
        self.clip_mob_ids
            .get(&clip)
            .cloned()
            .ok_or_else(|| unwritable("a clip with no MobID".to_owned()))
    }

    /// `_unique_mastermob`: the master mob for a clip's MobID, made on first
    /// use and given the clip's and its media's comments and attributes.
    fn unique_mastermob(&mut self, clip: NodeId) -> Result<ObjRef, Fail> {
        let key = self.mob_key(clip)?;
        if let Some(mob) = self.unique_mastermobs.get(&key) {
            return Ok(*mob);
        }
        let mastermob = self.f.create("MasterMob")?;
        self.f
            .set(mastermob, "Name", self.document.try_get(clip)?.name())?;
        self.f.set(mastermob, "MobID", key.mob_id()?)?;
        self.f.add_mob(mastermob)?;
        self.unique_mastermobs.insert(key, mastermob);

        self.transcribe_user_comments(clip, mastermob)?;
        self.transcribe_mob_attributes(clip, mastermob)?;
        // The media's comments and attributes after the clip's, so that
        // they win where both have the same key.
        let media = media_reference(self.document, clip)?;
        self.transcribe_user_comments(media, mastermob)?;
        self.transcribe_mob_attributes(media, mastermob)?;
        Ok(mastermob)
    }

    /// `_unique_tapemob`: the tape source mob for a clip's MobID, made on
    /// first use with a timecode slot spanning the clip's media.
    fn unique_tapemob(&mut self, clip: NodeId) -> Result<ObjRef, Fail> {
        let key = self.mob_key(clip)?;
        if let Some(mob) = self.unique_tapemobs.get(&key) {
            return Ok(*mob);
        }
        let name = self.document.try_get(clip)?.name().to_owned();
        let tapemob = self.f.create("SourceMob")?;
        self.f.set(tapemob, "Name", name.as_str())?;
        let import = self.f.create("ImportDescriptor")?;
        self.f.set(tapemob, "EssenceDescription", import)?;

        // A rate that is not a whole number is taken as drop frame at the
        // whole number nearest it. Python's `round` rounds halves to even.
        let edit_rate = self.document.visible_range(clip)?.duration().rate();
        let timecode_fps = edit_rate.round_ties_even();
        let fps = u16::try_from(py::float_int(timecode_fps)?)
            .map_err(|_| unwritable(format!("{timecode_fps} is not a timecode rate")))?;
        #[allow(clippy::float_cmp)]
        let drop = edit_rate != timecode_fps;
        let (_, timecode_slot) = self.f.create_tape_slots(
            tapemob,
            &name,
            rational(edit_rate)?,
            fps,
            drop,
            None,
            None,
        )?;

        let available = self.media_available_range(clip)?;
        let timecode_start = py::float_int(available.start_time().value())?;
        let timecode_length = py::float_int(available.duration().value())?;
        let timecode = self.segment(timecode_slot)?;
        self.f.set(timecode, "Start", timecode_start)?;
        self.f.set(timecode, "Length", timecode_length)?;
        self.f.add_mob(tapemob)?;
        self.unique_tapemobs.insert(key, tapemob);

        if let Node::ExternalReference(media) = self
            .document
            .try_get(media_reference(self.document, clip)?)?
        {
            if !media.target_url.is_empty() {
                let locator = self.f.create("NetworkLocator")?;
                self.f
                    .set(locator, "URLString", media.target_url.as_str())?;
                let descriptor = self
                    .f
                    .get_object(tapemob, "EssenceDescription")?
                    .ok_or_else(|| unwritable("a tape mob without a descriptor".to_owned()))?;
                self.f.append(descriptor, "Locator", locator)?;
            }
        }
        Ok(tapemob)
    }

    /// A clip's `media_reference.available_range`, which upstream uses
    /// without checking it is there.
    fn media_available_range(&self, clip: NodeId) -> Result<opentime::TimeRange, Fail> {
        self.document
            .try_get(media_reference(self.document, clip)?)?
            .media()
            .and_then(|m| m.available_range)
            .ok_or_else(|| unwritable("'NoneType' object has no attribute 'start_time'".to_owned()))
    }

    /// A slot's segment: pyaaf2's `slot.segment`.
    fn segment(&self, slot: ObjRef) -> Result<ObjRef, Fail> {
        self.f
            .get_object(slot, "Segment")?
            .ok_or_else(|| unwritable("'NoneType' object has no attribute 'length'".to_owned()))
    }

    /// A slot's identifier: pyaaf2's `slot.slot_id`.
    fn slot_id(&self, slot: ObjRef) -> Result<u32, Fail> {
        let id = self
            .f
            .get_int(slot, "SlotID")?
            .ok_or_else(|| unwritable("a slot without a SlotID".to_owned()))?;
        u32::try_from(id).map_err(|_| unwritable(format!("{id} is not a slot ID")))
    }

    /// A component's length: pyaaf2's `component.length`.
    fn length(&self, component: ObjRef) -> Result<i64, Fail> {
        self.f
            .get_int(component, "Length")?
            .ok_or_else(|| unwritable("a component without a length".to_owned()))
    }

    /// `add_timecode`: a timecode slot on the composition, starting at the
    /// timeline's global start time if it has one and at zero otherwise.
    ///
    /// The timecode's rate is a hint for display, so it is the nearest of
    /// the four rates upstream knows, non-drop.
    fn add_timecode(
        &mut self,
        global_start_time: Option<RationalTime>,
        default_edit_rate: Option<f64>,
    ) -> Result<(), Fail> {
        let (edit_rate, start) = match global_start_time {
            Some(time) => (time.rate(), py::float_int(time.value())?),
            None => (default_edit_rate.unwrap_or_default(), 0),
        };
        let slot = self
            .f
            .create_timeline_slot(self.composition_mob, rational(edit_rate)?, None)?;
        self.f.set(slot, "SlotName", "TC")?;
        // The primary timecode track.
        self.f.set(slot, "PhysicalTrackNumber", 1)?;

        let timecode = self.f.create("Timecode")?;
        let fps = nearest_timecode(edit_rate);
        self.f.set(timecode, "FPS", py::float_int(fps)?)?;
        self.f.set(timecode, "Drop", false)?;
        self.f.set(timecode, "Start", start)?;
        self.f.set(slot, "Segment", timecode)?;
        Ok(())
    }
}

/// Upstream's `_nearest_timecode`: of 24, 25, 30 and 60, the rate nearest
/// `rate`, the first of two that are equally near.
fn nearest_timecode(rate: f64) -> f64 {
    let mut nearest = 0.0;
    let mut min_diff = f64::INFINITY;
    for valid in [24.0, 25.0, 30.0, 60.0] {
        #[allow(clippy::float_cmp)]
        if valid == rate {
            return rate;
        }
        let diff = (rate - valid).abs();
        if diff >= min_diff {
            continue;
        }
        min_diff = diff;
        nearest = valid;
    }
    nearest
}

#[cfg(test)]
mod tests {
    use super::nearest_timecode;

    #[test]
    fn timecode_rates_round_to_the_ones_upstream_knows() {
        assert_eq!(nearest_timecode(24.0), 24.0);
        assert_eq!(nearest_timecode(23.976), 24.0);
        assert_eq!(nearest_timecode(29.97), 30.0);
        assert_eq!(nearest_timecode(27.5), 25.0);
        assert_eq!(nearest_timecode(50.0), 60.0);
        assert_eq!(nearest_timecode(0.0), 24.0);
    }
}

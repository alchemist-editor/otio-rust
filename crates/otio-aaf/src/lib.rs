//! Reading and writing Advanced Authoring Format files as OpenTimelineIO
//! timelines.
//!
//! AAF is how Avid Media Composer and the tools around it hand an edit to one
//! another. This crate reads one as an OTIO [`Document`]:
//!
//! ```no_run
//! let document = otio_aaf::read_from_file("cut.aaf")?;
//! let json = otio_core::to_string_pretty(&document, otio_core::DEFAULT_INDENT)?;
//! # Ok::<(), otio_aaf::Error>(())
//! ```
//!
//! # How an AAF becomes a timeline
//!
//! The shapes do not line up one to one, and the mapping is upstream's:
//!
//! | AAF | OTIO |
//! |---|---|
//! | the mobs worth showing | a `SerializableCollection` |
//! | `CompositionMob`, `MasterMob` | a `Timeline` |
//! | `TimelineMobSlot`, `MobSlot` | a `Track` |
//! | `Sequence` | a `Track` of its components |
//! | `NestedScope` | a `Stack` of its slots |
//! | `SourceClip` | a `Clip`, a `Stack` or a `Gap`, by what it points at |
//! | `Filler` | a `Gap` |
//! | `OperationGroup` | a `Stack` of its inputs, with an `Effect` on it |
//! | `Selector` | what it selects, or its one alternate, disabled, when muted |
//! | `Transition` | a `Transition` |
//! | `DescriptiveMarker` | a `Marker` |
//! | `SourceMob` | an `ExternalReference` or a `MissingReference` |
//! | `Timecode`, `Pulldown`, `EdgeCode` | nothing; they are read for their times |
//!
//! Every object keeps what it came from under `metadata["AAF"]`, so what OTIO
//! has no field for still survives the trip.
//!
//! # The passes after transcription
//!
//! Transcription gives AAF's structure in OTIO's objects. Upstream then runs
//! three passes over it, in this order, and so does this crate:
//!
//! 1. `_fix_transitions` moves a transition's length onto its neighbours.
//!    AAF counts it in both; OTIO counts it in neither. This one always runs.
//! 2. `_attach_markers` moves each marker from the slot that carries it onto
//!    the item it points at.
//! 3. `_simplify` collapses the nesting AAF has and OTIO does not need, so a
//!    simple edit reads as a timeline of tracks of clips.
//!
//! The last two are [`ReadOptions`], both on by default as upstream's are.
//! [`ReadOptions::structural`] turns them off, which is upstream's
//! `simplify=False, attach_markers=False`: the edit as AAF shapes it.
//!
//! Both ways match upstream byte for byte, once written as OTIO JSON, on
//! every sample file in its test suite.
//!
//! # Baking keyframes and logging
//!
//! Upstream's two other reading options are [`ReadOptions`] too.
//! [`ReadOptions::bake_keyframed_properties`] records each keyframed effect
//! parameter's value at every frame of its effect, interpolated as pyaaf2
//! interpolates it, beside the keyframes themselves.
//! [`ReadOptions::transcribe_log`] hands a [`TranscribeLog`] the lines
//! upstream prints as it reads, word for word. Both match upstream on every
//! sample file, with the exceptions their documentation names: a baked
//! curve's last digit can depend on the platform's maths library, and three
//! rare log lines cannot print a Python object's memory address.
//!
//! # Writing
//!
//! [`write_to_file`] and [`write_to_bytes`] write a document holding a
//! timeline as an AAF, the way upstream's adapter does:
//!
//! ```no_run
//! let document = otio_core::from_str(&std::fs::read_to_string("cut.otio")?)?;
//! otio_aaf::write_to_file(&document, "cut.aaf")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Each track becomes a slot of a composition mob. Each clip becomes a
//! source clip of a master mob, which points at a file source mob
//! describing the media, which points at a tape source mob carrying its
//! timecode: the chain Media Composer expects to relink through. Gaps
//! become fillers, dissolves transitions, nested tracks sequences, stacks
//! submaster operation groups, and markers descriptive markers on an event
//! slot per track. What the reader kept under `metadata["AAF"]` is written
//! back, so a file read and written again keeps its MobIDs, descriptors,
//! comments and marker dates.
//!
//! Upstream writes through pyaaf2, and this through the `aaf` crate's port of
//! pyaaf2's write path, which lays the file out byte for byte as pyaaf2 does
//! for the same operations in the same order. The writer here makes the
//! same operations in the same order as upstream's, so given the same clock
//! and the same random identifiers, which [`WriteOptions::sources`] fixes,
//! it writes the same bytes. The tests hold it to that on files upstream
//! wrote. Byte parity with upstream is the only check of the output: no
//! file written here has been opened in Media Composer as part of testing.
//!
//! Embedding media in the file ([`WriteOptions::embed_essence`]) is ported
//! with the rest: essence copied out of another AAF, or a raw DNxHD stream
//! imported, as upstream does both, and refused where upstream refuses or
//! fails. Upstream's pre- and post-write hooks run Python plugins, and there
//! are none to run here, so the one upstream suggests for making media it
//! can embed out of other files, `otio_aaf_pre_write_transcribe`, has no
//! counterpart.

mod adapter;
mod error;
mod interpolate;
mod log;
mod markers;
mod master_mob;
mod passes;
mod py;
mod simplify;
mod transcribe;
mod write;

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;

use aaf::property::RefKey;
use aaf::{Aaf as AafFile, Auid, MobId, Object};
use otio_core::{Document, NodeId};

pub use adapter::{Aaf, ReadOptions, WriteOptions};
pub use error::{Error, Result};
pub use log::TranscribeLog;
pub use write::Sources;

/// Replaying what pyaaf2 handed out while it wrote a fixture, which
/// [`WriteOptions::with_replay`] sets a write up from. Testing support, not
/// part of the supported interface.
#[doc(hidden)]
pub use aaf::write::replay;

/// The definition collections a weak reference can name something in.
///
/// A weak reference carries a key and not a path, so resolving one means
/// knowing where in the file's dictionary to look. AAF keeps definitions in
/// these nine collections and nothing says which a given key belongs to, so
/// all nine are indexed together.
const DEFINITION_KINDS: [&str; 9] = [
    "DataDefinitions",
    "OperationDefinitions",
    "ParameterDefinitions",
    "CodecDefinitions",
    "ContainerDefinitions",
    "InterpolationDefinitions",
    "PluginDefinitions",
    "KLVDataDefinitions",
    "TaggedValueDefinitions",
];

/// Reads an AAF file as a document.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, is not a readable AAF, or
/// describes an edit this crate cannot build a timeline from.
pub fn read_from_file(path: impl AsRef<Path>) -> Result<Document> {
    read(std::fs::File::open(path)?)
}

/// Reads an AAF file as a document, with the passes chosen.
///
/// # Errors
///
/// As [`read_from_file`].
pub fn read_from_file_with(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Document> {
    read_with(std::fs::File::open(path)?, options)
}

/// Reads an AAF as a document.
///
/// # Errors
///
/// Returns an error if the input is not a readable AAF, or describes an edit
/// this crate cannot build a timeline from.
pub fn read<R: Read + Seek>(reader: R) -> Result<Document> {
    read_with(reader, &ReadOptions::default())
}

/// Reads an AAF as a document, with the passes chosen.
///
/// # Errors
///
/// As [`read`].
pub fn read_with<R: Read + Seek>(reader: R, options: &ReadOptions) -> Result<Document> {
    Transcriber::new(AafFile::open(reader)?).run(options)
}

/// Writes a document holding a timeline as an AAF file, with upstream's
/// defaults.
///
/// # Errors
///
/// As [`write_to_bytes_with`], and if the file cannot be written.
pub fn write_to_file(document: &Document, path: impl AsRef<Path>) -> Result<()> {
    write_to_file_with(document, path, &WriteOptions::default())
}

/// Writes a document holding a timeline as an AAF file, with the options
/// chosen.
///
/// # Errors
///
/// As [`write_to_bytes_with`], and if the file cannot be written.
pub fn write_to_file_with(
    document: &Document,
    path: impl AsRef<Path>,
    options: &WriteOptions,
) -> Result<()> {
    let bytes = write_to_bytes_with(document, options)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Writes a document holding a timeline as the bytes of an AAF file, with
/// upstream's defaults.
///
/// # Errors
///
/// As [`write_to_bytes_with`].
pub fn write_to_bytes(document: &Document) -> Result<Vec<u8>> {
    write_to_bytes_with(document, &WriteOptions::default())
}

/// Writes a document holding a timeline as the bytes of an AAF file, with
/// the options chosen.
///
/// # Errors
///
/// Returns [`Error::Unsupported`] if the document's root is not a timeline,
/// a track is neither video nor audio, or an item is of a kind AAF has no
/// place for; [`Error::Invalid`] if the timeline lacks what upstream checks
/// for first, with everything it lacks listed; [`Error::MissingEssence`],
/// [`Error::Embed`] or [`Error::EmbedOnAudioTrack`] if media to embed is
/// not there or cannot be embedded; and [`Error::Unwritable`] or
/// [`Error::Write`] where upstream would fail part way, the latter among
/// other things for a `.dnx` file that is not a DNxHD stream.
pub fn write_to_bytes_with(document: &Document, options: &WriteOptions) -> Result<Vec<u8>> {
    write::write(document, options)
}

/// The state a read carries: the file, the document being built, and two
/// caches that keep the walk from doing the same work twice.
struct Transcriber<R> {
    aaf: AafFile<R>,
    document: Document,
    /// Every definition in the file's dictionary, by key.
    ///
    /// Built on first use, because a file whose objects name no definitions
    /// should not pay for reading them.
    definitions: Option<HashMap<Auid, Object>>,
    /// The timeline each mob was transcribed into.
    ///
    /// A mob is reached once per source clip that names it, and transcribing
    /// one means walking everything under it. Upstream caches for the same
    /// reason.
    timelines: HashMap<MobId, NodeId>,
    /// Where upstream's `transcribe_log` goes, if anywhere.
    log: Option<TranscribeLog>,
    /// Whether keyframed parameters are baked per frame.
    bake: bool,
    /// How far upstream's log would indent the object being transcribed.
    indent: usize,
}

impl<R: Read + Seek> Transcriber<R> {
    fn new(aaf: AafFile<R>) -> Self {
        Self {
            aaf,
            document: Document::new(),
            definitions: None,
            timelines: HashMap::new(),
            log: None,
            bake: false,
            indent: 0,
        }
    }

    /// Transcribes the whole file, runs the passes, and hands back the
    /// document.
    fn run(mut self, options: &ReadOptions) -> Result<Document> {
        self.log.clone_from(&options.transcribe_log);
        self.bake = options.bake_keyframed_properties;
        let mobs = self.mobs_worth_showing()?;
        let mut root = self.transcribe_mobs(&mobs)?;
        // Always, and before markers: AAF counts marker positions without
        // the transition offsets.
        passes::fix_transitions(&mut self.document, root)?;
        if options.attach_markers {
            self.attach_markers(root)?;
        }
        if options.simplify {
            root = simplify::simplify(&mut self.document, root)?;
        }
        simplify::retain_reachable(&mut self.document, root);
        self.document.set_root(Some(root));
        Ok(self.document)
    }

    /// The mobs a reader of this file would want to see.
    ///
    /// Upstream's heuristic, in its order: the mobs marked top level, else
    /// the compositions, else the master mobs. A file with none of those
    /// transcribes to an empty collection rather than failing, which is what
    /// makes an AAF holding nothing readable.
    fn mobs_worth_showing(&mut self) -> Result<Vec<Object>> {
        for (found, kind) in [
            (self.aaf.top_level_mobs()?, "top level"),
            (self.aaf.mobs_of("CompositionMob")?, "composition"),
            (self.aaf.mobs_of("MasterMob")?, "master"),
        ] {
            if !found.is_empty() {
                self.log_at(0, || format!("---\nTranscribing {kind} mobs\n---"));
                return Ok(found);
            }
        }
        self.log_at(0, || "---\nNo mobs found to transcribe\n---".to_owned());
        Ok(Vec::new())
    }

    /// Writes a line of upstream's log, indented as upstream indents it.
    ///
    /// The line is only made if there is a log to write it to.
    pub(crate) fn log_at(&self, indent: usize, line: impl FnOnce() -> String) {
        if let Some(log) = &self.log {
            log.write(&format!("{:indent$}{}", "", line()));
        }
    }

    /// Writes a line of upstream's log at the current object's indent.
    pub(crate) fn log(&self, line: impl FnOnce() -> String) {
        self.log_at(self.indent, line);
    }

    /// Runs `f` two spaces further in, as upstream's `indent + 2`.
    pub(crate) fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.indent += 2;
        let result = f(self);
        self.indent -= 2;
        result
    }

    /// The object a weak reference names, if the file holds it.
    ///
    /// A key of sixteen bytes names a definition and one of thirty-two names
    /// a mob, which is the only thing that tells the two apart.
    fn resolve(&mut self, key: RefKey) -> Result<Option<Object>> {
        match key {
            RefKey::MobId(id) => Ok(self.aaf.mob(id)?),
            RefKey::Auid(key) => Ok(self.definitions()?.get(&key).cloned()),
        }
    }

    /// Every definition in the file's dictionary, by key.
    ///
    /// Built on first use and kept, because a weak reference is resolved once
    /// per property that holds one and the file's dictionary does not change
    /// under us.
    fn definitions(&mut self) -> Result<&HashMap<Auid, Object>> {
        if self.definitions.is_none() {
            let mut index = HashMap::new();
            for kind in DEFINITION_KINDS {
                // A file need not carry every collection, and one it leaves
                // out is not an error: there is simply nothing of that kind.
                let Ok(found) = self.aaf.definitions(kind) else {
                    continue;
                };
                for (key, object) in found {
                    if let RefKey::Auid(key) = key {
                        index.insert(key, object);
                    }
                }
            }
            self.definitions = Some(index);
        }
        Ok(self.definitions.as_ref().expect("the index was just built"))
    }
}

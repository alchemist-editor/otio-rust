//! Reading Advanced Authoring Format files as OpenTimelineIO timelines.
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
//! | `OperationGroup` | the item it wraps, with an `Effect` on it |
//! | `Transition` | a `Transition` |
//! | `SourceMob` | an `ExternalReference` or a `MissingReference` |
//! | `Timecode`, `Pulldown`, `EdgeCode` | nothing; they are read for their times |
//!
//! Every object keeps what it came from under `metadata["AAF"]`, so what OTIO
//! has no field for still survives the trip.
//!
//! # What this reads, and what it does not
//!
//! This is the structural transcription: the shape of the edit, its times and
//! its media. Upstream then runs three passes over that result, and none of
//! them is here yet — `_fix_transitions`, which moves a transition's length
//! onto its neighbours, `_attach_markers`, which moves a marker from the slot
//! that carries it onto the item it points at, and `_simplify`, which
//! collapses the nesting AAF has and OTIO does not need. Reading a file
//! through this crate is what upstream calls reading with `simplify=False`
//! and `attach_markers=False`.
//!
//! Writing an AAF is not implemented.

mod error;
mod master_mob;
mod metadata;
mod transcribe;

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;

use aaf::property::RefKey;
use aaf::{Aaf, Auid, MobId, Object};
use otio_core::schema::{Base, ItemData, SerializableCollection};
use otio_core::{Any, AnyDictionary, Document, Node, NodeId};

pub use error::{Error, Result};

/// The name the collection at the root of every transcribed file carries.
///
/// Not a name anybody chose. Upstream hands its transcriber a Python list and
/// falls back to an object's class name when it has none, so the collection
/// is called after the type of the container it arrived in. Keeping the name
/// keeps a file read here identical to one read there.
const LIST_NAME: &str = "list";

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

/// Reads an AAF as a document.
///
/// # Errors
///
/// Returns an error if the input is not a readable AAF, or describes an edit
/// this crate cannot build a timeline from.
pub fn read<R: Read + Seek>(reader: R) -> Result<Document> {
    Transcriber::new(Aaf::open(reader)?).run()
}

/// The state a read carries: the file, the document being built, and two
/// caches that keep the walk from doing the same work twice.
struct Transcriber<R> {
    aaf: Aaf<R>,
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
}

impl<R: Read + Seek> Transcriber<R> {
    fn new(aaf: Aaf<R>) -> Self {
        Self {
            aaf,
            document: Document::new(),
            definitions: None,
            timelines: HashMap::new(),
        }
    }

    /// Transcribes the whole file and hands back the document.
    fn run(mut self) -> Result<Document> {
        let mobs = self.mobs_worth_showing()?;
        let root = self.transcribe_mobs(&mobs)?;
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
        for found in [
            self.aaf.top_level_mobs()?,
            self.aaf.mobs_of("CompositionMob")?,
            self.aaf.mobs_of("MasterMob")?,
        ] {
            if !found.is_empty() {
                return Ok(found);
            }
        }
        Ok(Vec::new())
    }

    /// The collection those mobs make up, named as [`LIST_NAME`] explains.
    fn transcribe_mobs(&mut self, mobs: &[Object]) -> Result<NodeId> {
        let mut children = Vec::new();
        for mob in mobs {
            if let Some(child) = self.transcribe(mob, &[], None)? {
                children.push(child);
            }
        }
        let mut aaf = AnyDictionary::new();
        aaf.insert("Name".to_owned(), Any::String(LIST_NAME.to_owned()));
        let mut metadata = AnyDictionary::new();
        metadata.insert("AAF".to_owned(), Any::Dictionary(aaf));
        Ok(self
            .document
            .insert(Node::SerializableCollection(SerializableCollection {
                base: Base {
                    name: LIST_NAME.to_owned(),
                    metadata,
                },
                children,
            })))
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

/// Item fields holding nothing but a name and its AAF metadata.
fn item_with(name: String, aaf: AnyDictionary) -> ItemData {
    let mut metadata = AnyDictionary::new();
    metadata.insert("AAF".to_owned(), Any::Dictionary(aaf));
    ItemData {
        base: Base { name, metadata },
        ..ItemData::new()
    }
}

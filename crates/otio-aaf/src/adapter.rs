//! The [`Adapter`] implementation, which is how the rest of the workspace
//! reaches this format.
//!
//! AAF is the reason [`Adapter`] is stated over bytes rather than over `&str`:
//! it is a compound file, not text, so it implements [`Adapter`] and not
//! [`otio_adapter::TextAdapter`].
//!
//! Writing goes through [`crate::write_to_bytes_with`], and a timeline the
//! writer cannot write is reported as unsupported, since nothing about the
//! caller's input is malformed as OTIO.
//!
//! Reading from bytes means holding the whole file in memory. That is what the
//! trait asks for, and [`crate::read`] takes anything that reads and seeks, so
//! [`Adapter::read_from_file`] is overridden to hand it the file itself and
//! leave a large AAF on disk where it is.

use std::path::Path;

use otio_adapter::{Adapter, Error, Result};
use otio_core::Document;

/// The Advanced Authoring Format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Aaf;

/// What a caller can ask for when reading an AAF.
///
/// Upstream's `read_from_file` arguments, with upstream's defaults: both
/// passes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReadOptions {
    /// Collapse the nesting AAF has and OTIO does not need.
    ///
    /// Off, the timeline keeps AAF's shape: a track per slot, a stack per
    /// nested composition, a track per sequence inside it.
    pub simplify: bool,
    /// Move each marker from the slot that carries it onto the item it
    /// points at.
    ///
    /// Off, markers stay on the tracks AAF keeps them on, with their
    /// positions in those tracks' time.
    pub attach_markers: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            simplify: true,
            attach_markers: true,
        }
    }
}

impl ReadOptions {
    /// Upstream's defaults: both passes on.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The structural transcription alone, with neither pass: what upstream
    /// calls reading with `simplify=False` and `attach_markers=False`.
    #[must_use]
    pub const fn structural() -> Self {
        Self {
            simplify: false,
            attach_markers: false,
        }
    }

    /// These options with `simplify` set.
    #[must_use]
    pub const fn with_simplify(mut self, simplify: bool) -> Self {
        self.simplify = simplify;
        self
    }

    /// These options with `attach_markers` set.
    #[must_use]
    pub const fn with_attach_markers(mut self, attach_markers: bool) -> Self {
        self.attach_markers = attach_markers;
        self
    }
}

/// What a caller can ask for when writing an AAF.
///
/// Upstream's `write_to_file` arguments, with upstream's defaults, and the
/// few things Python finds for itself that a library should let its caller
/// decide: where times and identifiers come from, whom a new marker is
/// credited to, and which platform the file says wrote it.
///
/// Upstream also runs pre- and post-write hooks, Python plugins handed the
/// open pyaaf2 file. There is no plugin mechanism here, so there are no
/// hooks to ask for.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct WriteOptions {
    /// Look for a clip's MobID in the AAF file its media names before
    /// looking in its metadata.
    ///
    /// Off, the clip's metadata comes first, then its media's metadata, then
    /// the file.
    pub prefer_file_mob_id: bool,
    /// Make up a MobID for a clip that has none anywhere.
    ///
    /// Off, such a clip stops the write, since a made-up MobID links the
    /// clip to no media Media Composer knows.
    pub use_empty_mob_ids: bool,
    /// Embed each clip's media in the file.
    ///
    /// Not implemented: upstream imports DNxHD and WAV essence through
    /// pyaaf2, or copies it out of another AAF, and doing either needs media
    /// decoding this crate does not do. Writing with this on is refused
    /// rather than producing a file without the media it was asked for.
    pub embed_essence: bool,
    /// Give each master mob an edge code slot carrying its media's range,
    /// which Media Composer shows as Frame Count Start and End.
    pub create_edgecode: bool,
    /// Whom a marker with no user of its own is credited to.
    ///
    /// Unset, the user is found as Python's `getpass.getuser()` finds it on
    /// most systems, from `LOGNAME`, `USER`, `LNAME` or `USERNAME`; if none
    /// is set, a timeline with such a marker cannot be written.
    pub user: Option<String>,
    /// Where the file's times and identifiers come from.
    ///
    /// Unset, the system clock and random identifiers, as pyaaf2 uses. Set,
    /// a file can be written the same way twice.
    pub sources: Option<crate::Sources>,
    /// The platform the file records it was written on.
    ///
    /// Unset, the one this program runs on, as pyaaf2 records Python's
    /// `sys.platform`.
    pub platform: Option<String>,
}

impl WriteOptions {
    /// Upstream's defaults: every option off.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// These options with `prefer_file_mob_id` set.
    #[must_use]
    pub const fn with_prefer_file_mob_id(mut self, prefer_file_mob_id: bool) -> Self {
        self.prefer_file_mob_id = prefer_file_mob_id;
        self
    }

    /// These options with `use_empty_mob_ids` set.
    #[must_use]
    pub const fn with_use_empty_mob_ids(mut self, use_empty_mob_ids: bool) -> Self {
        self.use_empty_mob_ids = use_empty_mob_ids;
        self
    }

    /// These options with `embed_essence` set.
    #[must_use]
    pub const fn with_embed_essence(mut self, embed_essence: bool) -> Self {
        self.embed_essence = embed_essence;
        self
    }

    /// These options with `create_edgecode` set.
    #[must_use]
    pub const fn with_create_edgecode(mut self, create_edgecode: bool) -> Self {
        self.create_edgecode = create_edgecode;
        self
    }

    /// These options crediting new markers to `user`.
    #[must_use]
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// These options drawing times and identifiers from `sources`.
    #[must_use]
    pub fn with_sources(mut self, sources: crate::Sources) -> Self {
        self.sources = Some(sources);
        self
    }

    /// These options recording `platform` as the one the file was written
    /// on.
    #[must_use]
    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    /// These options set up to write as the generator of a written fixture
    /// did, so the file comes out identical to the one upstream wrote: the
    /// times and identifiers drawn from the sidecar's replay, new markers
    /// credited to its user, and the platform recorded as `linux`, which
    /// the generator pins Python's `sys.platform` to.
    ///
    /// Testing support, not part of the supported interface. The writer's
    /// four flags are left as they are: the sidecar lists them, and a
    /// caller checking that its own flags reach the writer sets them.
    #[doc(hidden)]
    #[must_use]
    pub fn with_replay(mut self, sidecar: &crate::replay::Sidecar) -> Self {
        if let Some(user) = &sidecar.user {
            self.user = Some(user.clone());
        }
        self.with_sources(crate::Sources::new(
            sidecar.replay.clone(),
            sidecar.replay.clone(),
        ))
        .with_platform("linux")
    }
}

impl Adapter for Aaf {
    type ReadOptions = ReadOptions;
    type WriteOptions = WriteOptions;

    const NAME: &'static str = "AAF";
    const SUFFIXES: &'static [&'static str] = &["aaf"];

    fn read_from_bytes(input: &[u8], options: &Self::ReadOptions) -> Result<Document> {
        crate::read_with(std::io::Cursor::new(input), options).map_err(into_adapter_error)
    }

    /// Reads straight from the file rather than from its bytes.
    ///
    /// An AAF is read by seeking around it, so there is no reason to copy a
    /// file that may be hundreds of megabytes into memory first.
    fn read_from_file(path: impl AsRef<Path>, options: &Self::ReadOptions) -> Result<Document> {
        crate::read_from_file_with(path, options).map_err(into_adapter_error)
    }

    fn write_to_bytes(document: &Document, options: &Self::WriteOptions) -> Result<Vec<u8>> {
        crate::write_to_bytes_with(document, options).map_err(|error| match error {
            crate::Error::Io(error) => Error::Io(error),
            crate::Error::Otio(error) => Error::Core(error),
            other => Error::unsupported(other.to_string()),
        })
    }
}

/// This crate's error as the one the trait reports.
///
/// A malformed file is a parse failure whatever layer noticed it, since to a
/// caller of the adapter there is one operation and it did not work.
fn into_adapter_error(error: crate::Error) -> Error {
    match error {
        crate::Error::Io(error) => Error::Io(error),
        crate::Error::Otio(error) => Error::Core(error),
        other => Error::parse(other.to_string()),
    }
}

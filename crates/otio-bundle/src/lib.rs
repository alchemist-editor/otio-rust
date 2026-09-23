//! OpenTimelineIO file bundles: a timeline packaged with its media.
//!
//! This is upstream's `opentimelineio/bundle.h`. A bundle holds a timeline as
//! `content.otio`, a `version.txt` saying which bundle format it is, and every
//! file the timeline's media references point at, copied under `media/`
//! with the references rewritten to point there. It comes in two forms: an
//! `.otioz` is a zip archive, with the media stored uncompressed so that it
//! can be read in place, and an `.otiod` is the same layout as a directory.
//!
//! ```text
//! cut.otioz / cut.otiod
//! ├── version.txt      "1.0.0"
//! ├── content.otio     the timeline, references rewritten to media/...
//! └── media/
//!     ├── shot_010.mov
//!     └── render.0001.exr ...
//! ```
//!
//! Every media file lands directly under `media/`, so two files with the same
//! name in different directories cannot both go in; writing such a timeline
//! fails rather than silently dropping one. What happens to a reference that
//! is not a file on disk — a generator, or an `http://` URL — is up to the
//! [`MediaReferencePolicy`].
//!
//! Upstream writes the zip with minizip-ng. The workspace carries no
//! third-party dependencies, so this crate has its own small zip reader and
//! writer and its own DEFLATE; see the `zip` and `deflate` modules.

mod crc32;
mod deflate;
mod zip;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use otio_core::schema::{MediaReferenceData, MissingReference};
use otio_core::{Any, Document, Node, NodeId};

/// Turns a `file://` URL into a path, as upstream's `bundle::file_from_url`
/// does. It lives in `otio-core`, which the Python `url_utils` module also
/// calls, so the bundle and everything else read a URL the same way.
pub use otio_core::bundle::{InvalidEscape, file_from_url};

/// The bundle format version written to [`VERSION_FILE`].
pub const VERSION: &str = "1.0.0";

/// The name of the file in a bundle that holds [`VERSION`].
pub const VERSION_FILE: &str = "version.txt";

/// The name of the file in a bundle that holds the timeline.
pub const TIMELINE_FILE: &str = "content.otio";

/// The directory in a bundle that holds the media.
pub const MEDIA_DIR: &str = "media";

/// What to do with media references when writing a bundle.
///
/// A [`MissingReference`] is left alone whatever the policy: it names no
/// media, so there is nothing to bundle and nothing to complain about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaReferencePolicy {
    /// Fail if any reference is not a file on disk.
    #[default]
    ErrorIfNotFile,
    /// Replace each reference that is not a file with a missing reference.
    MissingIfNotFile,
    /// Replace every reference with a missing reference, bundling no media.
    AllMissing,
}

/// Options for writing a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
    /// The directory a relative media path is resolved against.
    ///
    /// Without one, a relative path is resolved against the current
    /// directory.
    pub relative_media_base_dir: Option<PathBuf>,
    /// What to do with references that are not files.
    pub policy: MediaReferencePolicy,
    /// The indentation of `content.otio`, in spaces.
    pub indent: usize,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            relative_media_base_dir: None,
            policy: MediaReferencePolicy::ErrorIfNotFile,
            indent: otio_core::DEFAULT_INDENT,
        }
    }
}

/// Options for reading a bundle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReadOptions {
    /// Extract an `.otioz` into this directory, which must not exist yet.
    ///
    /// Without one, only the timeline is read out of the archive.
    pub extract_path: Option<PathBuf>,
    /// Rewrite the media references to absolute paths into the bundle.
    ///
    /// For an `.otioz` this only happens when it is also extracted, since
    /// otherwise there is nowhere on disk for the paths to point.
    pub absolute_media_reference_paths: bool,
}

/// Why a bundle could not be read or written.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The bundle, or a media file, could not be read. Upstream reports
    /// this as `FILE_OPEN_FAILED`.
    FileOpen(String),
    /// The bundle could not be written, or the timeline cannot be bundled
    /// under the policy given. Upstream reports this as `FILE_WRITE_FAILED`.
    FileWrite(String),
    /// The object to bundle is not a timeline.
    NotATimeline(String),
    /// The timeline could not be written or read as OTIO JSON.
    Core(otio_core::Error),
    /// A media reference's URL has a `%` escape upstream cannot decode.
    ///
    /// Upstream decodes each escape with `std::stoi`, which throws
    /// `std::invalid_argument` for `%zz`; the exception is not caught, so
    /// writing the bundle fails with it, and upstream's Python bindings
    /// raise it as `ValueError("stoi")`. Every policy fails, since upstream
    /// decodes the URL before it looks at the policy.
    InvalidEscape(InvalidEscape),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileOpen(details) | Self::FileWrite(details) => f.write_str(details),
            Self::NotATimeline(schema) => {
                write!(f, "a bundle holds a Timeline, not a {schema}")
            }
            Self::Core(error) => error.fmt(f),
            Self::InvalidEscape(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(error) => Some(error),
            Self::InvalidEscape(error) => Some(error),
            _ => None,
        }
    }
}

impl From<otio_core::Error> for Error {
    fn from(error: otio_core::Error) -> Self {
        Self::Core(error)
    }
}

impl From<InvalidEscape> for Error {
    fn from(error: InvalidEscape) -> Self {
        Self::InvalidEscape(error)
    }
}

/// The result of a bundle operation.
pub type Result<T> = std::result::Result<T, Error>;

/// A media file to put in the bundle: where it is, and its name in there.
///
/// Ordered by both, as upstream's `std::set` of them is, which is the order
/// the files are written in. The source is an `OsString` rather than a
/// `PathBuf` because a path orders by its components, while upstream's
/// `std::string` orders byte by byte, as an `OsString` does.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BundleFile {
    source_path: OsString,
    archive_name: String,
}

/// The name a media file gets inside the bundle, without the directory.
///
/// Upstream names it with the source file's own name, byte for byte. Where
/// those bytes are not UTF-8, as a `%E9` escape in a media URL can make
/// them, upstream's bundle is unsound: the zip entry is flagged as UTF-8
/// when it is not, which Python's `zipfile` refuses to open, and the
/// rewritten reference puts the raw bytes in `content.otio`, which is then
/// not valid JSON. So here each byte that is not part of a UTF-8 character
/// is spelled as the `%XX` escape the URL would have used, and the file is
/// bundled under that name. The reference names it, the zip entry is UTF-8
/// and the JSON valid, and upstream's reader finds the file too, since it
/// reads the bundled reference as a plain path.
fn bundled_file_name(source: &Path) -> String {
    let Some(name) = source.file_name() else {
        return String::new();
    };
    let bytes = name.as_encoded_bytes();
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for byte in chunk.invalid() {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The name a media file gets inside the bundle.
fn bundled_name(source: &Path) -> String {
    Path::new(MEDIA_DIR)
        .join(bundled_file_name(source))
        .to_string_lossy()
        .into_owned()
}

/// Records one media file, refusing one that would overwrite another.
fn register_bundle_file(
    source: &Path,
    relative_media_base_dir: Option<&Path>,
    paths: &mut BTreeMap<String, PathBuf>,
    out: &mut BTreeSet<BundleFile>,
) -> Result<()> {
    let name = bundled_file_name(source);
    let key = name.to_ascii_lowercase();
    // Upstream refuses two files of the same name in different directories.
    // Two different names in one directory can also meet here, when one is
    // spelled with the escapes `bundled_file_name` makes up for bytes that
    // are not UTF-8, and one would overwrite the other just the same.
    let clashes = |previous: &PathBuf| {
        previous.parent() != source.parent()
            || (previous.file_name() != source.file_name() && bundled_file_name(previous) == name)
    };
    if let Some(previous) = paths.get(&key).filter(|previous| clashes(previous)) {
        return Err(Error::FileWrite(format!(
            "media file '{}' would overwrite '{}'",
            source.display(),
            previous.display()
        )));
    }
    paths.insert(key, source.to_path_buf());

    let resolved = match relative_media_base_dir {
        Some(base) if source.is_relative() && !base.as_os_str().is_empty() => base.join(source),
        _ => source.to_path_buf(),
    };
    out.insert(BundleFile {
        source_path: resolved.into_os_string(),
        archive_name: bundled_name(source),
    });
    Ok(())
}

/// The file a decoded media path names.
///
/// Upstream hands the bytes to `std::filesystem::u8path`. On Unix that
/// keeps them as they are, whether they are UTF-8 or not, and so does this:
/// a `%E9` in a media URL names the file whose name has the byte `0xE9` in
/// it.
#[cfg(unix)]
fn file_path(bytes: Vec<u8>) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

/// The file a decoded media path names.
///
/// Upstream hands the bytes to `std::filesystem::u8path`, which on Windows
/// converts them from UTF-8 to UTF-16 and throws when they are not UTF-8.
/// A Windows path is UTF-16, so no file can be named by those bytes, and
/// writing the bundle fails here too, as a [`Error::FileWrite`].
#[cfg(not(unix))]
fn file_path(bytes: Vec<u8>) -> Result<PathBuf> {
    String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|error| {
            Error::FileWrite(format!(
                "media path '{}' is not UTF-8",
                String::from_utf8_lossy(error.as_bytes())
            ))
        })
}

/// A decoded media path as text, for a missing reference's metadata.
///
/// Upstream stores the bytes as they are, which then cannot be written as
/// JSON if they are not UTF-8; here they are replaced.
fn path_text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

/// A missing reference standing in for `reference`, saying why.
fn missing_reference(
    reference: &MediaReferenceData,
    reason: &str,
    original_target_url: Option<&str>,
) -> Node {
    let mut metadata = reference.base.metadata.clone();
    metadata.insert(
        "missing_reference_because".to_string(),
        Any::String(reason.to_string()),
    );
    if let Some(url) = original_target_url {
        metadata.insert(
            "original_target_url".to_string(),
            Any::String(url.to_string()),
        );
    }
    let mut media = MediaReferenceData::default();
    media.base.name = reference.base.name.clone();
    media.base.metadata = metadata;
    Node::MissingReference(MissingReference { media })
}

/// Applies the policy to every media reference in `timeline`, rewriting the
/// ones that will be bundled to point into it, and returns the files to add.
fn process_media_references(
    document: &mut Document,
    timeline: NodeId,
    relative_media_base_dir: Option<&Path>,
    policy: MediaReferencePolicy,
) -> Result<BTreeSet<BundleFile>> {
    let mut out = BTreeSet::new();
    // Every media file goes directly under media/, so remember each name to
    // catch two files that would land on the same one.
    let mut paths = BTreeMap::new();

    for clip in document.find_clips(timeline)? {
        let references = match document.try_get(clip)? {
            Node::Clip(clip) => clip.media_references.clone(),
            _ => continue,
        };

        let mut replacements = Vec::new();
        for (key, reference) in &references {
            let mut file: Option<Vec<u8>> = None;
            let mut original_target_url: Option<String> = None;

            match document.try_get_mut(*reference)? {
                Node::ExternalReference(external) => {
                    file = file_from_url(&external.target_url)?;
                    if let Some(found) = file
                        .clone()
                        .filter(|_| policy != MediaReferencePolicy::AllMissing)
                    {
                        let path = file_path(found)?;
                        register_bundle_file(&path, relative_media_base_dir, &mut paths, &mut out)?;
                        external.target_url = bundled_name(&path);
                    }
                    // Taken after the rewrite, as upstream takes it.
                    original_target_url = Some(external.target_url.clone());
                }
                Node::ImageSequenceReference(sequence) => {
                    if policy != MediaReferencePolicy::AllMissing {
                        let count = sequence.number_of_images_in_sequence();
                        // Upstream steps the image number by the frame step,
                        // which skips images when the step is more than one;
                        // that is reproduced. A step that is not positive
                        // would never end, so it stops after one image.
                        let step = sequence.frame_step.max(1);
                        let mut image = 0;
                        while image < count {
                            file = file_from_url(
                                sequence
                                    .target_url_for_image_number(image)
                                    .unwrap_or_default(),
                            )?;
                            if let Some(found) = file.clone() {
                                register_bundle_file(
                                    &file_path(found)?,
                                    relative_media_base_dir,
                                    &mut paths,
                                    &mut out,
                                )?;
                            }
                            image += step;
                        }
                        sequence.target_url_base = format!("{MEDIA_DIR}/");
                    }
                    original_target_url =
                        file_from_url(sequence.target_url_for_image_number(0).unwrap_or_default())?
                            .map(path_text);
                }
                _ => {}
            }

            let node = document.try_get(*reference)?;
            let media = match node {
                Node::MissingReference(_) => continue,
                Node::ExternalReference(r) => &r.media,
                Node::ImageSequenceReference(r) => &r.media,
                Node::GeneratorReference(r) => &r.media,
                Node::MediaReference(media) => media,
                _ => continue,
            };
            let reason = match policy {
                MediaReferencePolicy::ErrorIfNotFile => {
                    if file.is_none() {
                        return Err(Error::FileWrite(format!(
                            "media reference '{}' is not a file",
                            media.base.name
                        )));
                    }
                    continue;
                }
                MediaReferencePolicy::MissingIfNotFile => {
                    if file.is_some() {
                        continue;
                    }
                    "'missing_if_not_file' specified as the MediaReferencePolicy"
                }
                MediaReferencePolicy::AllMissing => {
                    "'all_missing' specified as the MediaReferencePolicy"
                }
            };
            let missing = missing_reference(media, reason, original_target_url.as_deref());
            replacements.push((key.clone(), missing));
        }

        for (key, missing) in replacements {
            let id = document.insert(missing);
            if let Node::Clip(clip) = document.try_get_mut(clip)? {
                clip.media_references.insert(key, id);
            }
        }
    }
    Ok(out)
}

/// The timeline to write, with its references rewritten, and its media.
struct Prepared {
    json: String,
    files: BTreeSet<BundleFile>,
}

/// Copies the timeline, applies the policy to the copy and writes it out.
fn prepare_bundle(
    document: &Document,
    timeline: NodeId,
    options: &WriteOptions,
) -> Result<Prepared> {
    let node = document.try_get(timeline)?;
    if !matches!(node, Node::Timeline(_)) {
        return Err(Error::NotATimeline(node.schema_name().to_string()));
    }
    let mut copy = document.clone();
    let clone = copy.deep_clone(timeline)?;
    let files = process_media_references(
        &mut copy,
        clone,
        options.relative_media_base_dir.as_deref(),
        options.policy,
    )?;
    let json = otio_core::to_string_pretty_from(&copy, clone, options.indent)?;
    Ok(Prepared { json, files })
}

/// Returns how many bytes a bundle of `timeline` would hold, uncompressed.
///
/// This checks the timeline against the policy exactly as writing does,
/// without writing anything, which makes it a way to find out whether a
/// bundle can be written as well as how much room it needs.
///
/// # Errors
///
/// Fails as [`write_otioz`] would, and if a media file cannot be read.
pub fn dry_run(document: &Document, timeline: NodeId, options: &WriteOptions) -> Result<u64> {
    let prepared = prepare_bundle(document, timeline, options)?;
    let mut total = (VERSION.len() + prepared.json.len()) as u64;
    for file in &prepared.files {
        let source = Path::new(&file.source_path);
        total += fs::metadata(source)
            .map_err(|error| Error::FileOpen(format!("{}: \"{}\"", error, source.display())))?
            .len();
    }
    Ok(total)
}

/// Writes `timeline` and its media to an `.otioz` archive at `path`.
///
/// # Errors
///
/// Fails if `path` already exists, if a media reference breaks the policy,
/// if two media files share a name, or if anything cannot be read or
/// written. A partly written archive is removed.
pub fn write_otioz(
    document: &Document,
    timeline: NodeId,
    path: &Path,
    options: &WriteOptions,
) -> Result<()> {
    if path.exists() {
        return Err(Error::FileWrite(format!(
            "output path '{}' already exists",
            path.display()
        )));
    }
    let prepared = prepare_bundle(document, timeline, options)?;

    let written = (|| -> std::io::Result<()> {
        let mut zip = zip::ZipWriter::create(path).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("cannot initialize zip writer for '{}'", path.display()),
            )
        })?;
        zip.add_deflated(VERSION_FILE, VERSION.as_bytes())?;
        zip.add_deflated(TIMELINE_FILE, prepared.json.as_bytes())?;
        for file in &prepared.files {
            zip.add_stored_file(&file.archive_name, Path::new(&file.source_path))
                .map_err(|error| {
                    std::io::Error::new(
                        error.kind(),
                        format!(
                            "cannot add '{}' to zip '{}'",
                            Path::new(&file.source_path).display(),
                            path.display()
                        ),
                    )
                })?;
        }
        zip.finish()?;
        Ok(())
    })();

    written.map_err(|error| {
        let _ = fs::remove_file(path);
        Error::FileWrite(format!("error writing '{}': {}", path.display(), error))
    })
}

/// Reads the timeline from an `.otioz` archive, extracting it if asked.
///
/// # Errors
///
/// Fails if `path` is not a file, if the extraction directory already
/// exists, if the archive is damaged or has no `content.otio`, if an entry
/// would land outside the extraction directory, or if the timeline is not
/// valid OTIO JSON. A partly extracted directory is removed.
pub fn read_otioz(path: &Path, options: &ReadOptions) -> Result<Document> {
    if !path.is_file() {
        return Err(Error::FileOpen(format!(
            "input '{}' is not a file",
            path.display()
        )));
    }
    if let Some(output) = options.extract_path.as_ref().filter(|p| p.exists()) {
        return Err(Error::FileWrite(format!(
            "output directory '{}' already exists",
            output.display()
        )));
    }

    let read = (|| -> std::result::Result<String, String> {
        let mut zip = zip::ZipReader::open(path)
            .map_err(|_| format!("cannot open zip file '{}'", path.display()))?;
        let entries = zip.entries().to_vec();
        let content = entries
            .iter()
            .find(|entry| entry.name == TIMELINE_FILE)
            .ok_or_else(|| format!("'{}' is missing content.otio", path.display()))?;
        let json = zip.read(content).map_err(|error| error.to_string())?;
        let json = String::from_utf8(json).map_err(|error| error.to_string())?;

        if let Some(output) = &options.extract_path {
            fs::create_dir_all(output).map_err(|error| error.to_string())?;
            for entry in &entries {
                let target = output.join(&entry.name);
                // Refuse an entry that would land outside the directory
                // ("zip slip"), whether by ".." or by an absolute name.
                if !is_path_safe(output, &target) {
                    return Err(format!(
                        "unsafe path '{}' in '{}'",
                        entry.name,
                        path.display()
                    ));
                }
                if entry.is_dir() {
                    fs::create_dir_all(&target).map_err(|error| error.to_string())?;
                } else {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                    }
                    zip.extract(entry, &target).map_err(|_| {
                        format!(
                            "cannot extract zip entry in '{}' to '{}'",
                            path.display(),
                            target.display()
                        )
                    })?;
                }
            }
        }
        Ok(json)
    })();

    let json = read.map_err(|error| {
        if let Some(output) = &options.extract_path {
            let _ = fs::remove_dir_all(output);
        }
        Error::FileOpen(format!("error reading '{}': {}", path.display(), error))
    })?;

    let mut document = otio_core::from_str(&json)?;
    if let Some(output) = options
        .extract_path
        .as_ref()
        .filter(|_| options.absolute_media_reference_paths)
    {
        rewrite_media_to_absolute(&mut document, output)?;
    }
    Ok(document)
}

/// Writes `timeline` and its media to an `.otiod` directory at `path`.
///
/// # Errors
///
/// Fails as [`write_otioz`] does. Unlike it, and like upstream, a partly
/// written directory is left where it is.
pub fn write_otiod(
    document: &Document,
    timeline: NodeId,
    path: &Path,
    options: &WriteOptions,
) -> Result<()> {
    if path.exists() {
        return Err(Error::FileWrite(format!(
            "output path '{}' already exists",
            path.display()
        )));
    }
    let prepared = prepare_bundle(document, timeline, options)?;

    let written = (|| -> std::io::Result<()> {
        fs::create_dir_all(path)?;
        fs::create_dir(path.join(MEDIA_DIR))?;
        fs::write(path.join(VERSION_FILE), VERSION)?;
        fs::write(path.join(TIMELINE_FILE), &prepared.json)?;
        for file in &prepared.files {
            let target = path.join(&file.archive_name);
            // `copy_file` refuses to overwrite, and so does this.
            if target.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("'{}' already exists", target.display()),
                ));
            }
            fs::copy(&file.source_path, &target).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("{}: \"{}\"", error, Path::new(&file.source_path).display()),
                )
            })?;
        }
        Ok(())
    })();

    written
        .map_err(|error| Error::FileWrite(format!("error writing '{}': {}", path.display(), error)))
}

/// Reads the timeline from an `.otiod` directory.
///
/// # Errors
///
/// Fails if `path` is not a directory, if it has no version or timeline
/// file, or if the timeline is not valid OTIO JSON.
pub fn read_otiod(path: &Path, options: &ReadOptions) -> Result<Document> {
    if !path.is_dir() {
        return Err(Error::FileOpen(format!(
            "input '{}' is not a directory",
            path.display()
        )));
    }
    if !path.join(VERSION_FILE).is_file() {
        return Err(Error::FileOpen(format!(
            "'{}' is missing a version file",
            path.display()
        )));
    }
    let timeline_path = path.join(TIMELINE_FILE);
    if !timeline_path.is_file() {
        return Err(Error::FileOpen(format!(
            "'{}' is missing a timeline file",
            path.display()
        )));
    }
    let json = fs::read_to_string(&timeline_path)
        .map_err(|error| Error::FileOpen(format!("{}: \"{}\"", error, timeline_path.display())))?;
    let mut document = otio_core::from_str(&json)?;
    if options.absolute_media_reference_paths {
        let root = timeline_path.parent().unwrap_or(path).to_path_buf();
        rewrite_media_to_absolute(&mut document, &root)?;
    }
    Ok(document)
}

/// Points every relative media reference in a read timeline at `root`.
fn rewrite_media_to_absolute(document: &mut Document, root: &Path) -> Result<()> {
    let Some(timeline) = document.root() else {
        return Ok(());
    };
    if !matches!(document.try_get(timeline)?, Node::Timeline(_)) {
        return Ok(());
    }
    for clip in document.find_clips(timeline)? {
        let references: Vec<NodeId> = match document.try_get(clip)? {
            Node::Clip(clip) => clip.media_references.values().copied().collect(),
            _ => continue,
        };
        for reference in references {
            match document.try_get_mut(reference)? {
                Node::ExternalReference(external) => {
                    let current = Path::new(&external.target_url);
                    if current.is_relative() {
                        external.target_url = lexically_normal(&root.join(current))
                            .to_string_lossy()
                            .into_owned();
                    }
                }
                Node::ImageSequenceReference(sequence) => {
                    let base = Path::new(&sequence.target_url_base);
                    if base.is_relative() {
                        let mut absolute = lexically_normal(&root.join(base))
                            .to_string_lossy()
                            .into_owned();
                        if !absolute.is_empty() && !absolute.ends_with('/') {
                            absolute.push('/');
                        }
                        sequence.target_url_base = absolute;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// `std::filesystem::path::lexically_normal`: `.` and `a/..` removed, without
/// looking at the disk.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    out.iter().collect()
}

/// `std::filesystem::weakly_canonical`: the longest existing prefix of
/// `path` resolved on disk, and the rest normalized lexically.
fn weakly_canonical(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let normal = lexically_normal(&absolute);
    let mut existing = normal.clone();
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = existing.canonicalize() {
            let mut out = resolved;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return lexically_normal(&out);
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return normal,
        }
    }
}

/// Whether extracting to `target` stays inside `destination`.
fn is_path_safe(destination: &Path, target: &Path) -> bool {
    weakly_canonical(target).starts_with(weakly_canonical(destination))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{is_path_safe, lexically_normal};

    #[test]
    fn normalizing_removes_dots_without_touching_the_disk() {
        assert_eq!(
            lexically_normal(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(lexically_normal(Path::new("/../a")), PathBuf::from("/a"));
        assert_eq!(lexically_normal(Path::new("../a")), PathBuf::from("../a"));
    }

    #[test]
    fn extraction_stays_inside_its_directory() {
        let destination = std::env::temp_dir().join("otio-bundle-slip-check");
        assert!(is_path_safe(&destination, &destination.join("media/a.mov")));
        assert!(!is_path_safe(&destination, &destination.join("../outside")));
        assert!(!is_path_safe(&destination, Path::new("/etc/passwd")));
    }
}

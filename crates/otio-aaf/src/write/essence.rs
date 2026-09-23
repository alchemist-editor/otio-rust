//! Embedding a clip's media: the `embed_essence` branch of upstream's
//! `aaf_sourceclip`, with `_copy_essence_for_clip` and the video
//! transcriber's `_import_essence_for_clip`.
//!
//! Upstream embeds by the suffix of the file a clip's media names. An `.aaf`
//! has the master mob the clip's MobID names copied out of it, with its
//! source mob and essence. A `.dnx` or `.wav` is imported, which only the
//! video transcriber can do, and only as a DNxHD stream: pyaaf2's
//! `import_dnxhd_essence` under the clip's master mob, behind its tape mob.
//! Anything else is refused. So is a file that is not there, before any of
//! that, with the path upstream turns the media's URL into.

use std::path::Path;

use aaf::write::{EssenceImport, ObjRef};
use otio_core::{Node, NodeId};

use super::track::{Kind, Track};
use super::{Fail, FileTranscriber, media_reference, py, unwritable};
use crate::error::Error;

impl FileTranscriber<'_> {
    /// The master mob and slot a clip's embedded media gives it: the
    /// `embed_essence` branch of `aaf_sourceclip`, or `None` where upstream
    /// takes the other branch, for a clip with no media.
    pub(crate) fn embedded_mastermob(
        &mut self,
        t: &Track,
        clip: NodeId,
    ) -> Result<Option<(ObjRef, ObjRef)>, Fail> {
        let media = self
            .document
            .try_get(media_reference(self.document, clip)?)?;
        let url = match media {
            Node::MissingReference(_) => return Ok(None),
            Node::ExternalReference(external) => external.target_url.clone(),
            other => {
                return Err(unwritable(format!(
                    "'{}' object has no attribute 'target_url'",
                    other.schema_name()
                )));
            }
        };
        let target_path = filepath_from_url(&url);
        let path = Path::new(&target_path);
        if !path.is_file() {
            return Err(Fail::Other(Error::MissingEssence { path: target_path }));
        }
        let suffix = suffix(&target_path);
        if suffix == ".aaf" {
            self.copy_essence_for_clip(clip, path, &target_path)
                .map(Some)
        } else if suffix == ".dnx" || suffix == ".wav" {
            match t.kind {
                Kind::Video => self.import_essence_for_clip(clip, path).map(Some),
                // The audio transcriber has no import of its own, and the
                // one it inherits returns nothing.
                Kind::Audio => Err(Fail::Other(Error::EmbedOnAudioTrack { path: target_path })),
            }
        } else {
            Err(Fail::Other(Error::Embed(format!(
                "Cannot embed media reference at: '{target_path}'.\
                 Only .aaf / .dnx / .wav files are supported.\
                 You can add logic to transcode your media for embedding by implementing a \
                 'otio_aaf_pre_write_transcribe' hook."
            ))))
        }
    }

    /// `_copy_essence_for_clip`: the master mob with the clip's MobID in the
    /// AAF at `path`, copied into the file with the source mob and essence of
    /// its first timeline slot, and its first timeline slot.
    fn copy_essence_for_clip(
        &mut self,
        clip: NodeId,
        path: &Path,
        shown: &str,
    ) -> Result<(ObjRef, ObjRef), Fail> {
        let mob_id = self.mob_key(clip)?.mob_id()?;
        let file = std::fs::File::open(path).map_err(|e| Fail::Other(Error::Io(e)))?;
        let read = |e: aaf::Error| Fail::Other(Error::Aaf(e));
        let mut src = aaf::Aaf::open(file).map_err(read)?;

        for src_master_mob in src.mobs_of("MasterMob").map_err(read)? {
            if src.mob_id(&src_master_mob).map_err(read)? != Some(mob_id) {
                continue;
            }

            // The source mob and essence behind the first timeline slot.
            let mut copied = false;
            for slot in src.slots(&src_master_mob).map_err(read)? {
                if !src.is_a(&slot, "TimelineMobSlot") {
                    continue;
                }
                let segment = src.child(&slot, "Segment").map_err(read)?.ok_or_else(|| {
                    unwritable("'NoneType' object has no attribute 'mob'".to_owned())
                })?;
                let source_id = match src.value(&segment, "SourceID") {
                    Ok(Some(aaf::Value::MobId(id))) => id,
                    _ => {
                        return Err(unwritable(format!(
                            "'{}' object has no attribute 'mob'",
                            src.class_name(&segment).unwrap_or("AAFObject")
                        )));
                    }
                };
                let src_source_mob = src.mob(source_id).map_err(read)?.ok_or_else(|| {
                    unwritable("'NoneType' object has no attribute 'essence'".to_owned())
                })?;
                let content = src.content().map_err(read)?;
                let mut essence = None;
                for data in src.children(&content, "EssenceData").map_err(read)? {
                    if matches!(src.value(&data, "MobID"), Ok(Some(aaf::Value::MobId(id))) if id == source_id)
                    {
                        essence = Some(data);
                        break;
                    }
                }
                let essence = essence.ok_or_else(|| {
                    unwritable("'NoneType' object has no attribute 'copy'".to_owned())
                })?;

                let essence_copy = self.f.copy_from(&mut src, &essence)?;
                let content = self.f.content()?;
                self.f.append(content, "EssenceData", essence_copy)?;
                let source_mob_copy = self.f.copy_from(&mut src, &src_source_mob)?;
                self.f.add_mob(source_mob_copy)?;
                copied = true;
                break;
            }
            if !copied {
                return Err(Fail::Other(Error::Embed(format!(
                    "No essence data to copy for MasterMob with ID '{mob_id}' in media \
                     reference AAF file: {shown}"
                ))));
            }

            let master_mob_copy = self.f.copy_from(&mut src, &src_master_mob)?;
            self.f.add_mob(master_mob_copy)?;
            for slot in self.f.get_objects(master_mob_copy, "Slots")? {
                if self.f.is_a(slot, "TimelineMobSlot") {
                    return Ok((master_mob_copy, slot));
                }
            }
            return Err(Fail::Other(Error::Embed(format!(
                "No TimelineMobSlot for MasterMob with ID '{mob_id}'."
            ))));
        }
        Err(Fail::Other(Error::Embed(format!(
            "No matching MasterMob with ID '{mob_id}' in media reference AAF file: {shown}"
        ))))
    }

    /// `VideoTrackTranscriber._import_essence_for_clip`: the clip's DNxHD
    /// stream imported under its master mob, behind a clip of its tape mob
    /// that starts where its media does.
    fn import_essence_for_clip(
        &mut self,
        clip: NodeId,
        path: &Path,
    ) -> Result<(ObjRef, ObjRef), Fail> {
        let available = self.media_available_range(clip)?;
        let start = py::float_int(available.start_time().value())?;
        let length = py::float_int(available.duration().value())?;
        // Python's `round`, which rounds halves to even.
        let edit_rate = py::float_int(available.duration().rate().round_ties_even())?;

        let mastermob = self.unique_mastermob(clip)?;
        let tape_mob = self.unique_tapemob(clip)?;
        let tape_clip = self.f.create_mob_source_clip(
            tape_mob,
            Kind::Video.master_mob_slot_id(),
            Some(start),
            None,
            None,
        )?;

        let mastermob_slot = self.f.import_dnxhd_essence(
            mastermob,
            path,
            EssenceImport {
                edit_rate: Some(edit_rate.into()),
                tape: Some(tape_clip),
                length: Some(length),
                offline: false,
            },
        )?;
        Ok((mastermob, mastermob_slot))
    }
}

/// `Path(...).suffix`: the final component's last dotted part, dot and all,
/// or nothing for a name that only starts with a dot.
fn suffix(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i..].to_owned(),
        _ => String::new(),
    }
}

/// OpenTimelineIO's `url_utils.filepath_from_url`, as it behaves on POSIX,
/// made into a path the way `pathlib.Path` makes one.
///
/// A `file:` URL gives its path, percent-decoded, with a Windows drive or a
/// host other than `localhost` kept as upstream keeps them; anything that is
/// not a URL is taken as a path, decoded the same way.
pub(crate) fn filepath_from_url(url: &str) -> String {
    let parts = urlparse(url);
    // Decoded once here, and again by `url2pathname`, as upstream does.
    let decoded = unquote(&parts.path);
    let filepath = unquote(&decoded);

    // A network location that is a drive holds the start of the path.
    if is_windows_drive(&parts.netloc) {
        return pure_path(&format!("{}{decoded}", parts.netloc));
    }
    // A drive in the first part of the path, or the second, starts it, and
    // what comes before goes. (Upstream keeps the drive only on Windows,
    // where it is the first part's `drive`; on POSIX it has none.)
    let components = path_parts(&filepath);
    if components.first().is_some_and(|c| is_windows_drive(c)) {
        return pure_path(&components[1..].join("/"));
    }
    if components.get(1).is_some_and(|c| is_windows_drive(c)) {
        return pure_path(&components[1..].join("/"));
    }
    // Any other host names a share.
    if !parts.netloc.is_empty() && parts.netloc != "localhost" {
        return pure_path(&format!("//{}{decoded}", parts.netloc));
    }
    pure_path(&filepath)
}

/// `pathlib.PurePosixPath(text).parts`: the root, if there is one, then each
/// name.
fn path_parts(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    if text.starts_with("//") && !text.starts_with("///") {
        parts.push("//");
    } else if text.starts_with('/') {
        parts.push("/");
    }
    parts.extend(text.split('/').filter(|p| !p.is_empty() && *p != "."));
    parts
}

/// The parts of a URL `filepath_from_url` uses, as `urllib.parse.urlparse`
/// splits them.
struct UrlParts {
    netloc: String,
    path: String,
}

/// `urllib.parse.urlparse`, for the scheme, network location and path.
fn urlparse(url: &str) -> UrlParts {
    let mut rest = url;
    let mut scheme = String::new();
    if let Some(i) = rest.find(':') {
        let candidate = &rest[..i];
        if candidate
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            scheme = candidate.to_ascii_lowercase();
            rest = &rest[i + 1..];
        }
    }
    let mut netloc = String::new();
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = after[..end].to_owned();
        rest = &after[end..];
    }
    if let Some(i) = rest.find('#') {
        rest = &rest[..i];
    }
    if let Some(i) = rest.find('?') {
        rest = &rest[..i];
    }
    // `urlparse` takes `;params` off the last segment for the schemes that
    // have them, the empty one among them.
    const USES_PARAMS: [&str; 15] = [
        "", "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp", "rtspu", "sip",
        "sips", "mms", "sftp", "tel",
    ];
    if USES_PARAMS.contains(&scheme.as_str()) {
        let start = rest.rfind('/').unwrap_or(0);
        if let Some(i) = rest[start..].find(';') {
            rest = &rest[..start + i];
        }
    }
    UrlParts {
        netloc,
        path: rest.to_owned(),
    }
}

/// `urllib.parse.unquote`: `%xx` escapes decoded as UTF-8, with anything
/// that does not decode replaced.
fn unquote(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| char::from(b).to_digit(16);
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                // Both digits are below 16, so the byte fits.
                out.push(u8::try_from(high * 16 + low).unwrap_or(0));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `pathlib.PureWindowsPath(text).drive` is a drive letter, as it is
/// for `C:`.
fn is_windows_drive(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// `str(pathlib.PurePosixPath(text))`: repeated slashes and `.` segments
/// dropped, and a trailing slash, with two leading slashes kept as POSIX
/// keeps them.
fn pure_path(text: &str) -> String {
    if text.is_empty() {
        return ".".to_owned();
    }
    let root = if text.starts_with("//") && !text.starts_with("///") {
        "//"
    } else if text.starts_with('/') {
        "/"
    } else {
        ""
    };
    let parts: Vec<&str> = text
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let joined = parts.join("/");
    if root.is_empty() && joined.is_empty() {
        ".".to_owned()
    } else {
        format!("{root}{joined}")
    }
}

#[cfg(test)]
mod tests {
    use super::{filepath_from_url, suffix};

    #[test]
    fn urls_and_paths_become_paths_as_upstream_makes_them() {
        for (url, path) in [
            ("../aaf/tests/data/tone.wav", "../aaf/tests/data/tone.wav"),
            ("file:///media/A001.dnx", "/media/A001.dnx"),
            ("file://localhost/media/A%20001.dnx", "/media/A 001.dnx"),
            ("file:///C:/media/clip.aaf", "C:/media/clip.aaf"),
            ("file://C:/media/clip.aaf", "C:/media/clip.aaf"),
            ("file://server/share/clip.aaf", "//server/share/clip.aaf"),
            ("./media//clip.dnx", "media/clip.dnx"),
            ("media/clip;v2.dnx", "media/clip"),
            ("file:///media/clip;v2.dnx", "/media/clip;v2.dnx"),
            ("file:C:/x.aaf", "x.aaf"),
            ("a/C:/x.dnx", "C:/x.dnx"),
            ("C:/m/a.dnx", "/m/a.dnx"),
            ("//two/x.dnx", "//two/x.dnx"),
            ("///three/x.dnx", "/three/x.dnx"),
            ("media/a b.dnx?q#f", "media/a b.dnx"),
            ("a%2520b.dnx", "a b.dnx"),
            ("x%zz.dnx", "x%zz.dnx"),
            ("%e2%82%ac.dnx", "\u{20ac}.dnx"),
            ("%ff.dnx", "\u{fffd}.dnx"),
        ] {
            assert_eq!(filepath_from_url(url), path, "{url}");
        }
    }

    #[test]
    fn suffixes_are_pathlibs() {
        assert_eq!(suffix("a/b.tar.aaf"), ".aaf");
        assert_eq!(suffix("a/.aaf"), "");
        assert_eq!(suffix("a/b."), "");
        assert_eq!(suffix("a/b.DNX"), ".DNX");
        assert_eq!(suffix("a.b/c"), "");
    }
}

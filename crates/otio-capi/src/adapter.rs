//! Reading and writing the interchange formats, for C.
//!
//! One pair of calls covers every format: pick one with [`OtioFormat`] and
//! hand over bytes or a path. The options each format accepts are gathered
//! into [`OtioReadOptions`] and [`OtioWriteOptions`], where a null pointer
//! means "do the usual thing" and a field that another format does not use is
//! ignored.
//!
//! Every field is named so that zero is upstream's default, which is why the
//! AAF reading options are spelled as what turning a pass *off* does: a
//! caller that zeroes the struct, or a binding whose structs start zeroed,
//! reads an AAF the way upstream's adapter does.

use std::ffi::c_char;

use otio_aaf::Aaf;
use otio_adapter::{Adapter, Error as AdapterError};
use otio_ale::Ale;
use otio_cmx3600::{Cmx3600, Style};
use otio_core::Document;
use otio_fcp7::Fcp7Xml;
use otio_fcpx::FcpxXml;

use crate::buffer::OtioBuffer;
use crate::handle::{OtioDocument, bytes, document, optional_text, text, write_out};
use crate::status::{Fault, OtioStatus, Outcome, guard};

/// A file format this library reads and writes.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioFormat {
    /// OpenTimelineIO's own JSON, the `.otio` file.
    OtioJson = 0,
    /// Avid Log Exchange, the `.ale` file.
    Ale = 1,
    /// CMX 3600 EDL, the `.edl` file.
    Cmx3600 = 2,
    /// Final Cut Pro 7 XML, the `.xml` file.
    Fcp7Xml = 3,
    /// Final Cut Pro X XML, the `.fcpxml` file.
    FcpxXml = 4,
    /// The Advanced Authoring Format, the `.aaf` file.
    Aaf = 5,
}

/// Which system's conventions an EDL is written for.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtioEdlStyle {
    /// Avid Media Composer.
    Avid = 0,
    /// Nucoda.
    Nucoda = 1,
    /// Adobe Premiere Pro.
    Premiere = 2,
}

impl From<OtioEdlStyle> for Style {
    fn from(style: OtioEdlStyle) -> Self {
        match style {
            OtioEdlStyle::Avid => Self::Avid,
            OtioEdlStyle::Nucoda => Self::Nucoda,
            OtioEdlStyle::Premiere => Self::Premiere,
        }
    }
}

/// What to do while reading a file.
///
/// A field a format does not use is ignored, so one of these can be filled in
/// once and used for several. Pass a null pointer for the usual behaviour.
///
/// This struct may gain fields before the ABI is declared stable; a caller
/// that zeroes it before filling in what it cares about will keep working.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioReadOptions {
    /// The rate timecode is read at. Zero means the format's own default.
    ///
    /// An EDL says nothing about its rate, so this is the only thing that
    /// does. An ALE's heading may state one, and it wins if it does.
    pub rate: f64,
    /// ALE: the column a clip takes its name from. Null means `"Name"`.
    pub name_column: *const c_char,
    /// EDL: accept a file whose record timecode does not add up.
    pub ignore_timecode_mismatch: bool,
    /// AAF: keep the nesting AAF has and OTIO does not need.
    ///
    /// This is upstream's `simplify=False`: a track per slot, a stack per
    /// nested composition, a track per sequence inside it.
    pub aaf_keep_nesting: bool,
    /// AAF: leave each marker on the slot that carries it.
    ///
    /// This is upstream's `attach_markers=False`: the markers keep their
    /// positions in those tracks' time rather than moving onto the items
    /// they point at.
    pub aaf_markers_on_slots: bool,
    /// AAF: record each keyframed effect parameter's value at every frame of
    /// its effect, as upstream's `bake_keyframed_properties=True` does.
    pub aaf_bake_keyframes: bool,
}

/// What to do while writing a file.
///
/// As [`OtioReadOptions`]: a null pointer means the usual behaviour, and a
/// field another format does not use is ignored.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OtioWriteOptions {
    /// The rate timecode is written at. Zero takes it from the document.
    pub rate: f64,
    /// EDL: which system's conventions to write for.
    pub edl_style: OtioEdlStyle,
    /// EDL: how many characters to pad or truncate a reel name to.
    ///
    /// Zero writes it in full, which keeps the information but which most
    /// systems will not read.
    pub reelname_len: usize,
    /// ALE: the `VIDEO_FORMAT` to state in the heading. Null keeps the
    /// document's own.
    pub video_format: *const c_char,
    /// AAF: look for a clip's MobID in the AAF file its media names before
    /// looking in its metadata.
    pub aaf_prefer_file_mob_id: bool,
    /// AAF: make up a MobID for a clip that has none anywhere.
    ///
    /// Off, such a clip stops the write, since a made-up MobID links the
    /// clip to no media Media Composer knows.
    pub aaf_use_empty_mob_ids: bool,
    /// AAF: embed each clip's media in the file.
    pub aaf_embed_essence: bool,
    /// AAF: give each master clip an edge code slot carrying its media's
    /// range, which Media Composer shows as Frame Count Start and End.
    pub aaf_create_edgecode: bool,
    /// AAF: whom a marker with no user of its own is credited to.
    ///
    /// Null finds the user as upstream does, from `LOGNAME`, `USER`, `LNAME`
    /// or `USERNAME`; if none is set, a timeline with such a marker cannot
    /// be written.
    pub aaf_user: *const c_char,
    /// AAF: the time the file records as when it and each thing in it was
    /// made, in seconds since the Unix epoch. Zero reads the system clock.
    ///
    /// WebAssembly has no clock of its own, so a host there passes the time.
    pub aaf_time: i64,
    /// AAF: seeds the identifiers the file gives itself and each new clip.
    /// Zero draws fresh ones.
    ///
    /// The same seed, time and timeline write the same file. WebAssembly has
    /// no randomness of its own, so a host there passes some.
    pub aaf_id_seed: u64,
}

/// Returns the defaults, for a caller that wants to change one field.
#[unsafe(no_mangle)]
pub extern "C" fn otio_read_options_default() -> OtioReadOptions {
    OtioReadOptions {
        rate: 0.0,
        name_column: std::ptr::null(),
        ignore_timecode_mismatch: false,
        aaf_keep_nesting: false,
        aaf_markers_on_slots: false,
        aaf_bake_keyframes: false,
    }
}

/// Returns the defaults, for a caller that wants to change one field.
#[unsafe(no_mangle)]
pub extern "C" fn otio_write_options_default() -> OtioWriteOptions {
    OtioWriteOptions {
        rate: 0.0,
        edl_style: OtioEdlStyle::Avid,
        reelname_len: otio_cmx3600::DEFAULT_REELNAME_LEN,
        video_format: std::ptr::null(),
        aaf_prefer_file_mob_id: false,
        aaf_use_empty_mob_ids: false,
        aaf_embed_essence: false,
        aaf_create_edgecode: false,
        aaf_user: std::ptr::null(),
        aaf_time: 0,
        aaf_id_seed: 0,
    }
}

/// Returns an adapter's name, as upstream's plugin manifest spells it.
///
/// The string is static and needs no freeing.
#[unsafe(no_mangle)]
pub extern "C" fn otio_format_name(format: OtioFormat) -> *const c_char {
    let name: &'static str = match format {
        OtioFormat::OtioJson => "otio_json\0",
        OtioFormat::Ale => "ale\0",
        OtioFormat::Cmx3600 => "cmx_3600\0",
        OtioFormat::Fcp7Xml => "fcp_xml\0",
        OtioFormat::FcpxXml => "fcpx_xml\0",
        OtioFormat::Aaf => "AAF\0",
    };
    name.as_ptr().cast::<c_char>()
}

/// Returns the format that claims a filename suffix, such as `"edl"`.
///
/// The suffix is matched without its dot and without regard to case. Reports
/// `OTIO_STATUS_NO_VALUE` for a suffix no format claims.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_format_from_suffix(
    suffix: *const c_char,
    out_format: *mut OtioFormat,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let suffix = unsafe { text(suffix, "suffix") }?;
        let suffix = suffix.trim_start_matches('.').to_ascii_lowercase();
        let format = match suffix.as_str() {
            "otio" => OtioFormat::OtioJson,
            "ale" => OtioFormat::Ale,
            "edl" => OtioFormat::Cmx3600,
            "xml" => OtioFormat::Fcp7Xml,
            "fcpxml" => OtioFormat::FcpxXml,
            "aaf" => OtioFormat::Aaf,
            other => return Err(Fault::no_value(&format!("a format for '.{other}'"))),
        };
        unsafe { write_out(out_format, format, "out_format") }
    })
}

/// The options a read was asked for, checked and owned.
struct Reading {
    rate: f64,
    name_column: String,
    ignore_timecode_mismatch: bool,
    aaf: otio_aaf::ReadOptions,
}

/// Reads the options a caller passed, or the defaults if it passed none.
unsafe fn read_options(options: *const OtioReadOptions) -> Outcome<Reading> {
    let options = if options.is_null() {
        otio_read_options_default()
    } else {
        unsafe { *options }
    };
    let name_column = unsafe { optional_text(options.name_column, "name_column") }?
        .unwrap_or(otio_ale::DEFAULT_NAME_COLUMN)
        .to_string();
    Ok(Reading {
        rate: options.rate,
        name_column,
        ignore_timecode_mismatch: options.ignore_timecode_mismatch,
        aaf: otio_aaf::ReadOptions::new()
            .with_simplify(!options.aaf_keep_nesting)
            .with_attach_markers(!options.aaf_markers_on_slots)
            .with_bake_keyframed_properties(options.aaf_bake_keyframes),
    })
}

/// Reads a document out of the bytes of a file in some format.
fn read(format: OtioFormat, input: &[u8], options: Reading) -> Outcome<Document> {
    let Reading {
        rate,
        name_column,
        ignore_timecode_mismatch,
        aaf,
    } = options;
    let document = match format {
        OtioFormat::OtioJson => {
            let text = std::str::from_utf8(input)
                .map_err(|error| Fault::new(OtioStatus::InvalidUtf8, error.to_string()))?;
            otio_core::from_str(text)?
        }
        OtioFormat::Ale => Ale::read_from_bytes(
            input,
            &otio_ale::ReadOptions {
                fps: if rate > 0.0 {
                    rate
                } else {
                    otio_ale::DEFAULT_FPS
                },
                name_column,
            },
        )?,
        OtioFormat::Cmx3600 => Cmx3600::read_from_bytes(
            input,
            &otio_cmx3600::ReadOptions {
                rate: if rate > 0.0 {
                    rate
                } else {
                    otio_cmx3600::DEFAULT_RATE
                },
                ignore_timecode_mismatch,
            },
        )?,
        OtioFormat::Fcp7Xml => Fcp7Xml::read_from_bytes(input, &otio_fcp7::ReadOptions::default())?,
        OtioFormat::FcpxXml => FcpxXml::read_from_bytes(input, &otio_fcpx::ReadOptions::default())?,
        OtioFormat::Aaf => Aaf::read_from_bytes(input, &aaf)?,
    };
    Ok(document)
}

/// Writes a document as the bytes of a file in some format.
unsafe fn write(
    format: OtioFormat,
    source: &Document,
    options: *const OtioWriteOptions,
) -> Outcome<Vec<u8>> {
    let options = if options.is_null() {
        otio_write_options_default()
    } else {
        unsafe { *options }
    };
    let video_format = unsafe { optional_text(options.video_format, "video_format") }?;

    let bytes = match format {
        OtioFormat::OtioJson => otio_core::to_string(source)?.into_bytes(),
        OtioFormat::Ale => Ale::write_to_bytes(
            source,
            &otio_ale::WriteOptions {
                columns: None,
                fps: (options.rate > 0.0).then_some(options.rate),
                video_format: video_format.map(str::to_string),
            },
        )?,
        OtioFormat::Cmx3600 => Cmx3600::write_to_bytes(
            source,
            &otio_cmx3600::WriteOptions {
                rate: (options.rate > 0.0).then_some(options.rate),
                style: options.edl_style.into(),
                reelname_len: (options.reelname_len > 0).then_some(options.reelname_len),
            },
        )?,
        OtioFormat::Fcp7Xml => {
            Fcp7Xml::write_to_bytes(source, &otio_fcp7::WriteOptions::default())?
        }
        OtioFormat::FcpxXml => {
            FcpxXml::write_to_bytes(source, &otio_fcpx::WriteOptions::default())?
        }
        OtioFormat::Aaf => {
            let user = unsafe { optional_text(options.aaf_user, "aaf_user") }?;
            Aaf::write_to_bytes(source, &aaf_write_options(&options, user))?
        }
    };
    Ok(bytes)
}

/// The `otio-aaf` options a caller's write options ask for.
fn aaf_write_options(options: &OtioWriteOptions, user: Option<&str>) -> otio_aaf::WriteOptions {
    use aaf::write::{Clock, FixedClock, RandomIds, SystemClock, Timestamp};

    let mut aaf = otio_aaf::WriteOptions::new()
        .with_prefer_file_mob_id(options.aaf_prefer_file_mob_id)
        .with_use_empty_mob_ids(options.aaf_use_empty_mob_ids)
        .with_embed_essence(options.aaf_embed_essence)
        .with_create_edgecode(options.aaf_create_edgecode);
    if let Some(user) = user {
        aaf = aaf.with_user(user);
    }
    if options.aaf_time != 0 || options.aaf_id_seed != 0 {
        let clock: Box<dyn Clock + Send> = if options.aaf_time == 0 {
            Box::new(SystemClock)
        } else {
            Box::new(FixedClock::new(Timestamp::from_unix(options.aaf_time)))
        };
        let ids = if options.aaf_id_seed == 0 {
            RandomIds::new()
        } else {
            RandomIds::from_seed(options.aaf_id_seed)
        };
        aaf = aaf.with_sources(otio_aaf::Sources::new(BoxedClock(clock), ids));
    }
    aaf
}

/// A clock chosen at run time, as [`otio_aaf::Sources`] takes one.
struct BoxedClock(Box<dyn aaf::write::Clock + Send>);

impl aaf::write::Clock for BoxedClock {
    fn now(&mut self) -> aaf::write::Timestamp {
        self.0.now()
    }
}

/// Reads a document from the bytes of a file in some format.
///
/// `options` may be null for the format's usual behaviour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_read_from_bytes(
    format: OtioFormat,
    data: *const u8,
    len: usize,
    options: *const OtioReadOptions,
    out_document: *mut *mut OtioDocument,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let options = unsafe { read_options(options) }?;
        let input = unsafe { bytes(data, len, "data") }?;
        let parsed = read(format, input, options)?;
        let owned = Box::into_raw(Box::new(OtioDocument(parsed)));
        unsafe { write_out(out_document, owned, "out_document") }
    })
}

/// Reads a document from a file on disk in some format.
///
/// An AAF is read where it lies, seeking around the file, rather than copied
/// into memory first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_read_from_file(
    format: OtioFormat,
    path: *const c_char,
    options: *const OtioReadOptions,
    out_document: *mut *mut OtioDocument,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let options = unsafe { read_options(options) }?;
        let path = unsafe { text(path, "path") }?;
        let parsed = if format == OtioFormat::Aaf {
            Aaf::read_from_file(path, &options.aaf)?
        } else {
            let input =
                std::fs::read(path).map_err(|error| Fault::from(AdapterError::Io(error)))?;
            read(format, &input, options)?
        };
        let owned = Box::into_raw(Box::new(OtioDocument(parsed)));
        unsafe { write_out(out_document, owned, "out_document") }
    })
}

/// Writes a document as the bytes of a file in some format.
///
/// The buffer is NUL-terminated, so a text format's output can be used as a C
/// string; `len` is what matters for a binary one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_write_to_bytes(
    format: OtioFormat,
    source: *const OtioDocument,
    options: *const OtioWriteOptions,
    out_bytes: *mut OtioBuffer,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let source = unsafe { document(source) }?;
        let written = unsafe { write(format, source, options) }?;
        unsafe { write_out(out_bytes, OtioBuffer::from_bytes(&written), "out_bytes") }
    })
}

/// Writes a document to a file on disk in some format.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn otio_write_to_file(
    format: OtioFormat,
    source: *const OtioDocument,
    path: *const c_char,
    options: *const OtioWriteOptions,
    out_error: *mut OtioBuffer,
) -> OtioStatus {
    guard(out_error, || {
        let path = unsafe { text(path, "path") }?;
        let source = unsafe { document(source) }?;
        let written = unsafe { write(format, source, options) }?;
        std::fs::write(path, written).map_err(|error| Fault::from(AdapterError::Io(error)))
    })
}

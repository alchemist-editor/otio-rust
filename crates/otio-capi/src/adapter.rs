//! Reading and writing the interchange formats, for C.
//!
//! One pair of calls covers every format: pick one with [`OtioFormat`] and
//! hand over bytes or a path. The options each format accepts are gathered
//! into [`OtioReadOptions`] and [`OtioWriteOptions`], where a null pointer
//! means "do the usual thing" and a field that another format does not use is
//! ignored.
//!
//! AAF is not here yet. Reading and writing one are implemented in the
//! `otio-aaf` crate; adding it to this enum is a handful of lines, tracked
//! by issue #59.

use std::ffi::c_char;

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
}

/// Returns the defaults, for a caller that wants to change one field.
#[unsafe(no_mangle)]
pub extern "C" fn otio_read_options_default() -> OtioReadOptions {
    OtioReadOptions {
        rate: 0.0,
        name_column: std::ptr::null(),
        ignore_timecode_mismatch: false,
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
            other => return Err(Fault::no_value(&format!("a format for '.{other}'"))),
        };
        unsafe { write_out(out_format, format, "out_format") }
    })
}

/// Reads the options a caller passed, or the defaults if it passed none.
unsafe fn read_options(options: *const OtioReadOptions) -> Outcome<(f64, String, bool)> {
    if options.is_null() {
        return Ok((0.0, otio_ale::DEFAULT_NAME_COLUMN.to_string(), false));
    }
    let options = unsafe { *options };
    let name_column = unsafe { optional_text(options.name_column, "name_column") }?
        .unwrap_or(otio_ale::DEFAULT_NAME_COLUMN)
        .to_string();
    Ok((options.rate, name_column, options.ignore_timecode_mismatch))
}

/// Reads a document out of the bytes of a file in some format.
fn read(format: OtioFormat, input: &[u8], options: (f64, String, bool)) -> Outcome<Document> {
    let (rate, name_column, ignore_timecode_mismatch) = options;
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
    };
    Ok(bytes)
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
        let input = std::fs::read(path).map_err(|error| Fault::from(AdapterError::Io(error)))?;
        let parsed = read(format, &input, options)?;
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

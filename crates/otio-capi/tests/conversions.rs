//! Every sample file converted to every other interchange format, through the
//! C ABI, and read back.
//!
//! This is what a host such as Alchemist's native editor does when it offers
//! "open an EDL, save it as FCPXML": read one format, hand the document on as
//! OTIO JSON, write another format, and expect a file it can open again. Each
//! adapter's own tests prove it reads and writes its own format; what they
//! cannot see is one adapter tripping over what another one produced. Five of
//! the conversions here failed before issue #98, as they do upstream too.

use std::ffi::{CString, c_char};
use std::path::PathBuf;
use std::ptr;

use otio::{
    OtioBuffer, OtioDocument, OtioFormat, OtioReadOptions, OtioStatus, otio_buffer_free,
    otio_document_free, otio_document_from_json, otio_document_to_json, otio_read_from_bytes,
    otio_read_options_default, otio_write_to_bytes,
};

/// A sample file, the format it is in, and the rate an EDL is read at.
struct Sample {
    crate_dir: &'static str,
    name: &'static str,
    format: OtioFormat,
    rate: f64,
}

const SAMPLES: &[Sample] = &[
    Sample {
        crate_dir: "otio-cmx3600",
        name: "dissolve_test.edl",
        format: OtioFormat::Cmx3600,
        rate: 24.0,
    },
    Sample {
        crate_dir: "otio-cmx3600",
        name: "speed_effects_small.edl",
        format: OtioFormat::Cmx3600,
        rate: 24.0,
    },
    Sample {
        crate_dir: "otio-cmx3600",
        name: "multi_audio.edl",
        format: OtioFormat::Cmx3600,
        rate: 24.0,
    },
    Sample {
        crate_dir: "otio-cmx3600",
        name: "gap_test.edl",
        format: OtioFormat::Cmx3600,
        rate: 24.0,
    },
    Sample {
        crate_dir: "otio-fcp7",
        name: "premiere_example.xml",
        format: OtioFormat::Fcp7Xml,
        rate: 30.0,
    },
    Sample {
        crate_dir: "otio-fcpx",
        name: "fcpx_project.fcpxml",
        format: OtioFormat::FcpxXml,
        rate: 30.0,
    },
];

/// The formats a conversion is written as. EDL holds one video track, so a
/// host writes it from a single track rather than from a whole timeline, and
/// it is left to the EDL adapter's own tests.
const TARGETS: &[OtioFormat] = &[
    OtioFormat::OtioJson,
    OtioFormat::Fcp7Xml,
    OtioFormat::FcpxXml,
];

fn sample_bytes(sample: &Sample) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(sample.crate_dir)
        .join("tests/data")
        .join(sample.name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Takes the text out of a buffer and frees it.
fn take(buffer: OtioBuffer) -> Vec<u8> {
    let bytes = if buffer.data.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(buffer.data.cast::<u8>(), buffer.len) }.to_vec()
    };
    unsafe { otio_buffer_free(buffer) };
    bytes
}

fn empty() -> OtioBuffer {
    OtioBuffer {
        data: ptr::null_mut(),
        len: 0,
    }
}

/// Turns a status and its error buffer into a `Result`.
fn check(status: OtioStatus, error: OtioBuffer) -> Result<(), String> {
    let message = String::from_utf8_lossy(&take(error)).into_owned();
    if status == OtioStatus::Ok {
        Ok(())
    } else {
        Err(format!("{status:?}: {message}"))
    }
}

fn read(format: OtioFormat, bytes: &[u8], rate: f64) -> Result<*mut OtioDocument, String> {
    let options = OtioReadOptions {
        rate,
        ..otio_read_options_default()
    };
    let mut document = ptr::null_mut();
    let mut error = empty();
    let status = unsafe {
        otio_read_from_bytes(
            format,
            bytes.as_ptr(),
            bytes.len(),
            &raw const options,
            &raw mut document,
            &raw mut error,
        )
    };
    check(status, error).map(|()| document)
}

fn to_json(document: *const OtioDocument) -> String {
    let mut json = empty();
    let mut error = empty();
    let status = unsafe { otio_document_to_json(document, 4, &raw mut json, &raw mut error) };
    check(status, error).expect("a document serializes");
    String::from_utf8(take(json)).expect("JSON is UTF-8")
}

fn from_json(json: &str) -> *mut OtioDocument {
    let json = CString::new(json).expect("JSON has no NUL");
    let mut document = ptr::null_mut();
    let mut error = empty();
    let status = unsafe {
        otio_document_from_json(
            json.as_ptr().cast::<c_char>(),
            &raw mut document,
            &raw mut error,
        )
    };
    check(status, error).expect("JSON this library wrote reads back");
    document
}

fn write(format: OtioFormat, document: *const OtioDocument) -> Result<Vec<u8>, String> {
    let mut bytes = empty();
    let mut error = empty();
    let status = unsafe {
        otio_write_to_bytes(
            format,
            document,
            ptr::null(),
            &raw mut bytes,
            &raw mut error,
        )
    };
    check(status, error).map(|()| take(bytes))
}

/// Counts the clips in a document's JSON.
fn clips(json: &str) -> usize {
    json.matches("\"OTIO_SCHEMA\": \"Clip.").count()
}

/// Reads a sample, passes it on as JSON, writes it as `target` and reads that
/// back. Returns the clip counts before and after.
fn convert(sample: &Sample, target: OtioFormat) -> Result<(usize, usize), String> {
    let source = read(sample.format, &sample_bytes(sample), sample.rate)?;
    let json = to_json(source);
    unsafe { otio_document_free(source) };

    let handed_on = from_json(&json);
    let written = write(target, handed_on);
    unsafe { otio_document_free(handed_on) };
    let written = written.map_err(|error| format!("write: {error}"))?;

    let back =
        read(target, &written, sample.rate).map_err(|error| format!("read back: {error}"))?;
    let back_json = to_json(back);
    unsafe { otio_document_free(back) };
    Ok((clips(&json), clips(&back_json)))
}

#[test]
fn every_sample_converts_to_every_other_format_and_reads_back() {
    let mut failures = Vec::new();
    for sample in SAMPLES {
        for &target in TARGETS {
            if target == sample.format {
                continue;
            }
            match convert(sample, target) {
                Ok((before, after)) if before > 0 && after == 0 => failures.push(format!(
                    "{} -> {target:?}: none of {before} clips survived",
                    sample.name
                )),
                Ok((before, after)) if target == OtioFormat::OtioJson && after != before => {
                    failures.push(format!(
                        "{} -> {target:?}: {after} of {before} clips survived",
                        sample.name
                    ));
                }
                Ok(_) => {}
                Err(error) => failures.push(format!("{} -> {target:?}: {error}", sample.name)),
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

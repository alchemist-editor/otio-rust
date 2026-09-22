//! Reading the C spelling of the enum constants out of the header.
//!
//! The Rust source is where this interface is defined, and `tests/header.rs`
//! in `otio-capi` already holds the header to it for every function. One
//! thing the Rust source does not state, though, is what a variant is called
//! in C: `OtioValueKind::Bool` is `OTIO_VALUE_BOOL`, not
//! `OTIO_VALUE_KIND_BOOL`, and no rule spelled from the Rust name gets every
//! case right.
//!
//! So the spelling comes from the header, and the classifier checks the two
//! agree on how many variants there are and what each is worth. A header that
//! drifts from the source fails here as well as there.

use std::collections::BTreeMap;

use crate::scan::{ScanError, Scanned};

/// Reads every `typedef enum` in the header as its constants and their
/// values, in declaration order.
///
/// # Errors
///
/// Fails on a constant with no explicit value, which the C ABI's own
/// convention requires so that the numbers are part of the reviewed diff.
pub fn enum_constants(header: &str) -> Scanned<BTreeMap<String, Vec<(String, i64)>>> {
    let mut found = BTreeMap::new();
    let mut current: Option<(String, Vec<(String, i64)>)> = None;

    for (number, line) in header.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("typedef enum ") {
            let name = rest.trim_end_matches('{').trim().to_string();
            current = Some((name, Vec::new()));
            continue;
        }
        let Some((name, constants)) = current.as_mut() else {
            continue;
        };
        if trimmed.starts_with('}') {
            let (name, constants) = current.take().expect("just checked");
            found.insert(name, constants);
            continue;
        }
        if !trimmed.starts_with("OTIO_") {
            continue;
        }
        let entry = trimmed.trim_end_matches(',');
        let (constant, value) = entry.split_once('=').ok_or_else(|| ScanError {
            location: format!("crates/otio-capi/include/otio.h:{}", number + 1),
            message: format!("the constant `{entry}` of `{name}` has no explicit value"),
        })?;
        let value = value.trim().parse::<i64>().map_err(|_| ScanError {
            location: format!("crates/otio-capi/include/otio.h:{}", number + 1),
            message: format!("the constant `{entry}` of `{name}` is not a plain integer"),
        })?;
        constants.push((constant.trim().to_string(), value));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::enum_constants;

    #[test]
    fn it_reads_an_enum_out_of_a_header() {
        let header = "\
/** What a call did. */
typedef enum OtioStatus {
    /** Fine. */
    OTIO_STATUS_OK = 0,
    /** Not fine. */
    OTIO_STATUS_PANIC = 11
} OtioStatus;
";
        let found = enum_constants(header).expect("the sample reads");
        let status = &found["OtioStatus"];
        assert_eq!(status[0], ("OTIO_STATUS_OK".to_string(), 0));
        assert_eq!(status[1], ("OTIO_STATUS_PANIC".to_string(), 11));
    }
}

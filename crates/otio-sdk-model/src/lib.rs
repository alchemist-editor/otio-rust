//! One description of the C ABI, for every SDK generated from it.
//!
//! # Why this exists
//!
//! Everything above the Python bindings sits on `otio-capi`. Writing a
//! binding per language by hand means six surfaces that drift from the core
//! the moment anyone adds a function to it — and drift a user finds before
//! CI does.
//!
//! So there is one description, and the generators read it. This crate makes
//! that description, by reading the Rust source of `otio-capi`: its entry
//! points, the structs and enums that cross the boundary, and the doc
//! comments, which travel with the description so the SDKs document
//! themselves.
//!
//! # Why the Rust source, and not the header
//!
//! Three candidates were considered: the Rust source, the C header, and an
//! interface file written alongside both.
//!
//! The Rust source wins because it is the only one of the three that cannot
//! be out of date. It *is* the library: a function exists because it is
//! written there, and its doc comment is the one a Rust user already reads.
//! The header is a second statement of the same thing — `otio-capi`'s own
//! `tests/header.rs` exists precisely because a second statement can
//! disagree with the first — and an interface file would be a third, needing
//! its own check to stay honest. Reading the source means a function added to
//! the C ABI reaches every SDK by being written, with no second place to
//! remember.
//!
//! The one thing the Rust source does not state is what a C caller spells a
//! constant, since `OtioValueKind::Bool` is `OTIO_VALUE_BOOL`. That comes
//! from the header, whose agreement with the source is checked here as well.
//!
//! # How drift becomes a failing build
//!
//! The description is written to `sdk/api.json` and committed, and the
//! generated SDKs are committed beside it. A test regenerates both and fails
//! on any difference, so:
//!
//! - Adding a function to the C ABI and not regenerating fails CI.
//! - Adding one that fits none of the ABI's conventions fails with its name,
//!   rather than quietly missing from every SDK.
//! - Adding a schema to the core without saying where it sits in the OTIO
//!   ladder fails with its name.
//! - Changing a doc comment without regenerating fails, so the SDKs never
//!   document an older version of the library than they wrap.
//!
//! # What a backend gets
//!
//! [`Api`] is the interface with its conventions read back out: which calls
//! are constructors, what each is a method on, which parameters a caller
//! supplies and which exist only to receive a result, where a list is a
//! two-pass call, and where "no value" is an answer rather than a failure.
//! That is enough to emit a method on a `Clip` returning a `[]Clip` and an
//! `error`, rather than a free function taking six pointers.

mod classify;
pub mod conformance;
mod header;
pub mod json;
mod layout;
pub mod model;
pub mod names;
pub mod overrides;
mod placement;
pub mod scan;
mod schema;

use std::path::Path;

pub use model::{
    Api, ByWidth, CResult, Docs, Enum, Field, Function, Group, Layout, Output, Param, ParamRole,
    Placement, Receiver, Role, Schema, Struct, Type, Variant,
};
pub use scan::{ScanError, Scanned};

/// Where the C ABI crate sits, relative to the workspace root.
pub const CAPI_SOURCE: &str = "crates/otio-capi/src";

/// Where its header sits, relative to the workspace root.
pub const CAPI_HEADER: &str = "crates/otio-capi/include/otio.h";

/// Where the description is committed, relative to the workspace root.
pub const API_JSON: &str = "sdk/api.json";

/// Where the conformance scenarios are committed as JSON, beside the
/// description, for a reader or a generator that is not written in Rust.
pub const CONFORMANCE_JSON: &str = "sdk/conformance.json";

/// Reads the C ABI and builds the description of it.
///
/// `workspace` is the root of the repository.
///
/// # Errors
///
/// Fails if the source cannot be read, or if anything in it fits none of the
/// conventions the C ABI is written to — which is the point: a surprise here
/// is a build failure rather than a function missing from six SDKs.
pub fn describe(workspace: &Path) -> Scanned<Api> {
    let source = scan::directory(&workspace.join(CAPI_SOURCE))?;
    let header_path = workspace.join(CAPI_HEADER);
    let header_text = std::fs::read_to_string(&header_path).map_err(|error| ScanError {
        location: header_path.display().to_string(),
        message: error.to_string(),
    })?;
    let version = env!("CARGO_PKG_VERSION");
    classify::api(&source, &header_text, version)
}

//! Generating the language SDKs from the description of the C ABI.
//!
//! The binary writes the SDKs; this library is the same work as a function,
//! so that a test can ask whether what is committed is still what the C ABI
//! says it should be. That test is the drift detector: an edit to the C ABI
//! that nobody regenerated fails `cargo test`, rather than shipping an SDK
//! missing a function.

use std::path::{Path, PathBuf};

pub mod emit;
mod go;
mod zig;

/// A backend: everything it writes, from the description.
type Backend = fn(&otio_sdk_model::Api) -> Result<Vec<emit::File>, String>;

/// Every SDK this writes, by the name the command line calls it.
pub const TARGETS: &[(&str, Backend)] = &[("go", go::generate), ("zig", zig::generate)];

/// Generates, or checks, every requested target.
///
/// With `check` set nothing is written: the files are built in memory and
/// compared with what is on disk, and any difference is the error.
///
/// # Errors
///
/// Fails if the C ABI cannot be described, if a backend cannot write it, or
/// — in `check` mode — if what is committed is not what the C ABI says.
pub fn run(workspace: &Path, check: bool, wanted: &[String]) -> Result<Vec<PathBuf>, String> {
    let api = otio_sdk_model::describe(workspace).map_err(|error| error.to_string())?;

    let mut files = vec![emit::File {
        path: PathBuf::from(otio_sdk_model::API_JSON),
        contents: otio_sdk_model::json::render(&api),
    }];

    for (name, generate) in TARGETS {
        if !wanted.is_empty() && !wanted.iter().any(|want| want == name) {
            continue;
        }
        files.extend(generate(&api)?);
    }

    let mut written = Vec::new();
    let mut stale = Vec::new();
    for file in &files {
        let path = workspace.join(&file.path);
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref() == Some(file.contents.as_str()) {
            continue;
        }
        if check {
            stale.push(file.path.display().to_string());
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, &file.contents).map_err(|error| error.to_string())?;
        written.push(file.path.clone());
    }

    if stale.is_empty() {
        return Ok(written);
    }
    Err(format!(
        "these are not what the C ABI says they should be:\n  {}\n\nRun `cargo run -p \
         otio-sdk-gen` and commit what it writes.",
        stale.join("\n  ")
    ))
}

/// The root of the repository, found from where this crate was built.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two directories below the workspace root")
        .to_path_buf()
}

//! Generates the TypeScript SDK from the C ABI's source.
//!
//! ```sh
//! cargo run -p otio-wasm --bin otio-ts-gen           # write the files
//! cargo run -p otio-wasm --bin otio-ts-gen -- --check # fail if they differ
//! ```
//!
//! `--check` is what CI runs. A change to `crates/otio-capi` that the SDK has
//! not accounted for fails there rather than reaching a user.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use otio_wasm::sdk::abi::Abi;
use otio_wasm::sdk::{emit, plan};

fn main() -> ExitCode {
    let check = std::env::args().any(|argument| argument == "--check");
    match run(check) {
        Ok(differences) if differences.is_empty() => ExitCode::SUCCESS,
        Ok(differences) => {
            eprintln!(
                "the generated SDK is out of date; run `cargo run -p otio-wasm --bin otio-ts-gen`:"
            );
            for path in differences {
                eprintln!("  {path}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Generates everything, writing it out or comparing it, and returns the paths
/// that differ.
fn run(check: bool) -> Result<Vec<String>, String> {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let capi = crate_root
        .parent()
        .ok_or("the crate has a parent directory")?
        .join("otio-capi/src");

    let abi = Abi::read(&capi).map_err(|error| error.to_string())?;
    let sdk = plan::plan(&abi)?;

    let artifacts = vec![
        emit::assertions(&abi)?,
        emit::exports(&abi),
        emit::types(&abi)?,
        emit::raw(&abi, &sdk)?,
        emit::values(&abi, &sdk)?,
        emit::api(&abi, &sdk)?,
    ];

    let mut differences = Vec::new();
    for artifact in artifacts {
        let path = crate_root.join(&artifact.path);
        if check {
            if read(&path).as_deref() != Some(artifact.text.as_str()) {
                differences.push(artifact.path);
            }
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&path, &artifact.text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(differences)
}

/// Reads a file, treating a missing one as absent rather than as an error.
fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

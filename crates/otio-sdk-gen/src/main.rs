//! Writes the language SDKs from the description of the C ABI.
//!
//! Run it with no arguments to write every SDK, or with `--check` to
//! regenerate everything into memory and fail if what is on disk differs.

use std::process::ExitCode;

use otio_sdk_gen::{run, workspace_root};

fn main() -> ExitCode {
    let mut check = false;
    let mut wanted: Vec<String> = Vec::new();
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--check" => check = true,
            "--help" | "-h" => {
                println!("otio-sdk-gen [--check] [target ...]");
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => {
                eprintln!("otio-sdk-gen: unknown option `{other}`");
                return ExitCode::FAILURE;
            }
            other => wanted.push(other.to_string()),
        }
    }

    match run(&workspace_root(), check, &wanted) {
        Ok(written) => {
            for path in written {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("otio-sdk-gen: {message}");
            ExitCode::FAILURE
        }
    }
}

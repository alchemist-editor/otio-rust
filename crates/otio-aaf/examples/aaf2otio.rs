//! Prints an AAF file as OTIO JSON, as this crate reads it.
//!
//! ```text
//! cargo run --release -p otio-aaf --example aaf2otio -- [--structural] cut.aaf
//! ```
//!
//! `--structural` reads without simplifying or attaching markers, which is
//! upstream's `simplify=False, attach_markers=False`. The output is what the
//! baselines under `tests/data` are compared against.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let structural = args.iter().any(|arg| arg == "--structural");
    let Some(path) = args.iter().find(|arg| !arg.starts_with("--")) else {
        eprintln!("usage: aaf2otio [--structural] FILE.aaf");
        return ExitCode::FAILURE;
    };
    let options = if structural {
        otio_aaf::ReadOptions::structural()
    } else {
        otio_aaf::ReadOptions::default()
    };
    let written = otio_aaf::read_from_file_with(path, &options)
        .map_err(|error| error.to_string())
        .and_then(|document| {
            otio_core::to_string_pretty(&document, otio_core::DEFAULT_INDENT)
                .map_err(|error| error.to_string())
        });
    match written {
        Ok(json) => {
            print!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{path}: {error}");
            ExitCode::FAILURE
        }
    }
}

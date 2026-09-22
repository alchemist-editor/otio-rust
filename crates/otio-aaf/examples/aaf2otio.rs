//! Prints an AAF file as OTIO JSON, as this crate reads it.
//!
//! ```text
//! cargo run --release -p otio-aaf --example aaf2otio -- [--structural] [--bake] [--log] cut.aaf
//! ```
//!
//! `--structural` reads without simplifying or attaching markers, which is
//! upstream's `simplify=False, attach_markers=False`. The output is what the
//! baselines under `tests/data` are compared against. `--bake` is upstream's
//! `bake_keyframed_properties=True`, and `--log` its `transcribe_log=True`,
//! printed to standard error so that standard output stays the JSON.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|arg| arg == name);
    let Some(path) = args.iter().find(|arg| !arg.starts_with("--")) else {
        eprintln!("usage: aaf2otio [--structural] [--bake] [--log] FILE.aaf");
        return ExitCode::FAILURE;
    };
    let mut options = if flag("--structural") {
        otio_aaf::ReadOptions::structural()
    } else {
        otio_aaf::ReadOptions::default()
    }
    .with_bake_keyframed_properties(flag("--bake"));
    if flag("--log") {
        options = options.with_transcribe_log(otio_aaf::TranscribeLog::new(|line| {
            eprintln!("{line}");
        }));
    }
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

//! How much gzip and Brotli save on the repository's timelines, and what
//! they cost, as a Markdown table. CI runs it in release and adds the table
//! to the job summary:
//!
//! ```text
//! cargo run --release -p otio-ext-sync-transport --example compression
//! ```
//!
//! Each timeline is written compact, as a host publishes it, and wrapped in
//! a Furnace `snapshot` envelope, so the sizes are what would go on the
//! wire. It fails if Brotli does not at least halve any timeline above the
//! compression threshold, which would mean the settings are wrong.

use std::path::Path;
use std::time::Instant;

use otio_ext_sync_transport::compression::{Encoding, THRESHOLD};

fn main() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../crates/otio-core/tests/data");
    let mut files: Vec<_> = std::fs::read_dir(&data)
        .expect("the core's test data")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "otio")
        })
        .collect();
    files.sort();

    println!("| Timeline | Snapshot | gzip | gzip time | Brotli | Brotli time |");
    println!("| --- | ---: | ---: | ---: | ---: | ---: |");
    let mut failures = Vec::new();
    for path in files {
        let name = path
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path).expect("reads");
        let Ok(document) = otio_core::from_str(&text) else {
            continue;
        };
        let Some(root) = document.root() else {
            continue;
        };
        let options = otio_core::WriteOptions {
            indent: None,
            ..otio_core::WriteOptions::default()
        };
        let timeline =
            otio_core::to_string_with(&document, &otio_core::Any::Object(root), &options)
                .expect("writes");
        let message = format!(
            r#"{{"v":2,"type":"snapshot","seq":1,"ts":0,"payload":{{"sequenceId":"main","generation":1,"otioJson":{timeline}}}}}"#
        );
        let mut cells = vec![name.clone(), kib(message.len())];
        for encoding in [Encoding::Gzip, Encoding::Brotli] {
            let rounds = (4_000_000 / message.len()).clamp(1, 200);
            let started = Instant::now();
            let mut compressed = Vec::new();
            for _ in 0..rounds {
                compressed = encoding.compress(message.as_bytes()).expect("compresses");
            }
            let elapsed = started.elapsed().as_secs_f64() * 1000.0 / rounds as f64;
            assert_eq!(
                encoding.expand(&compressed).expect("expands"),
                message.as_bytes()
            );
            let ratio = compressed.len() as f64 / message.len() as f64;
            cells.push(format!("{} ({:.0}%)", kib(compressed.len()), ratio * 100.0));
            cells.push(format!("{elapsed:.2} ms"));
            if encoding == Encoding::Brotli && message.len() >= THRESHOLD && ratio > 0.5 {
                failures.push(format!("{name}: Brotli kept {:.0}%", ratio * 100.0));
            }
        }
        println!("| {} |", cells.join(" | "));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn kib(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

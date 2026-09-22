// Each test that includes this module uses only some of it.
#![allow(dead_code)]

//! Checking a written file against one upstream wrote, shared by the tests
//! of every crate that writes AAF: this crate's own, and the OpenTimelineIO
//! adapter's, which includes this file by path.
//!
//! A generator script writes each fixture with pyaaf2 and records beside it,
//! in a `<name>.calls.tsv` sidecar, how it was set up and every time and
//! identifier pyaaf2 (and whatever drove it) asked for, in order.
//! [`Replay`], which lives in the crate as `aaf::write::replay` so that the
//! Python bindings' tests can use it too, hands those same values back to our
//! writer in the same order, and [`assert_identical`] compares the two files
//! and names the part of the compound file where they first differ.

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::Path;

use aaf::cfb::CompoundFile;

// Not every test that includes this module names both.
#[allow(unused_imports)]
pub use aaf::write::replay::{Replay, Sidecar};

/// Reads the sidecar at `path`, for the fixture called `name`.
pub fn read_sidecar(name: &str, path: &Path) -> Sidecar {
    Sidecar::read(name, path).unwrap_or_else(|e| panic!("{e}"))
}

// --- saying where two files differ --------------------------------------------

/// What part of compound file `bytes` the byte at `offset` belongs to.
pub fn locate(bytes: &[u8], offset: usize) -> String {
    let Ok(file) = CompoundFile::open(Cursor::new(bytes.to_vec())) else {
        return "a file that does not open".to_owned();
    };
    let ss = file.sector_size() as usize;
    if offset < ss {
        return format!("the header, byte {offset}");
    }
    let sector = u32::try_from(offset / ss - 1).unwrap();
    let within = offset % ss;
    let fat = file.fat();
    let chain = |start: Option<u32>, table: &[u32]| {
        let mut out = Vec::new();
        let mut s = start;
        while let Some(sid) = s {
            if sid as usize >= table.len() || out.contains(&sid) {
                break;
            }
            out.push(sid);
            s = Some(table[sid as usize]).filter(|n| (*n as usize) < table.len());
        }
        out
    };

    match fat.get(sector as usize) {
        Some(&0xffff_fffd) => return format!("FAT sector {sector}"),
        Some(&0xffff_fffc) => return format!("DIFAT sector {sector}"),
        Some(&0xffff_ffff) | None => return format!("free sector {sector}"),
        _ => {}
    }

    let header = file.header();
    let dir_chain = chain(Some(header.dir_sector_start), fat);
    if let Some(i) = dir_chain.iter().position(|s| *s == sector) {
        let entry = (i * ss + within) / 128;
        let name = file
            .entries()
            .iter()
            .find(|e| e.id().get() as usize == entry)
            .map_or("an unused slot".to_owned(), |e| {
                file.path(e.id()).unwrap_or_default()
            });
        return format!(
            "directory entry {entry} ({name}), byte {}",
            (i * ss + within) % 128
        );
    }
    if chain(Some(header.mini_fat_sector_start), fat).contains(&sector) {
        return format!("mini FAT sector {sector}");
    }

    let root = file.root().expect("a root");
    let mini_chain = chain(root.start_sector(), fat);
    if let Some(i) = mini_chain.iter().position(|s| *s == sector) {
        let mini_offset = i * ss + within;
        let mini_sector = u32::try_from(mini_offset / 64).unwrap();
        for e in file.entries() {
            if e.is_stream()
                && e.len() < 4096
                && chain(e.start_sector(), file.mini_fat()).contains(&mini_sector)
            {
                let at = chain(e.start_sector(), file.mini_fat())
                    .iter()
                    .position(|s| *s == mini_sector)
                    .unwrap()
                    * 64
                    + mini_offset % 64;
                return format!(
                    "stream {}, byte {at} (in the mini stream)",
                    file.path(e.id()).unwrap()
                );
            }
        }
        return format!("mini stream sector {mini_sector}, owned by no stream");
    }
    for e in file.entries() {
        if e.is_stream() && e.len() >= 4096 {
            let c = chain(e.start_sector(), fat);
            if let Some(i) = c.iter().position(|s| *s == sector) {
                return format!(
                    "stream {}, byte {}",
                    file.path(e.id()).unwrap(),
                    i * ss + within
                );
            }
        }
    }
    format!("sector {sector}, owned by nothing")
}

/// Asserts two files are identical, and says where they first differ if not.
pub fn assert_identical(name: &str, ours: &[u8], theirs: &[u8]) {
    if ours == theirs {
        return;
    }
    let first = ours
        .iter()
        .zip(theirs)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| ours.len().min(theirs.len()));
    let mut message = format!(
        "{name}: the written file differs from pyaaf2's ({} bytes against {}).\n\
         First difference at byte {first:#x}:\n  in pyaaf2's file: {}\n  in ours: {}\n",
        ours.len(),
        theirs.len(),
        locate(theirs, first),
        locate(ours, first),
    );
    let end = (first + 32).min(ours.len()).min(theirs.len());
    let _ = writeln!(message, "  pyaaf2: {:02x?}", &theirs[first.min(end)..end]);
    let _ = writeln!(message, "  ours:   {:02x?}", &ours[first.min(end)..end]);
    panic!("{message}");
}

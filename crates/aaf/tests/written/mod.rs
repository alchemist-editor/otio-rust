// Each test that includes this module uses only some of it.
#![allow(dead_code)]

//! Checking a written file against one upstream wrote, shared by the tests
//! of every crate that writes AAF: this crate's own, and the OpenTimelineIO
//! adapter's, which includes this file by path.
//!
//! A generator script writes each fixture with pyaaf2 and records beside it,
//! in a `<name>.calls.tsv` sidecar, how it was set up and every time and
//! identifier pyaaf2 (and whatever drove it) asked for, in order. [`Replay`]
//! hands those same values back to our writer in the same order and fails
//! the moment it asks for something else, and [`assert_identical`] compares
//! the two files and names the part of the compound file where they first
//! differ.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex};

use aaf::Auid;
use aaf::cfb::CompoundFile;
use aaf::write::{Clock, IdSource, Timestamp};

// --- replaying pyaaf2's times and identifiers ---------------------------------

/// The values pyaaf2 handed out, as the fixture's sidecar lists them.
#[derive(Clone)]
pub struct Replay {
    name: String,
    calls: Arc<Mutex<VecDeque<(String, String)>>>,
}

impl Replay {
    fn next(&self, kind: &str) -> String {
        let mut calls = self.calls.lock().unwrap();
        let (k, value) = calls.pop_front().unwrap_or_else(|| {
            panic!(
                "{}: the writer asked for {kind} after pyaaf2 had stopped asking",
                self.name
            )
        });
        assert_eq!(
            k, kind,
            "{}: the writer asked for {kind} where pyaaf2 asked for {k}",
            self.name
        );
        value
    }

    pub fn assert_used_up(&self) {
        let calls = self.calls.lock().unwrap();
        assert!(
            calls.is_empty(),
            "{}: pyaaf2 asked for {} more value(s) than the writer did, starting with {:?}",
            self.name,
            calls.len(),
            calls.front()
        );
    }
}

impl Clock for Replay {
    fn now(&mut self) -> Timestamp {
        let value = self.next("now");
        Timestamp::parse_iso(&value).expect("the sidecar holds ISO times")
    }
}

impl IdSource for Replay {
    fn uuid4(&mut self) -> Auid {
        self.next("uuid4").parse().expect("the sidecar holds UUIDs")
    }
}

/// What a sidecar says about how its fixture was written.
pub struct Sidecar {
    /// The sector size the file was written with.
    pub sector_size: u32,
    /// The writer's options, by name, where the generator set any.
    pub options: Vec<(String, bool)>,
    /// The user the generator said was logged in, if it said.
    pub user: Option<String>,
    /// The times and identifiers, to be replayed.
    pub replay: Replay,
}

impl Sidecar {
    /// Reads the sidecar at `path`, for the fixture called `name`.
    ///
    /// Lines are tab-separated: `sector_size` and a size, `option`, a name
    /// and `true` or `false`, `user` and a name, and then `now` and `uuid4`
    /// lines, which are the calls to replay. Lines starting `#` are
    /// comments.
    pub fn read(name: &str, path: &Path) -> Self {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut sector_size = 4096;
        let mut options = Vec::new();
        let mut user = None;
        let mut calls = VecDeque::new();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let (kind, value) = line.split_once('\t').expect("a tab-separated line");
            match kind {
                "sector_size" => sector_size = value.parse().expect("a sector size"),
                "option" => {
                    let (option, on) = value.split_once('\t').expect("an option and a value");
                    options.push((option.to_owned(), on == "true"));
                }
                "user" => user = Some(value.to_owned()),
                _ => calls.push_back((kind.to_owned(), value.to_owned())),
            }
        }
        Self {
            sector_size,
            options,
            user,
            replay: Replay {
                name: name.to_owned(),
                calls: Arc::new(Mutex::new(calls)),
            },
        }
    }

    /// Whether the generator turned `option` on.
    pub fn option(&self, option: &str) -> bool {
        self.options.iter().any(|(o, on)| o == option && *on)
    }
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

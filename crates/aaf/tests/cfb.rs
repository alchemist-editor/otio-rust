//! Reading real AAF files, checked against what upstream `pyaaf2` reads.
//!
//! The two fixtures cover both sector sizes the format allows: `empty.aaf` is
//! a version 4 file with 4096-byte sectors, `sector_size_512.aaf` a version 3
//! file with 512-byte ones. Between them they exercise full-sector streams,
//! mini streams, multi-sector directories and the DIFAT.

mod common;

use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};

use aaf::cfb::{CompoundFile, EntryType, ROOT_ID, cmp_names, sector};
use aaf::{Auid, cfb};

use common::{data_dir, describe_stream, read_manifest};

fn open(name: &str) -> CompoundFile<File> {
    let path = data_dir().join(name);
    CompoundFile::open(File::open(&path).expect("fixture is readable")).expect("fixture opens")
}

#[test]
fn sha256_matches_known_vectors() {
    // FIPS 180-4's own examples, so a wrong hash here is a broken helper
    // rather than a broken reader.
    let hex = |bytes: [u8; 32]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(common::sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(common::sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(common::sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn reads_a_version_4_header() {
    let file = open("empty.aaf");
    let header = file.header();

    assert_eq!(header.major_version, 4);
    assert_eq!(header.minor_version, 0x3e);
    assert_eq!(header.sector_size, 4096);
    assert_eq!(header.mini_sector_size, 64);
    assert_eq!(header.mini_stream_cutoff, 4096);
    assert_eq!(
        header.class_id.to_string(),
        "0d010201-0200-0000-060e-2b3403020101"
    );
}

#[test]
fn reads_a_version_3_header() {
    let file = open("sector_size_512.aaf");
    let header = file.header();

    assert_eq!(header.major_version, 3);
    assert_eq!(header.sector_size, 512);
    assert_eq!(header.mini_sector_size, 64);
    assert_eq!(
        header.class_id.to_string(),
        "42464141-000d-4d4f-060e-2b34010101ff"
    );
}

#[test]
fn both_fixtures_are_well_formed() {
    // Nothing had to be worked around in either file, so a warning appearing
    // here later means the reader started mis-reading something.
    assert_eq!(open("empty.aaf").warnings(), &[]);
    assert_eq!(open("sector_size_512.aaf").warnings(), &[]);
}

#[test]
fn the_root_entry_owns_the_mini_stream() {
    for (name, start, len) in [
        ("empty.aaf", 3, 65_984),
        ("sector_size_512.aaf", 3, 120_256),
    ] {
        let file = open(name);
        let root = file.root().expect("the file has a root");

        assert_eq!(root.entry_type(), EntryType::RootStorage, "{name}");
        assert_eq!(root.name(), "Root Entry", "{name}");
        assert_eq!(root.start_sector(), Some(start), "{name}");
        assert_eq!(root.len(), len, "{name}");
    }
}

/// Every entry in the file, with every stream's contents, against `pyaaf2`.
///
/// This is the test that matters: it walks the whole directory tree and reads
/// every stream in both fixtures — 766 and 1386 entries — and compares the
/// paths, entry kinds, class AUIDs, stream lengths and stream content hashes
/// against a manifest upstream produced from the same two files.
#[test]
fn matches_pyaaf2_on_every_entry() {
    for (fixture, manifest) in [
        ("empty.aaf", "empty.manifest.tsv"),
        ("sector_size_512.aaf", "sector_size_512.manifest.tsv"),
    ] {
        let expected = read_manifest(manifest);
        assert!(!expected.is_empty(), "{manifest} has rows");

        let mut file = open(fixture);
        let mut walked = file.walk().expect("the directory tree is walkable");
        walked.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(
            walked.len(),
            expected.len(),
            "{fixture}: entry count differs from pyaaf2"
        );

        for (row, (path, id)) in expected.iter().zip(walked) {
            assert_eq!(&path, &row.path, "{fixture}: paths diverge");

            let entry = file.entry(id).expect("walk returned a real entry");
            let kind = match entry.entry_type() {
                EntryType::RootStorage => "root",
                EntryType::Storage => "storage",
                EntryType::Stream => "stream",
                other => panic!("{fixture}: {path} has unexpected type {other:?}"),
            };
            assert_eq!(kind, row.kind, "{fixture}: {path} has the wrong kind");

            let class_id = entry.class_id().map_or("-".to_owned(), |id| id.to_string());
            assert_eq!(
                class_id, row.class_id,
                "{fixture}: {path} has the wrong class"
            );

            if entry.is_stream() {
                let bytes = file.read_stream(id).expect("the stream reads");
                assert_eq!(
                    describe_stream(&bytes),
                    row.contents,
                    "{fixture}: {path} has the wrong contents"
                );
            }
        }
    }
}

/// The mini stream is a full-sector stream, and every mini stream sits inside it.
///
/// Neither fixture has a stream over the 4096-byte cutoff, so the root
/// storage's own stream is what exercises the full-sector read path: 17
/// sectors in the version 4 file, 235 in the version 3 one. Slicing the mini
/// streams back out of it checks the two paths against each other, since a
/// mistake in either one would make them disagree.
#[test]
fn mini_streams_are_slices_of_the_root_stream() {
    for fixture in ["empty.aaf", "sector_size_512.aaf"] {
        let mut file = open(fixture);
        let sector_size = u64::from(file.sector_size());

        let mini_stream = {
            let mut stream = file.open_stream(ROOT_ID).expect("the root stream opens");
            assert!(
                !stream.is_mini(),
                "{fixture}: the mini stream is not itself mini"
            );
            assert!(
                stream.len() > sector_size * 4,
                "{fixture}: the mini stream should span several sectors"
            );
            stream.read_to_end_vec().expect("the root stream reads")
        };
        assert_eq!(mini_stream.len() as u64, file.root().expect("root").len());

        let mini_size = sector::MINI_SECTOR_SIZE as usize;
        let mut checked = 0;

        for (path, id) in file.walk().expect("walkable") {
            let entry = file.entry(id).expect("real entry");
            if !entry.is_stream() || entry.len() >= u64::from(file.header().mini_stream_cutoff) {
                continue;
            }
            let Some(start) = entry.start_sector() else {
                continue;
            };
            let len = entry.len() as usize;

            // Gather the stream by hand, one mini sector at a time, straight
            // out of the bytes the full-sector path just produced.
            let mut assembled = Vec::with_capacity(len);
            let mut mini_sid = start;
            while assembled.len() < len {
                let at = mini_sid as usize * mini_size;
                assembled.extend_from_slice(&mini_stream[at..at + mini_size]);
                mini_sid = file.mini_fat()[mini_sid as usize];
            }
            assembled.truncate(len);

            assert_eq!(
                assembled,
                file.read_stream(id).expect("the stream reads"),
                "{fixture}: {path} differs between the mini and full sector paths"
            );
            checked += 1;
        }

        assert!(
            checked > 100,
            "{fixture}: only checked {checked} mini streams"
        );
    }
}

#[test]
fn finds_entries_by_path() {
    let file = open("empty.aaf");

    assert_eq!(file.find("/"), Some(ROOT_ID));
    assert_eq!(file.find(""), Some(ROOT_ID));

    let header = file.find("/Header-2").expect("the header storage exists");
    assert_eq!(file.entry(header).expect("real").name(), "Header-2");
    // Names are compared the way the format compares them, case-insensitively.
    assert_eq!(file.find("/header-2"), Some(header));
    assert_eq!(file.find("Header-2"), Some(header));

    assert_eq!(file.find("/Header-2/NotThere"), None);
    assert_eq!(file.find("/NotThere/Header-2"), None);
}

#[test]
fn path_round_trips_through_find() {
    let file = open("sector_size_512.aaf");
    for (path, id) in file.walk().expect("walkable") {
        assert_eq!(file.path(id).expect("has a path"), path);
        assert_eq!(file.find(&path), Some(id), "{path} did not round trip");
    }
}

#[test]
fn children_come_back_in_the_formats_order() {
    let file = open("empty.aaf");
    let dictionary = file.find("/Header-2/Dictionary-3b04").expect("exists");
    let children = file.children(dictionary).expect("listable");

    assert!(children.len() > 1);
    for pair in children.windows(2) {
        assert!(
            cmp_names(pair[0].name(), pair[1].name()).is_lt(),
            "{} should sort before {}",
            pair[0].name(),
            pair[1].name()
        );
    }
}

#[test]
fn listing_a_stream_is_an_error() {
    let file = open("empty.aaf");
    let stream = file
        .find("/Header-2/Content-3b03/properties")
        .expect("exists");
    assert!(matches!(
        file.children(stream),
        Err(cfb::Error::WrongEntryType {
            expected: "storage",
            ..
        })
    ));
}

#[test]
fn streams_seek_like_files() {
    let mut file = open("empty.aaf");
    let id = file
        .find("/Header-2/Content-3b03/properties")
        .expect("exists");
    let whole = file.read_stream(id).expect("reads");
    assert_eq!(whole.len(), 30);

    let mut stream = file.open_stream(id).expect("opens");

    let mut tail = Vec::new();
    stream.seek(SeekFrom::Start(10)).expect("seeks");
    stream.read_to_end(&mut tail).expect("reads");
    assert_eq!(tail, &whole[10..]);

    let mut last = [0u8; 4];
    stream.seek(SeekFrom::End(-4)).expect("seeks");
    stream.read_exact(&mut last).expect("reads");
    assert_eq!(last, whole[26..]);

    // Seeking past the end is allowed and reads nothing, as for a real file.
    stream.seek(SeekFrom::Start(1_000)).expect("seeks");
    assert_eq!(stream.read(&mut last).expect("reads"), 0);

    assert!(stream.seek(SeekFrom::Start(0)).is_ok());
    assert!(stream.seek(SeekFrom::Current(-1)).is_err());
}

#[test]
fn reads_from_memory_as_well_as_from_a_file() {
    let bytes = std::fs::read(data_dir().join("empty.aaf")).expect("readable");
    let mut from_memory = CompoundFile::open(Cursor::new(bytes)).expect("opens");
    let mut from_disk = open("empty.aaf");

    let path = "/Header-2/Content-3b03/properties";
    let id = from_disk.find(path).expect("exists");
    assert_eq!(from_memory.find(path), Some(id));
    assert_eq!(
        from_memory.read_stream(id).expect("reads"),
        from_disk.read_stream(id).expect("reads")
    );
}

#[test]
fn rejects_files_that_are_not_compound_files() {
    let err = CompoundFile::open(Cursor::new(vec![0u8; 512])).expect_err("rejected");
    assert!(matches!(err, cfb::Error::BadSignature { .. }));

    let err = CompoundFile::open(Cursor::new(b"too short".to_vec())).expect_err("rejected");
    assert!(matches!(err, cfb::Error::Io(_)));
}

#[test]
fn rejects_an_unsupported_sector_size() {
    let mut bytes = std::fs::read(data_dir().join("empty.aaf")).expect("readable");
    bytes[30..32].copy_from_slice(&10u16.to_le_bytes()); // 1024-byte sectors
    let err = CompoundFile::open(Cursor::new(bytes)).expect_err("rejected");
    assert!(matches!(
        err,
        cfb::Error::UnsupportedSectorSize { size: 1024 }
    ));
}

#[test]
fn rejects_a_cyclic_sector_chain() {
    let mut bytes = std::fs::read(data_dir().join("empty.aaf")).expect("readable");
    let file = CompoundFile::open(Cursor::new(bytes.clone())).expect("opens");
    let dir_start = file.header().dir_sector_start;
    let fat_sector = file.header().difat_head[0];

    // Point the directory chain's first sector back at itself.
    let at = sector::offset(fat_sector, 4096) as usize + dir_start as usize * 4;
    bytes[at..at + 4].copy_from_slice(&dir_start.to_le_bytes());

    let err = CompoundFile::open(Cursor::new(bytes)).expect_err("rejected");
    assert!(
        matches!(err, cfb::Error::CyclicChain { mini: false, .. }),
        "{err}"
    );
}

#[test]
fn auids_round_trip_through_their_text_form() {
    let text = "0d010101-0101-2f00-060e-2b3402060101";
    let id: Auid = text.parse().expect("parses");

    assert_eq!(id.to_string(), text);
    assert_eq!(Auid::from_bytes_le(id.to_bytes_le()), id);
    assert_eq!(Auid::from_bytes_be(id.to_bytes_be()), id);
    assert_eq!(id.data1(), 0x0d01_0101);
    assert_eq!(id.data2(), 0x0101);
    assert_eq!(id.data3(), 0x2f00);
    assert_eq!(id.data4(), [0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06, 0x01, 0x01]);

    // The on-disk order is not the textual order: the first three groups are
    // byte-swapped and the last two are not.
    assert_eq!(
        id.to_bytes_le(),
        [
            0x01, 0x01, 0x01, 0x0d, 0x01, 0x01, 0x00, 0x2f, 0x06, 0x0e, 0x2b, 0x34, 0x02, 0x06,
            0x01, 0x01
        ]
    );

    assert_eq!("{0d010101-0101-2f00-060e-2b3402060101}".parse(), Ok(id));
    assert_eq!("urn:0d01010101012f00060e2b3402060101".parse(), Ok(id));
    assert!(Auid::NIL.is_nil());
    assert!("not an auid".parse::<Auid>().is_err());
    assert!(
        "0d010101-0101-2f00-060e-2b340206010"
            .parse::<Auid>()
            .is_err()
    );
    assert!(
        "0d010101-0101-2f00-060e-2b340206010100"
            .parse::<Auid>()
            .is_err()
    );
}

#[test]
fn a_header_count_that_disagrees_with_the_file_is_a_warning_not_an_error() {
    let mut bytes = std::fs::read(data_dir().join("empty.aaf")).expect("readable");
    let declared = u32::from_le_bytes(bytes[44..48].try_into().expect("four bytes"));
    bytes[44..48].copy_from_slice(&(declared + 3).to_le_bytes());

    // Real AAF files get this wrong, so the chains win and the file still opens.
    let file = CompoundFile::open(Cursor::new(bytes)).expect("opens anyway");
    assert_eq!(
        file.warnings(),
        &[cfb::Warning::FatSectorCountMismatch {
            declared: declared + 3,
            found: declared,
        }]
    );
    assert!(file.find("/Header-2").is_some());
}

#[test]
fn a_stream_longer_than_its_chain_fails_instead_of_allocating() {
    let file = open("empty.aaf");
    let id = file
        .find("/Header-2/Content-3b03/properties")
        .expect("exists");
    let entry_at = {
        // Directory entries live in the directory stream, in entry order.
        let per_sector = 4096 / 128;
        let chain_index = id.get() as usize / per_sector;
        let dir_chain_sector = |n: usize| {
            let mut sid = file.header().dir_sector_start;
            for _ in 0..n {
                sid = file.fat()[sid as usize];
            }
            sid
        };
        sector::offset(dir_chain_sector(chain_index), 4096) as usize
            + (id.get() as usize % per_sector) * 128
    };

    let mut bytes = std::fs::read(data_dir().join("empty.aaf")).expect("readable");
    // Claim four exabytes. The chain holds one mini sector.
    bytes[entry_at + 120..entry_at + 128].copy_from_slice(&u64::MAX.to_le_bytes());

    let mut corrupt = CompoundFile::open(Cursor::new(bytes)).expect("still opens");
    let err = corrupt.read_stream(id).expect_err("the read is refused");
    assert!(
        matches!(
            err,
            cfb::Error::StreamLongerThanChain {
                declared: u64::MAX,
                ..
            }
        ),
        "{err}"
    );
}

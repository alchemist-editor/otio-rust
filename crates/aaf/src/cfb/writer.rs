//! Writing compound files, laid out exactly as pyaaf2 lays them out.
//!
//! There are many valid ways to lay out the same set of storages and streams:
//! which sector each stream starts in, which directory slot each entry takes,
//! how each storage's red-black tree of children is balanced. Applications
//! that import AAF are not all equally forgiving of the less common ones, so
//! this writer does not choose. It reproduces pyaaf2's `CompoundFileBinary`
//! in write mode step for step — the same allocation order, the same free
//! lists, the same tree insertion — so that the same sequence of operations
//! produces the same bytes. The tests hold it to that against files pyaaf2
//! itself wrote.
//!
//! # How it works
//!
//! The file is built in memory, as a byte buffer that the writer seeks and
//! writes into just as pyaaf2 seeks and writes into its file. That includes
//! pyaaf2's habit of leaving stale bytes behind: a sector that is freed and
//! not reused keeps whatever it held, and those bytes are part of the output.
//! [`CompoundFileWriter::finish`] writes the header, the allocation tables and
//! the directory, trims the file after its last allocated sector, and hands
//! back the bytes.
//!
//! Directory entries are the one thing kept as values rather than bytes.
//! pyaaf2 writes a changed entry out whenever more than 128 are pending and
//! once more at close, but always to the same slot and always its current
//! state, so the bytes at close are the final state of every entry.
//!
//! # Moving and removing
//!
//! pyaaf2 moves and removes entries for the streams it parks under `/tmp`
//! while their object is not in the file: when an object holding a stream is
//! made or copied in before it joins the file, and when an object holding
//! one is taken out of a file opened for changing. The `remove` module
//! reproduces pyaaf2's `move`, `remove` and `rmtree`, down to the
//! rebalancing of a storage's tree when a child is taken out of it.
//!
//! # Changing an existing file
//!
//! [`CompoundFileWriter::open`] is pyaaf2's `CompoundFileBinary(f, 'rb+')`.
//! It reads the header, the FAT, DIFAT and mini FAT, and the directory into
//! the same state a new file is built in, and keeps the file's bytes as the
//! buffer later writes land in. What pyaaf2 does not rewrite is left as it
//! was:
//!
//! - An entry read from the file keeps its 128 bytes, and at close only the
//!   fields that changed are written over them. Entries nothing touched are
//!   not written at all, and freed ones are zeroed.
//! - The directory's free list starts empty, as pyaaf2's does, so the first
//!   new entries go into a new directory sector. Entries freed while the
//!   file is open are reused.
//! - Freed sectors go to the front of the free list and are reused from
//!   there, and the file is cut after the last sector in use.
//!
//! # Randomness
//!
//! pyaaf2's container module imports `random` for exactly one routine, which
//! rebuilds a storage's tree by inserting its children in shuffled order.
//! Nothing in pyaaf2 calls it, so nothing here needs a random source to match
//! pyaaf2's output, and nothing here has one.
//!
//! # Example
//!
//! ```
//! use aaf::cfb::{CompoundFile, CompoundFileWriter, ROOT_ID};
//!
//! let mut writer = CompoundFileWriter::new(4096)?;
//! let stream = writer.touch(ROOT_ID, "hello")?;
//! writer.write_stream(stream, b"hello, world")?;
//! let bytes = writer.finish()?;
//!
//! let mut file = CompoundFile::open(std::io::Cursor::new(bytes))?;
//! let id = file.find("/hello").expect("the stream is there");
//! assert_eq!(file.read_stream(id)?, b"hello, world");
//! # Ok::<(), aaf::cfb::Error>(())
//! ```

use std::collections::{HashMap, VecDeque};

mod remove;

use super::dir_entry::{DirId, ROOT_ID};
use super::error::{Error, Result};
use super::sector::{DIFAT, END_OF_CHAIN, FAT, FREE, SectorId};
use crate::Auid;

/// The mini stream's sector size. The format fixes it at 64 bytes.
const MINI_SECTOR_SIZE: u64 = 64;

/// Streams shorter than this live in the mini stream.
const MINI_STREAM_CUTOFF: u64 = 4096;

/// The sector covering byte `0x7FFFFF00`, which a version 4 file must leave
/// free for Windows byte-range locking.
const RANGE_LOCK_SECTOR: u32 = (0x7fff_ff00 / 4096) - 1;

/// The class of a version 4 compound file written by pyaaf2.
const CLASS_4096: Auid = Auid::from_bytes_be([
    0x0d, 0x01, 0x02, 0x01, 0x02, 0x00, 0x00, 0x00, 0x06, 0x0e, 0x2b, 0x34, 0x03, 0x02, 0x01, 0x01,
]);

/// The class of a 512-byte-sector compound file written by pyaaf2.
const CLASS_512: Auid = Auid::from_bytes_be([
    0x42, 0x46, 0x41, 0x41, 0x00, 0x0d, 0x4d, 0x4f, 0x06, 0x0e, 0x2b, 0x34, 0x01, 0x01, 0x01, 0xff,
]);

/// The class pyaaf2 puts on the root storage: AAF's `Root` class.
const ROOT_CLASS: Auid = Auid::from_bytes_be([
    0xb3, 0xb3, 0x98, 0xa5, 0x1c, 0x90, 0x11, 0xd4, 0x80, 0x53, 0x08, 0x00, 0x36, 0x21, 0x08, 0x04,
]);

/// The directory entry type bytes.
const TYPE_STORAGE: u8 = 0x01;
const TYPE_STREAM: u8 = 0x02;
const TYPE_ROOT: u8 = 0x05;

/// One directory entry, as the writer holds it until the file is finished.
#[derive(Debug, Clone)]
struct Node {
    name: String,
    kind: u8,
    red: bool,
    left: Option<u32>,
    right: Option<u32>,
    child: Option<u32>,
    class_id: Option<Auid>,
    sector: Option<SectorId>,
    byte_size: u64,
    /// The storage this entry was added to. Not stored in the file.
    parent: Option<u32>,
    /// For an entry read from an existing file, its bytes as read and the
    /// values they decoded to. Not stored in the file.
    parsed: Option<Box<Parsed>>,
    /// Whether the entry has been renamed since it was read, which rewrites
    /// its name field even with the name it had.
    renamed: bool,
}

/// A directory entry as read from an existing file: its 128 bytes, and what
/// each field pyaaf2 can change decoded to.
///
/// pyaaf2 holds an entry as its bytes and changes a field by overwriting
/// just that field, so the bytes it does not model — the flags, the two
/// timestamps, whatever follows the name — are written back as they were
/// read. Here each field is compared with the value it was read as, and
/// only a changed field is written over the bytes as read.
#[derive(Debug, Clone)]
struct Parsed {
    raw: [u8; 128],
    kind: u8,
    red: bool,
    left: Option<u32>,
    right: Option<u32>,
    child: Option<u32>,
    class_id: Option<Auid>,
    sector: Option<SectorId>,
    byte_size: u64,
}

/// pyaaf2's `decode_sid`: only `FREESECT` stands for no link or sector.
fn decode_sid(value: u32) -> Option<u32> {
    (value != FREE).then_some(value)
}

impl Node {
    const fn blank() -> Self {
        Self {
            name: String::new(),
            kind: 0,
            red: false,
            left: None,
            right: None,
            child: None,
            class_id: None,
            sector: None,
            byte_size: 0,
            parent: None,
            parsed: None,
            renamed: false,
        }
    }

    /// An entry read from an existing file, from its 128 bytes.
    fn read(raw: [u8; 128]) -> Self {
        let u32_at =
            |at: usize| u32::from_le_bytes(raw[at..at + 4].try_into().expect("four bytes"));
        let name_size = usize::from(u16::from_le_bytes([raw[64], raw[65]])).min(64);
        let class_bytes: [u8; 16] = raw[80..96].try_into().expect("sixteen bytes");
        let class_id = (class_bytes != [0; 16]).then(|| Auid::from_bytes_le(class_bytes));
        let parsed = Parsed {
            raw,
            kind: raw[66],
            // pyaaf2 reads anything but 0x01 as red.
            red: raw[67] != 0x01,
            left: decode_sid(u32_at(68)),
            right: decode_sid(u32_at(72)),
            child: decode_sid(u32_at(76)),
            class_id,
            sector: decode_sid(u32_at(116)),
            byte_size: u64::from_le_bytes(raw[120..128].try_into().expect("eight bytes")),
        };
        Self {
            name: crate::utf16::decode_le(&raw[..name_size]),
            kind: parsed.kind,
            red: parsed.red,
            left: parsed.left,
            right: parsed.right,
            child: parsed.child,
            class_id: parsed.class_id,
            sector: parsed.sector,
            byte_size: parsed.byte_size,
            parent: None,
            parsed: Some(Box::new(parsed)),
            renamed: false,
        }
    }

    /// The entry's 128 bytes, laid out as pyaaf2's `DirEntry.data` holds them.
    fn encode(&self) -> [u8; 128] {
        let Some(parsed) = &self.parsed else {
            return self.encode_new();
        };
        let mut data = parsed.raw;
        if self.renamed {
            Self::encode_name(&mut data, &self.name);
        }
        if self.kind != parsed.kind {
            data[66] = self.kind;
        }
        if self.red != parsed.red {
            data[67] = if self.red { 0x00 } else { 0x01 };
        }
        for (at, now, then) in [
            (68, self.left, parsed.left),
            (72, self.right, parsed.right),
            (76, self.child, parsed.child),
            (116, self.sector, parsed.sector),
        ] {
            if now != then {
                data[at..at + 4].copy_from_slice(&sid(now).to_le_bytes());
            }
        }
        if self.class_id != parsed.class_id {
            let bytes = self.class_id.map_or([0; 16], |c| c.to_bytes_le());
            data[80..96].copy_from_slice(&bytes);
        }
        if self.byte_size != parsed.byte_size {
            data[120..128].copy_from_slice(&self.byte_size.to_le_bytes());
        }
        data
    }

    /// pyaaf2's `DirEntry.name` setter: the name, zeros to the end of the
    /// field, and the size.
    fn encode_name(data: &mut [u8; 128], name: &str) {
        let bytes: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        data[..64].fill(0);
        data[..bytes.len()].copy_from_slice(&bytes);
        // pyaaf2 counts the terminator, but never past the 64-byte field.
        let name_size = (bytes.len() + 2).min(64) as u16;
        data[64..66].copy_from_slice(&name_size.to_le_bytes());
    }

    /// The bytes of an entry made in this session.
    fn encode_new(&self) -> [u8; 128] {
        let mut data = [0u8; 128];
        let name: Vec<u8> = self
            .name
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        data[..name.len()].copy_from_slice(&name);
        // pyaaf2 counts the terminator, but never past the 64-byte field.
        let name_size = (name.len() + 2).min(64) as u16;
        data[64..66].copy_from_slice(&name_size.to_le_bytes());
        data[66] = self.kind;
        data[67] = if self.red { 0x00 } else { 0x01 };
        data[68..72].copy_from_slice(&sid(self.left).to_le_bytes());
        data[72..76].copy_from_slice(&sid(self.right).to_le_bytes());
        data[76..80].copy_from_slice(&sid(self.child).to_le_bytes());
        if let Some(class_id) = self.class_id {
            data[80..96].copy_from_slice(&class_id.to_bytes_le());
        }
        data[116..120].copy_from_slice(&sid(self.sector).to_le_bytes());
        data[120..128].copy_from_slice(&self.byte_size.to_le_bytes());
        data
    }
}

/// An absent link, sector or child, as pyaaf2 encodes `None`.
fn sid(value: Option<u32>) -> u32 {
    value.unwrap_or(FREE)
}

/// A node in a red-black tree walk: a real entry, or the false root pyaaf2
/// hangs the tree off while it rebalances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Head,
    Entry(u32),
}

/// A position in a stream, as pyaaf2's `Stream` object holds one.
#[derive(Debug, Clone)]
struct Cursor {
    id: u32,
    pos: u64,
    chain: Vec<SectorId>,
}

/// A compound file being written, laid out as pyaaf2 lays one out.
///
/// Storages and streams are created inside a storage by name, and a stream's
/// bytes are written in whole with [`write_stream`](Self::write_stream) or a
/// piece at a time with [`append_stream`](Self::append_stream). Everything is
/// held in memory until [`finish`](Self::finish) returns the file.
#[derive(Debug, Clone)]
pub struct CompoundFileWriter {
    file: Vec<u8>,
    sector_size: u32,
    class_id: Auid,
    minor_version: u16,
    major_version: u16,
    dir_sector_start: SectorId,
    transaction_signature: u32,

    dir_sector_count: u32,
    fat_sector_count: u32,
    minifat_sector_start: SectorId,
    minifat_sector_count: u32,
    difat_sector_start: SectorId,
    difat_sector_count: u32,

    difat: Vec<Vec<SectorId>>,
    fat: Vec<SectorId>,
    fat_freelist: VecDeque<SectorId>,
    minifat: Vec<SectorId>,
    minifat_freelist: VecDeque<SectorId>,
    minifat_chain: Vec<SectorId>,
    dir_fat_chain: Vec<SectorId>,
    mini_stream_chain: Vec<SectorId>,
    dir_freelist: VecDeque<u32>,

    entries: Vec<Node>,
    /// Each storage's children by exact name, for lookups.
    children: HashMap<u32, HashMap<String, u32>>,
    /// The false root of the tree an insertion is working on.
    head: Node,
    /// The append position of each stream opened with `append_stream`.
    open: HashMap<u32, Cursor>,
}

impl CompoundFileWriter {
    /// Starts a new compound file with sectors of `sector_size` bytes.
    ///
    /// pyaaf2 writes 4096-byte sectors unless told otherwise, and accepts 512.
    /// Either way it marks the file as major version 4.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedSectorSize`] for anything but 512 or 4096.
    pub fn new(sector_size: u32) -> Result<Self> {
        let class_id = match sector_size {
            4096 => CLASS_4096,
            512 => CLASS_512,
            size => return Err(Error::UnsupportedSectorSize { size }),
        };

        let per_sector = sector_size / 4;
        let mut fat = vec![FREE; per_sector as usize];
        fat[0] = END_OF_CHAIN; // the first directory sector
        fat[1] = FAT; // the first FAT sector
        let mut difat_head = vec![FREE; 109];
        difat_head[0] = 1;

        let mut root = Node::blank();
        root.name = "Root Entry".to_owned();
        root.kind = TYPE_ROOT;
        root.class_id = Some(ROOT_CLASS);
        // pyaaf2 starts every entry as `bytearray(128)` and only recolours
        // entries it inserts into a tree, so the root keeps colour byte 0.
        root.red = true;

        let mut writer = Self {
            file: Vec::new(),
            sector_size,
            class_id,
            minor_version: 62,
            // pyaaf2 marks a new file as version 4, whatever its sector size.
            major_version: 4,
            dir_sector_start: 0,
            transaction_signature: 1,
            dir_sector_count: 1,
            fat_sector_count: 1,
            minifat_sector_start: FREE,
            minifat_sector_count: 0,
            difat_sector_start: FREE,
            difat_sector_count: 0,
            difat: vec![difat_head],
            fat,
            fat_freelist: (2..per_sector).collect(),
            minifat: Vec::new(),
            minifat_freelist: VecDeque::new(),
            minifat_chain: Vec::new(),
            dir_fat_chain: vec![0],
            mini_stream_chain: Vec::new(),
            // pyaaf2 never puts the rest of the first directory sector on the
            // free list, so the first entry it creates starts a new sector.
            dir_freelist: VecDeque::new(),
            entries: vec![root],
            children: HashMap::new(),
            head: Node::blank(),
            open: HashMap::new(),
        };

        writer.write_header();
        let root = writer.entries[0].encode();
        let pos = writer.dir_entry_pos(0);
        writer.write_at(pos, &root);
        writer.write_at(pos + 128, &vec![0u8; sector_size as usize - 128]);
        writer.write_fat();
        Ok(writer)
    }

    /// Opens an existing compound file for changing, as pyaaf2's
    /// `CompoundFileBinary(f, 'rb+')` does.
    ///
    /// The file's allocation tables, directory and header values are read
    /// into the writer's state, and its bytes become the buffer every later
    /// write lands in, so whatever the changes do not touch stays exactly as
    /// it was. The free lists start as pyaaf2's do: every free sector and
    /// mini sector, in order, and no free directory entries at all, because
    /// pyaaf2 never reuses a directory slot it did not free itself.
    ///
    /// pyaaf2 believes the chains over the header where the two disagree
    /// about the number of FAT or directory sectors, and writes the corrected
    /// counts back; so does this.
    ///
    /// # Errors
    ///
    /// Returns an error for anything pyaaf2 refuses to open: a bad
    /// signature, a sector size other than 512 or 4096, a mini stream cutoff
    /// other than 4096, a cyclic chain, or a chain or entry that points
    /// outside the file's tables.
    pub fn open(file: Vec<u8>) -> Result<Self> {
        let head: &[u8; 512] =
            file.get(..512)
                .and_then(|h| h.try_into().ok())
                .ok_or(Error::Unrepresentable {
                    what: "a file shorter than a compound file header",
                })?;
        let header = super::header::Header::parse(head)?;
        if u64::from(header.mini_stream_cutoff) != MINI_STREAM_CUTOFF {
            return Err(Error::Unrepresentable {
                what: "a mini stream cutoff other than 4096 bytes",
            });
        }
        let sector_size = header.sector_size;

        let mut writer = Self {
            file,
            sector_size,
            class_id: header.class_id,
            minor_version: header.minor_version,
            major_version: header.major_version,
            dir_sector_start: header.dir_sector_start,
            transaction_signature: header.transaction_signature,
            dir_sector_count: header.dir_sector_count,
            fat_sector_count: header.fat_sector_count,
            minifat_sector_start: header.mini_fat_sector_start,
            minifat_sector_count: header.mini_fat_sector_count,
            difat_sector_start: header.difat_sector_start,
            difat_sector_count: header.difat_sector_count,
            difat: vec![header.difat_head.to_vec()],
            fat: Vec::new(),
            fat_freelist: VecDeque::new(),
            minifat: Vec::new(),
            minifat_freelist: VecDeque::new(),
            minifat_chain: Vec::new(),
            dir_fat_chain: Vec::new(),
            mini_stream_chain: Vec::new(),
            dir_freelist: VecDeque::new(),
            entries: Vec::new(),
            children: HashMap::new(),
            head: Node::blank(),
            open: HashMap::new(),
        };

        // The DIFAT sectors after the header's, each ending in the next.
        let mut sid = header.difat_sector_start;
        for _ in 0..header.difat_sector_count {
            if !super::sector::is_regular(sid) {
                break;
            }
            let table = writer.read_table_sector(sid)?;
            sid = *table.last().expect("a sector holds entries");
            writer.difat.push(table);
        }
        if writer.difat.len() - 1 != writer.difat_sector_count as usize {
            return Err(Error::Unrepresentable {
                what: "a DIFAT chain shorter than the header says",
            });
        }

        let fat_sectors = writer.fat_sectors();
        writer.fat_sector_count = fat_sectors.len() as u32;
        for sid in fat_sectors {
            let table = writer.read_table_sector(sid)?;
            writer.fat.extend(table);
        }
        writer.fat_freelist = (0..writer.fat.len() as u32)
            .filter(|&i| writer.fat[i as usize] == FREE)
            .collect();

        writer.minifat_chain = writer.checked_chain(header.mini_fat_sector_start, false)?;
        for sid in writer.minifat_chain.clone() {
            let table = writer.read_table_sector(sid)?;
            writer.minifat.extend(table);
        }
        writer.minifat_freelist = (0..writer.minifat.len() as u32)
            .filter(|&i| writer.minifat[i as usize] == FREE)
            .collect();

        writer.dir_fat_chain = writer.checked_chain(header.dir_sector_start, false)?;
        if writer.dir_fat_chain.is_empty() {
            return Err(Error::MissingRootEntry);
        }
        writer.dir_sector_count = writer.dir_fat_chain.len() as u32;
        let slots = writer.dir_fat_chain.len() * (sector_size as usize / 128);
        for id in 0..slots {
            let pos = writer.dir_entry_pos(id as u32) as usize;
            let mut raw = [0u8; 128];
            if pos < writer.file.len() {
                let end = (pos + 128).min(writer.file.len());
                raw[..end - pos].copy_from_slice(&writer.file[pos..end]);
            }
            writer.entries.push(Node::read(raw));
        }
        if !matches!(writer.entries[0].kind, TYPE_ROOT) {
            return Err(Error::MissingRootEntry);
        }
        writer.read_tree(ROOT_ID.0)?;

        if writer.minifat_sector_count != 0 {
            if let Some(start) = writer.entries[0].sector {
                writer.mini_stream_chain = writer.checked_chain(start, false)?;
            }
        }
        Ok(writer)
    }

    /// A sector of the FAT, mini FAT or DIFAT, as its table of sector
    /// numbers.
    fn read_table_sector(&self, sid: SectorId) -> Result<Vec<SectorId>> {
        let start = self.sector_pos(sid) as usize;
        let end = start + self.sector_size as usize;
        let bytes = self.file.get(start..end).ok_or(Error::SectorOutOfRange {
            sector: sid,
            table_len: (self.file.len() / self.sector_size as usize) as u32,
        })?;
        Ok(bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("four bytes")))
            .collect())
    }

    /// pyaaf2's `get_fat_chain` on a file being opened: the chain from
    /// `start`, refusing a cycle as pyaaf2 does and a sector outside the
    /// table where pyaaf2 would fail with an `IndexError`.
    fn checked_chain(&self, start: SectorId, mini: bool) -> Result<Vec<SectorId>> {
        if matches!(start, END_OF_CHAIN | FREE | DIFAT | FAT) {
            return Ok(Vec::new());
        }
        let table = if mini { &self.minifat } else { &self.fat };
        let next = |sid: SectorId| {
            table
                .get(sid as usize)
                .copied()
                .ok_or(Error::SectorOutOfRange {
                    sector: sid,
                    table_len: table.len() as u32,
                })
        };
        let mut chain = Vec::new();
        let (mut slow, mut fast) = (start, start);
        while fast != END_OF_CHAIN {
            chain.push(fast);
            fast = next(fast)?;
            if slow != END_OF_CHAIN {
                slow = next(slow)?;
                if slow != END_OF_CHAIN {
                    slow = next(slow)?;
                    if slow == fast {
                        return Err(Error::CyclicChain { mini, start });
                    }
                }
            }
        }
        Ok(chain)
    }

    /// Reads a storage's tree of children, and theirs, recording each
    /// child's parent and name as pyaaf2's `listdir_dict` does.
    fn read_tree(&mut self, storage: u32) -> Result<()> {
        let max = self.dir_sector_count as usize * (self.sector_size as usize / 128);
        let mut pending = vec![storage];
        while let Some(storage) = pending.pop() {
            let mut names = HashMap::new();
            let mut visited = Vec::new();
            let mut stack: Vec<u32> = self.entries[storage as usize].child.into_iter().collect();
            let mut count = 0;
            while let Some(current) = stack.pop() {
                let node = self
                    .entries
                    .get(current as usize)
                    .ok_or(Error::DirEntryOutOfRange {
                        id: current,
                        count: self.entries.len() as u32,
                    })?;
                count += 1;
                if count > max {
                    return Err(Error::CorruptDirectoryTree { id: current });
                }
                names.insert(node.name.clone(), current);
                visited.push(current);
                stack.extend(node.left);
                stack.extend(node.right);
            }
            for child in visited {
                let node = &mut self.entries[child as usize];
                if node.parent.is_none() && child != ROOT_ID.0 {
                    node.parent = Some(storage);
                    if matches!(node.kind, TYPE_STORAGE | TYPE_ROOT) {
                        pending.push(child);
                    }
                }
            }
            self.children.insert(storage, names);
        }
        Ok(())
    }

    /// The sector size this file is being written with.
    #[must_use]
    pub const fn sector_size(&self) -> u32 {
        self.sector_size
    }

    /// The entry named `name` directly inside `parent`, if there is one.
    ///
    /// Names match exactly here, as they do in pyaaf2's lookups, even though
    /// the format orders them case-insensitively.
    #[must_use]
    pub fn get(&self, parent: DirId, name: &str) -> Option<DirId> {
        self.children
            .get(&parent.0)
            .and_then(|names| names.get(name))
            .map(|id| DirId(*id))
    }

    /// The path of an entry, `/`-separated from the root.
    #[must_use]
    pub fn path(&self, id: DirId) -> String {
        let mut names = Vec::new();
        let mut current = Some(id.0);
        while let Some(i) = current {
            if i == ROOT_ID.0 {
                break;
            }
            let node = &self.entries[i as usize];
            names.push(node.name.as_str());
            current = node.parent;
        }
        names.reverse();
        format!("/{}", names.join("/"))
    }

    /// Creates a storage inside `parent`.
    ///
    /// # Errors
    ///
    /// Returns an error if `parent` already has an entry of that name, if the
    /// name is too long, or if `parent` is not a storage.
    pub fn create_storage(
        &mut self,
        parent: DirId,
        name: &str,
        class_id: Option<Auid>,
    ) -> Result<DirId> {
        self.create_entry(parent, name, TYPE_STORAGE, class_id)
    }

    /// The stream named `name` inside `parent`, created empty if need be.
    ///
    /// This is pyaaf2's `DirEntry.touch`.
    ///
    /// # Errors
    ///
    /// Returns an error if the name is too long or `parent` is not a storage.
    pub fn touch(&mut self, parent: DirId, name: &str) -> Result<DirId> {
        match self.get(parent, name) {
            Some(id) => Ok(id),
            None => self.create_entry(parent, name, TYPE_STREAM, None),
        }
    }

    /// Records the class of the object a storage holds.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not an entry of this file.
    pub fn set_class_id(&mut self, id: DirId, class_id: Option<Auid>) -> Result<()> {
        self.node_mut(id.0)?.class_id = class_id;
        Ok(())
    }

    /// Replaces a stream's contents, the way pyaaf2 rewrites a stream it
    /// opened for reading and writing: write from the start, then truncate
    /// where the write ended.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not a stream of this file, or if the
    /// allocation runs into one of the states pyaaf2 refuses.
    pub fn write_stream(&mut self, id: DirId, data: &[u8]) -> Result<()> {
        self.check_stream(id)?;
        self.open.remove(&id.0);
        let mut cursor = self.cursor(id.0);
        self.stream_write(&mut cursor, data)?;
        self.stream_truncate(&mut cursor, None)
    }

    /// Writes `data` after whatever was last appended to this stream, as a
    /// stream opened once and written piece by piece is.
    ///
    /// The first append to a stream empties it first, which is what opening
    /// a stream for writing does. The size of each piece matters: a stream
    /// that crosses the 4096-byte mini stream cutoff partway through is moved
    /// out of the mini stream when it crosses, which leaves its old mini
    /// sectors free, and pyaaf2 does the same.
    ///
    /// # Errors
    ///
    /// As for [`write_stream`](Self::write_stream).
    pub fn append_stream(&mut self, id: DirId, data: &[u8]) -> Result<()> {
        self.check_stream(id)?;
        let mut cursor = match self.open.remove(&id.0) {
            Some(cursor) => cursor,
            None => {
                self.empty_stream(id.0)?;
                self.cursor(id.0)
            }
        };
        let result = self.stream_write(&mut cursor, data);
        self.open.insert(id.0, cursor);
        result
    }

    /// Makes the next [`append_stream`](Self::append_stream) to a stream
    /// start it afresh, as opening it for writing again does.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not a stream of this file.
    pub fn reopen_stream(&mut self, id: DirId) -> Result<()> {
        self.check_stream(id)?;
        self.open.remove(&id.0);
        Ok(())
    }

    /// Opens `/`-separated `path` for writing, creating the stream if it is
    /// not there, and appends `data` to it. See
    /// [`append_stream`](Self::append_stream).
    ///
    /// # Errors
    ///
    /// Returns an error if a storage on the way is missing.
    pub fn append_path(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let (dir, name) = match path.trim_start_matches('/').rsplit_once('/') {
            Some((dir, name)) => (dir, name),
            None => ("", path.trim_start_matches('/')),
        };
        let mut parent = ROOT_ID;
        for component in dir.split('/').filter(|c| !c.is_empty()) {
            parent = self.get(parent, component).ok_or(Error::Unrepresentable {
                what: "a storage on the path is missing",
            })?;
        }
        let id = self.touch(parent, name)?;
        self.append_stream(id, data)
    }

    /// The name of an entry.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not an entry of this file.
    pub fn name(&self, id: DirId) -> Result<&str> {
        Ok(&self.node(id.0)?.name)
    }

    /// Whether an entry is a storage, the root included.
    #[must_use]
    pub fn is_storage(&self, id: DirId) -> bool {
        self.entries
            .get(id.0 as usize)
            .is_some_and(|n| matches!(n.kind, TYPE_STORAGE | TYPE_ROOT))
    }

    /// The class an entry records, if it records one.
    #[must_use]
    pub fn class_id(&self, id: DirId) -> Option<Auid> {
        self.entries.get(id.0 as usize).and_then(|n| n.class_id)
    }

    /// Reads the whole of a stream, as pyaaf2's `Stream.read()` does.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not a stream of this file, or its chain
    /// runs out before its length does.
    pub fn read_stream(&self, id: DirId) -> Result<Vec<u8>> {
        self.check_stream(id)?;
        let mut cursor = self.cursor(id.0);
        let size = self.entries[id.0 as usize].byte_size;
        let unit = if self.is_mini(id.0) {
            MINI_SECTOR_SIZE
        } else {
            u64::from(self.sector_size)
        };
        let mini_units =
            self.mini_stream_chain.len() as u64 * u64::from(self.sector_size) / MINI_SECTOR_SIZE;
        if (cursor.chain.len() as u64) < size.div_ceil(unit)
            || (self.is_mini(id.0) && cursor.chain.iter().any(|s| u64::from(*s) >= mini_units))
        {
            return Err(Error::Unrepresentable {
                what: "a stream whose chain is shorter than its length",
            });
        }
        Ok(self.stream_read(&mut cursor))
    }

    /// Finishes the file and returns its bytes.
    ///
    /// This is pyaaf2's `CompoundFileBinary.close`: it settles the mini
    /// stream's length, writes the header, the DIFAT, the FAT, the mini FAT
    /// and the directory, and trims the file after its last allocated sector.
    ///
    /// # Errors
    ///
    /// Returns an error only if trimming the mini stream runs into one of the
    /// states pyaaf2 refuses.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        if self.entries[0].sector.is_some() {
            // The mini stream's length is up to its last used mini sector,
            // not the count of used ones. pyaaf2 notes that some applications
            // crash outright when it is anything else.
            let trailing_free = self
                .minifat
                .iter()
                .rev()
                .take_while(|s| **s == FREE)
                .count();
            let last_used = self.minifat.len() - trailing_free.min(self.minifat.len() - 1);
            let size = last_used as u64 * MINI_SECTOR_SIZE;
            self.entries[0].byte_size = size;
            let mut cursor = self.cursor(0);
            self.stream_truncate(&mut cursor, Some(size))?;
        }

        self.write_header();
        self.write_difat();
        self.write_fat();
        self.write_minifat();
        self.write_dir_entries();

        let trailing_free = self.fat.iter().rev().take_while(|s| **s == FREE).count();
        let last_used = self.fat.len() - trailing_free.min(self.fat.len() - 1);
        let len = (last_used as u64 + 1) * u64::from(self.sector_size);
        self.file.resize(len as usize, 0);
        Ok(self.file)
    }

    // --- raw file -------------------------------------------------------

    /// Writes bytes at a position, growing the file with zeros if need be,
    /// as a seek and write on a real file does.
    fn write_at(&mut self, pos: u64, data: &[u8]) {
        let pos = pos as usize;
        let end = pos + data.len();
        if self.file.len() < end {
            self.file.resize(end, 0);
        }
        self.file[pos..end].copy_from_slice(data);
    }

    /// Reads a sector, padded with zeros past the end of the file.
    fn read_sector(&self, sid: SectorId) -> Vec<u8> {
        let size = self.sector_size as usize;
        let start = (sid as usize + 1) * size;
        let mut sector = vec![0u8; size];
        if start < self.file.len() {
            let end = (start + size).min(self.file.len());
            sector[..end - start].copy_from_slice(&self.file[start..end]);
        }
        sector
    }

    fn sector_pos(&self, sid: SectorId) -> u64 {
        (u64::from(sid) + 1) * u64::from(self.sector_size)
    }

    fn write_header(&mut self) {
        let mut h = Vec::with_capacity(512);
        h.extend_from_slice(&super::header::SIGNATURE);
        h.extend_from_slice(&self.class_id.to_bytes_le());
        h.extend_from_slice(&self.minor_version.to_le_bytes());
        h.extend_from_slice(&self.major_version.to_le_bytes());
        h.extend_from_slice(&0xfffeu16.to_le_bytes());
        h.extend_from_slice(&(self.sector_size.trailing_zeros() as u16).to_le_bytes());
        h.extend_from_slice(&6u16.to_le_bytes());
        h.extend_from_slice(&[0u8; 6]);
        for value in [
            self.dir_sector_count,
            self.fat_sector_count,
            self.dir_sector_start,
            self.transaction_signature,
            MINI_STREAM_CUTOFF as u32,
            self.minifat_sector_start,
            self.minifat_sector_count,
            self.difat_sector_start,
            self.difat_sector_count,
        ] {
            h.extend_from_slice(&value.to_le_bytes());
        }
        for sid in &self.difat[0] {
            h.extend_from_slice(&sid.to_le_bytes());
        }
        h.resize(self.sector_size as usize, 0);
        self.write_at(0, &h);
    }

    fn write_difat(&mut self) {
        let mut head = Vec::with_capacity(self.sector_size as usize - 76);
        for sid in &self.difat[0] {
            head.extend_from_slice(&sid.to_le_bytes());
        }
        head.resize(self.sector_size as usize - 76, 0);
        self.write_at(76, &head);

        let mut sid = self.difat_sector_start;
        for i in 1..self.difat.len() {
            let table: Vec<u8> = self.difat[i].iter().flat_map(|s| s.to_le_bytes()).collect();
            let pos = self.sector_pos(sid);
            self.write_at(pos, &table);
            sid = *self.difat[i].last().expect("a DIFAT sector is never empty");
        }
    }

    /// Every FAT sector, in the order the DIFAT lists them.
    fn fat_sectors(&self) -> Vec<SectorId> {
        self.iter_difat()
            .into_iter()
            .map(|(_, _, sid)| sid)
            .filter(|sid| super::sector::is_regular(*sid))
            .collect()
    }

    fn write_fat(&mut self) {
        let per_sector = self.sector_size as usize / 4;
        for (i, sid) in self.fat_sectors().into_iter().enumerate() {
            let table: Vec<u8> = self.fat[i * per_sector..(i + 1) * per_sector]
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect();
            let pos = self.sector_pos(sid);
            self.write_at(pos, &table);
        }
    }

    fn write_minifat(&mut self) {
        let per_sector = self.sector_size as usize / 4;
        let chain = self.fat_chain(self.minifat_sector_start, false);
        for (i, sid) in chain.into_iter().enumerate() {
            let table: Vec<u8> = self.minifat[i * per_sector..(i + 1) * per_sector]
                .iter()
                .flat_map(|s| s.to_le_bytes())
                .collect();
            let pos = self.sector_pos(sid);
            self.write_at(pos, &table);
        }
    }

    fn write_dir_entries(&mut self) {
        for id in 0..self.entries.len() as u32 {
            // Slots never handed out keep whatever was written there when
            // their sector was: zeros, for the rest of the first sector.
            if self.dir_freelist.contains(&id) || self.entries[id as usize].kind == 0 {
                continue;
            }
            let data = self.entries[id as usize].encode();
            let pos = self.dir_entry_pos(id);
            self.write_at(pos, &data);
        }
        let mut free: Vec<u32> = self.dir_freelist.iter().copied().collect();
        free.sort_unstable();
        for id in free {
            let pos = self.dir_entry_pos(id);
            self.write_at(pos, &[0u8; 128]);
        }
    }

    fn dir_entry_pos(&self, id: u32) -> u64 {
        let stream_pos = u64::from(id) * 128;
        let index = (stream_pos / u64::from(self.sector_size)) as usize;
        let offset = stream_pos % u64::from(self.sector_size);
        self.sector_pos(self.dir_fat_chain[index]) + offset
    }

    // --- allocation -----------------------------------------------------

    /// The DIFAT's entries as `(table, index, sector)`, skipping the pointer
    /// to the next table at the end of each DIFAT sector.
    fn iter_difat(&self) -> Vec<(usize, usize, SectorId)> {
        let mut out = Vec::new();
        for (i, sid) in self.difat[0].iter().enumerate() {
            out.push((0, i, *sid));
        }
        for (t, table) in self.difat.iter().enumerate().skip(1) {
            for (i, sid) in table[..table.len() - 1].iter().enumerate() {
                out.push((t, i, *sid));
            }
        }
        out
    }

    /// Follows a chain from `start`, as pyaaf2's `get_fat_chain` does.
    fn fat_chain(&self, start: SectorId, mini: bool) -> Vec<SectorId> {
        if matches!(start, END_OF_CHAIN | FREE | DIFAT | FAT) {
            return Vec::new();
        }
        let fat = if mini { &self.minifat } else { &self.fat };
        let mut chain = Vec::new();
        let mut sid = start;
        while sid != END_OF_CHAIN && chain.len() <= fat.len() {
            chain.push(sid);
            sid = fat[sid as usize];
        }
        chain
    }

    fn next_free_sect(&mut self) -> Result<SectorId> {
        loop {
            if let Some(i) = self.fat_freelist.pop_front() {
                if i == RANGE_LOCK_SECTOR && self.sector_size == 4096 {
                    self.fat[i as usize] = END_OF_CHAIN;
                    continue;
                }
                return Ok(i);
            }
            self.grow_fat();
        }
    }

    /// Adds a sector to the FAT, and a sector to the DIFAT first if the one
    /// there is full. pyaaf2's `next_free_sect` past its free list.
    fn grow_fat(&mut self) {
        let mut slot = self
            .iter_difat()
            .into_iter()
            .find(|(_, _, v)| *v == FREE)
            .map(|(t, i, _)| (t, i));

        let mut new_difat_sect = None;
        if slot.is_none() {
            let sect = self.fat.len() as u32 + 1;
            new_difat_sect = Some(sect);
            if self.difat_sector_count == 0 {
                self.difat_sector_start = sect;
                self.difat_sector_count = 1;
            } else {
                let last = self
                    .difat
                    .last_mut()
                    .expect("the header DIFAT is always there");
                *last.last_mut().expect("a DIFAT table is never empty") = sect;
                self.difat_sector_count += 1;
            }
            // pyaaf2 means to mark the new table's last slot as the end of
            // the chain, but compares instead of assigning, so it stays free.
            self.difat.push(vec![FREE; self.sector_size as usize / 4]);
            slot = self
                .iter_difat()
                .into_iter()
                .find(|(_, _, v)| *v == FREE)
                .map(|(t, i, _)| (t, i));
        }

        let (table, index) = slot.expect("a fresh DIFAT table has free slots");
        let new_fat_sect = self.fat.len() as u32;
        self.difat[table][index] = new_fat_sect;

        let start = self.fat.len() as u32;
        let end = start + self.sector_size / 4;
        self.fat.resize(end as usize, FREE);

        let mut reserved = vec![new_fat_sect];
        reserved.extend(new_difat_sect);
        if RANGE_LOCK_SECTOR < end && RANGE_LOCK_SECTOR > start && self.sector_size == 4096 {
            reserved.push(RANGE_LOCK_SECTOR);
            self.fat[RANGE_LOCK_SECTOR as usize] = END_OF_CHAIN;
        }
        self.fat_freelist
            .extend((start..end).filter(|i| !reserved.contains(i)));

        self.fat[new_fat_sect as usize] = FAT;
        self.fat_sector_count += 1;
        if let Some(sect) = new_difat_sect {
            self.fat[sect as usize] = DIFAT;
        }
    }

    fn next_free_minifat_sect(&mut self) -> Result<SectorId> {
        loop {
            let per_sector = u64::from(self.sector_size) / MINI_SECTOR_SIZE;
            let stream_sects = self.mini_stream_chain.len() as u64 * per_sector;

            if let Some(i) = self.minifat_freelist.pop_front() {
                if u64::from(i) + 1 > stream_sects {
                    self.mini_stream_grow()?;
                }
                return Ok(i);
            }

            let sid = self.next_free_sect()?;
            let start = self.minifat.len() as u32;
            let end = start + self.sector_size / 4;
            self.minifat.resize(end as usize, FREE);
            self.minifat_freelist.extend(start..end);

            if self.minifat_sector_count == 0 {
                self.minifat_sector_count = 1;
                self.minifat_sector_start = sid;
            } else {
                self.minifat_sector_count += 1;
                let last = *self
                    .minifat_chain
                    .last()
                    .expect("the mini FAT has a sector");
                self.fat[last as usize] = sid;
            }
            self.minifat_chain.push(sid);
            self.fat[sid as usize] = END_OF_CHAIN;
        }
    }

    fn mini_stream_grow(&mut self) -> Result<()> {
        let sid = self.next_free_sect()?;
        match self.mini_stream_chain.last() {
            None => {
                self.mini_stream_chain.push(sid);
                self.entries[0].sector = Some(sid);
            }
            Some(&last) => {
                self.fat[last as usize] = sid;
                self.mini_stream_chain.push(sid);
            }
        }
        self.fat[sid as usize] = END_OF_CHAIN;
        Ok(())
    }

    fn fat_chain_append(&mut self, start: Option<SectorId>, mini: bool) -> Result<SectorId> {
        let sect = if mini {
            self.next_free_minifat_sect()?
        } else {
            self.next_free_sect()?
        };
        if let Some(start) = start {
            let last = *self
                .fat_chain(start, mini)
                .last()
                .ok_or(Error::Unrepresentable {
                    what: "appending to an empty sector chain",
                })?;
            self.table_mut(mini)[last as usize] = sect;
        }
        self.table_mut(mini)[sect as usize] = END_OF_CHAIN;
        Ok(sect)
    }

    fn table_mut(&mut self, mini: bool) -> &mut Vec<SectorId> {
        if mini {
            &mut self.minifat
        } else {
            &mut self.fat
        }
    }

    fn free_fat_chain(&mut self, start: Option<SectorId>, mini: bool) {
        let Some(start) = start else { return };
        for sid in self.fat_chain(start, mini) {
            self.table_mut(mini)[sid as usize] = FREE;
            if mini {
                self.minifat_freelist.push_front(sid);
            } else {
                self.fat_freelist.push_front(sid);
            }
        }
    }

    // --- directory ------------------------------------------------------

    fn node(&self, id: u32) -> Result<&Node> {
        self.entries
            .get(id as usize)
            .ok_or(Error::DirEntryOutOfRange {
                id,
                count: self.entries.len() as u32,
            })
    }

    fn node_mut(&mut self, id: u32) -> Result<&mut Node> {
        let count = self.entries.len() as u32;
        self.entries
            .get_mut(id as usize)
            .ok_or(Error::DirEntryOutOfRange { id, count })
    }

    fn next_free_dir_id(&mut self) -> Result<u32> {
        if let Some(id) = self.dir_freelist.pop_front() {
            return Ok(id);
        }
        let last = *self
            .dir_fat_chain
            .last()
            .expect("the directory has a sector");
        let sect = self.fat_chain_append(Some(last), false)?;
        self.dir_fat_chain.push(sect);
        self.dir_sector_count += 1;

        let per_sector = self.sector_size / 128;
        let first = (self.dir_fat_chain.len() as u32 - 1) * per_sector;
        self.dir_freelist.extend(first..first + per_sector);
        Ok(self
            .dir_freelist
            .pop_front()
            .expect("a new directory sector has free slots"))
    }

    fn create_entry(
        &mut self,
        parent: DirId,
        name: &str,
        kind: u8,
        class_id: Option<Auid>,
    ) -> Result<DirId> {
        if !matches!(self.node(parent.0)?.kind, TYPE_STORAGE | TYPE_ROOT) {
            return Err(Error::WrongEntryType {
                id: parent.0,
                expected: "storage",
            });
        }
        if self.get(parent, name).is_some() {
            let sep = if parent == ROOT_ID { "" } else { "/" };
            return Err(Error::EntryExists {
                path: format!("{}{sep}{name}", self.path(parent)),
            });
        }
        if name.encode_utf16().count() > 32 {
            return Err(Error::BadName {
                name: name.to_owned(),
            });
        }

        let id = self.next_free_dir_id()?;
        let mut node = Node::blank();
        node.name = name.to_owned();
        node.kind = kind;
        node.class_id = class_id;
        if self.entries.len() <= id as usize {
            self.entries.resize(id as usize + 1, Node::blank());
        }
        self.entries[id as usize] = node;
        self.add_child(parent.0, id)?;
        self.children
            .entry(parent.0)
            .or_default()
            .insert(name.to_owned(), id);
        Ok(DirId(id))
    }

    fn add_child(&mut self, parent: u32, entry: u32) -> Result<()> {
        self.entries[entry as usize].parent = Some(parent);
        self.entries[entry as usize].red = false;
        if self.entries[parent as usize].child.is_none() {
            self.entries[parent as usize].child = Some(entry);
            Ok(())
        } else {
            self.insert(parent, entry)
        }
    }

    fn link(&self, node: Link, side: usize) -> Option<u32> {
        let n = match node {
            Link::Head => &self.head,
            Link::Entry(i) => &self.entries[i as usize],
        };
        if side == 0 { n.left } else { n.right }
    }

    fn set_link(&mut self, node: Link, side: usize, value: Option<u32>) {
        let n = match node {
            Link::Head => &mut self.head,
            Link::Entry(i) => &mut self.entries[i as usize],
        };
        if side == 0 {
            n.left = value;
        } else {
            n.right = value;
        }
    }

    fn is_red(&self, node: Option<Link>) -> bool {
        match node {
            None => false,
            Some(Link::Head) => self.head.red,
            Some(Link::Entry(i)) => self.entries[i as usize].red,
        }
    }

    fn set_red(&mut self, node: Link, red: bool) {
        match node {
            Link::Head => self.head.red = red,
            Link::Entry(i) => self.entries[i as usize].red = red,
        }
    }

    /// pyaaf2's `jsw_single`: one rotation, recolouring as it goes.
    fn single(&mut self, root: u32, direction: usize) -> Result<u32> {
        let other = 1 - direction;
        let new_root = self.link(Link::Entry(root), other).ok_or(LOST)?;
        let inner = self.link(Link::Entry(new_root), direction);
        self.set_link(Link::Entry(root), other, inner);
        self.set_link(Link::Entry(new_root), direction, Some(root));
        self.entries[root as usize].red = true;
        self.entries[new_root as usize].red = false;
        Ok(new_root)
    }

    /// pyaaf2's `jsw_double`: two rotations.
    fn double(&mut self, root: u32, direction: usize) -> Result<u32> {
        let other = 1 - direction;
        let child = self.link(Link::Entry(root), other).ok_or(LOST)?;
        let rotated = self.single(child, other)?;
        self.set_link(Link::Entry(root), other, Some(rotated));
        self.single(root, direction)
    }

    /// Whether entry `a` sorts before entry `b`, as pyaaf2's `DirEntry.__lt__`
    /// has it: shorter names first, then case-insensitively.
    ///
    /// pyaaf2 measures names in code points, where the format itself counts
    /// UTF-16 units. They differ only outside the Basic Multilingual Plane,
    /// where this follows pyaaf2.
    fn less(&self, a: u32, b: u32) -> bool {
        let a = &self.entries[a as usize].name;
        let b = &self.entries[b as usize].name;
        let (la, lb) = (a.chars().count(), b.chars().count());
        if la == lb {
            a.to_uppercase() < b.to_uppercase()
        } else {
            la < lb
        }
    }

    /// Inserts `entry` into `storage`'s tree of children: pyaaf2's
    /// `DirEntry.insert`, a top-down red-black insertion after Julienne
    /// Walker's, including pyaaf2's own bookkeeping of the ancestors it keeps.
    fn insert(&mut self, storage: u32, entry: u32) -> Result<()> {
        let max_entries = self.dir_sector_count * (self.sector_size / 128);

        self.head = Node::blank();
        self.head.red = true;
        self.entries[entry as usize].red = true;

        let entry_link = Link::Entry(entry);
        let mut gggp: Option<Link> = None;
        let mut ggp: Option<Link> = Some(Link::Head);
        let mut gp: Option<Link> = None;
        let mut parent: Option<Link> = None;
        let mut direction = 0usize;
        let mut last = 0usize;

        let mut node: Option<Link> = self.entries[storage as usize].child.map(Link::Entry);
        self.entries[storage as usize].child = None;
        self.head.right = match node {
            Some(Link::Entry(i)) => Some(i),
            _ => return Err(LOST),
        };

        let as_id = |link: Option<Link>| match link {
            Some(Link::Entry(i)) => Some(i),
            _ => None,
        };
        let child = |w: &Self, n: Link, side: usize| w.link(n, side).map(Link::Entry);

        let mut count = 0;
        while count < max_entries {
            match node {
                None => {
                    node = Some(entry_link);
                    self.set_link(parent.ok_or(LOST)?, direction, Some(entry));
                }
                Some(n) => {
                    let (l, r) = (child(self, n, 0), child(self, n, 1));
                    if self.is_red(l) && self.is_red(r) {
                        // Colour flip.
                        self.set_red(n, true);
                        self.set_red(l.ok_or(LOST)?, false);
                        self.set_red(r.ok_or(LOST)?, false);
                    }
                }
            }

            // Fix a red violation.
            if self.is_red(node) && self.is_red(parent) {
                let g = ggp.ok_or(LOST)?;
                let direction2 = if child(self, g, 0) == gp {
                    0
                } else if child(self, g, 1) == gp {
                    1
                } else {
                    return Err(LOST);
                };
                let p = parent.ok_or(LOST)?;
                let grand = as_id(gp).ok_or(LOST)?;
                if node == child(self, p, last) {
                    let rotated = self.single(grand, 1 - last)?;
                    self.set_link(g, direction2, Some(rotated));
                    gp = ggp;
                    ggp = gggp;
                } else if node == child(self, p, 1 - last) {
                    let rotated = self.double(grand, 1 - last)?;
                    self.set_link(g, direction2, Some(rotated));
                    parent = ggp;
                    gp = if parent == Some(Link::Head) {
                        None
                    } else {
                        gggp
                    };
                    ggp = None;
                } else {
                    return Err(LOST);
                }
            }

            if node == Some(entry_link) {
                break;
            }

            let n = as_id(node).ok_or(LOST)?;
            last = direction;
            direction = usize::from(!self.less(entry, n));

            if ggp.is_some() {
                gggp = ggp;
            }
            if gp.is_some() {
                ggp = gp;
            }
            gp = parent;
            parent = node;
            node = child(self, Link::Entry(n), direction);
            count += 1;
        }

        if count >= max_entries {
            return Err(Error::Unrepresentable {
                what: "a storage has more children than the directory holds",
            });
        }

        let root = self.head.right.ok_or(LOST)?;
        self.entries[storage as usize].child = Some(root);
        self.entries[root as usize].red = false;
        Ok(())
    }

    // --- streams --------------------------------------------------------

    fn check_stream(&self, id: DirId) -> Result<()> {
        if self.node(id.0)?.kind == TYPE_STREAM {
            Ok(())
        } else {
            Err(Error::WrongEntryType {
                id: id.0,
                expected: "stream",
            })
        }
    }

    /// Empties a stream, as opening an existing one for writing does.
    fn empty_stream(&mut self, id: u32) -> Result<()> {
        let node = self.node(id)?;
        let (sector, mini) = (node.sector, node.byte_size < MINI_STREAM_CUTOFF);
        self.free_fat_chain(sector, mini);
        let node = self.node_mut(id)?;
        node.sector = None;
        node.byte_size = 0;
        node.class_id = None;
        Ok(())
    }

    fn is_mini(&self, id: u32) -> bool {
        let node = &self.entries[id as usize];
        node.kind != TYPE_ROOT && node.byte_size < MINI_STREAM_CUTOFF
    }

    /// A new position at the start of a stream: pyaaf2's `Stream.__init__`.
    fn cursor(&self, id: u32) -> Cursor {
        let chain = match self.entries[id as usize].sector {
            Some(start) => self.fat_chain(start, self.is_mini(id)),
            None => Vec::new(),
        };
        Cursor { id, pos: 0, chain }
    }

    fn stream_read(&self, cursor: &mut Cursor) -> Vec<u8> {
        let byte_size = self.entries[cursor.id as usize].byte_size;
        let mut to_read = byte_size.saturating_sub(cursor.pos);
        let mut out = Vec::with_capacity(to_read as usize);
        let full = u64::from(self.sector_size);
        let mini = byte_size < MINI_STREAM_CUTOFF;

        while to_read > 0 {
            let (sector, offset, available) = if mini {
                let index = (cursor.pos / MINI_SECTOR_SIZE) as usize;
                let within = cursor.pos % MINI_SECTOR_SIZE;
                let stream_pos = u64::from(cursor.chain[index]) * MINI_SECTOR_SIZE + within;
                let sector = self.mini_stream_chain[(stream_pos / full) as usize];
                (sector, stream_pos % full, MINI_SECTOR_SIZE - within)
            } else {
                let index = (cursor.pos / full) as usize;
                let within = cursor.pos % full;
                (cursor.chain[index], within, full - within)
            };
            let n = to_read.min(available);
            let data = self.read_sector(sector);
            out.extend_from_slice(&data[offset as usize..(offset + n) as usize]);
            cursor.pos += n;
            to_read -= n;
        }
        out
    }

    /// pyaaf2's `Stream.allocate`: grows a stream to `byte_size`, moving it
    /// out of the mini stream first if it has reached the cutoff.
    fn stream_allocate(&mut self, cursor: &mut Cursor, byte_size: u64) -> Result<()> {
        let id = cursor.id;
        let mut mini = self.is_mini(id);
        let mut realloc = None;
        let mut orig_pos = 0;

        if mini && byte_size >= MINI_STREAM_CUTOFF {
            orig_pos = cursor.pos;
            cursor.pos = 0;
            let data = self.stream_read(cursor);
            let sector = self.entries[id as usize].sector;
            self.free_fat_chain(sector, true);
            self.entries[id as usize].sector = None;
            mini = false;
            cursor.chain.clear();
            realloc = Some(data);
        }

        self.entries[id as usize].byte_size = byte_size;
        let sector_size = if self.is_mini(id) {
            MINI_SECTOR_SIZE
        } else {
            u64::from(self.sector_size)
        };
        let count = byte_size.div_ceil(sector_size) as usize;

        while cursor.chain.len() < count {
            let last = cursor.chain.last().copied();
            let sid = self.fat_chain_append(last, mini)?;
            cursor.chain.push(sid);
            if self.entries[id as usize].sector.is_none() {
                self.entries[id as usize].sector = Some(sid);
            }
        }

        if let Some(data) = realloc {
            cursor.pos = 0;
            self.stream_write(cursor, &data)?;
            cursor.pos = orig_pos.min(data.len() as u64);
        }
        Ok(())
    }

    /// pyaaf2's `Stream.write`.
    fn stream_write(&mut self, cursor: &mut Cursor, data: &[u8]) -> Result<()> {
        let current = self.entries[cursor.id as usize].byte_size;
        let new_size = (cursor.pos + data.len() as u64).max(current);
        if new_size > current {
            self.stream_allocate(cursor, new_size)?;
        }

        let mini = self.entries[cursor.id as usize].byte_size < MINI_STREAM_CUTOFF;
        let full = u64::from(self.sector_size);
        let mut rest = data;
        while !rest.is_empty() {
            let (seek, writable) = if mini {
                let index = (cursor.pos / MINI_SECTOR_SIZE) as usize;
                let within = cursor.pos % MINI_SECTOR_SIZE;
                let stream_pos = u64::from(cursor.chain[index]) * MINI_SECTOR_SIZE + within;
                let sector = *self
                    .mini_stream_chain
                    .get((stream_pos / full) as usize)
                    .ok_or(Error::Unrepresentable {
                        what: "the mini stream would have to grow by more than a sector",
                    })?;
                (
                    self.sector_pos(sector) + stream_pos % full,
                    MINI_SECTOR_SIZE - within,
                )
            } else {
                let index = (cursor.pos / full) as usize;
                let within = cursor.pos % full;
                (self.sector_pos(cursor.chain[index]) + within, full - within)
            };
            let n = (rest.len() as u64).min(writable) as usize;
            self.write_at(seek, &rest[..n]);
            cursor.pos += n as u64;
            rest = &rest[n..];
        }
        Ok(())
    }

    /// pyaaf2's `Stream.truncate`, at `size` or at the current position.
    fn stream_truncate(&mut self, cursor: &mut Cursor, size: Option<u64>) -> Result<()> {
        let id = cursor.id;
        let size = size.unwrap_or(cursor.pos);
        let current = self.entries[id as usize].byte_size;
        let mini = self.is_mini(id);

        if size == 0 {
            let sector = self.entries[id as usize].sector;
            self.free_fat_chain(sector, mini);
            cursor.pos = 0;
            cursor.chain.clear();
            let node = &mut self.entries[id as usize];
            node.sector = None;
            node.byte_size = 0;
            return Ok(());
        }

        if size > current {
            return self.stream_allocate(cursor, size);
        }

        if size < MINI_STREAM_CUTOFF && !mini && self.entries[id as usize].kind != TYPE_ROOT {
            let orig_pos = cursor.pos;
            cursor.pos = 0;
            let mut data = self.stream_read(cursor);
            data.truncate(size as usize);
            let sector = self.entries[id as usize].sector;
            self.free_fat_chain(sector, false);
            cursor.pos = 0;
            cursor.chain.clear();
            let node = &mut self.entries[id as usize];
            node.sector = None;
            node.byte_size = 0;
            self.stream_write(cursor, &data)?;
            cursor.pos = orig_pos.min(size);
            return Ok(());
        }

        let sector_size = if mini {
            MINI_SECTOR_SIZE
        } else {
            u64::from(self.sector_size)
        };
        let count = size.div_ceil(sector_size) as usize;
        if cursor.chain.len() > count {
            let last = cursor.chain[count - 1];
            self.free_fat_chain(Some(cursor.chain[count]), mini);
            self.table_mut(mini)[last as usize] = END_OF_CHAIN;
            cursor.chain.truncate(count);
        }
        self.entries[id as usize].byte_size = size;
        cursor.pos = cursor.pos.min(size);
        Ok(())
    }
}

/// The error for a tree insertion that has lost track of where it is, which
/// pyaaf2 raises as a bare `CompoundFileBinaryError`.
const LOST: Error = Error::Unrepresentable {
    what: "a directory tree insertion lost its place",
};

#[cfg(test)]
mod tests {
    use std::io::Cursor as IoCursor;

    use super::*;
    use crate::cfb::{CompoundFile, DirId, cmp_names};

    fn read_back(bytes: Vec<u8>) -> CompoundFile<IoCursor<Vec<u8>>> {
        CompoundFile::open(IoCursor::new(bytes)).expect("the written file reads back")
    }

    #[test]
    fn an_empty_file_is_three_sectors() {
        let bytes = CompoundFileWriter::new(4096).unwrap().finish().unwrap();
        // Header, directory, FAT.
        assert_eq!(bytes.len(), 3 * 4096);
        let file = read_back(bytes);
        assert!(file.warnings().is_empty());
        assert_eq!(file.entries().len(), 32);
    }

    #[test]
    fn streams_on_either_side_of_the_cutoff_read_back() {
        for sector_size in [512, 4096] {
            let mut w = CompoundFileWriter::new(sector_size).unwrap();
            let dir = w.create_storage(ROOT_ID, "dir", None).unwrap();
            let small = w.touch(dir, "small").unwrap();
            let big = w.touch(dir, "big").unwrap();
            let small_data: Vec<u8> = (0..100u8).collect();
            let big_data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
            w.write_stream(small, &small_data).unwrap();
            w.write_stream(big, &big_data).unwrap();

            let mut file = read_back(w.finish().unwrap());
            assert!(file.warnings().is_empty(), "{:?}", file.warnings());
            let id = file.find("/dir/small").unwrap();
            assert_eq!(file.read_stream(id).unwrap(), small_data);
            let id = file.find("/dir/big").unwrap();
            assert_eq!(file.read_stream(id).unwrap(), big_data);
        }
    }

    #[test]
    fn a_stream_appended_past_the_cutoff_moves_out_of_the_mini_stream() {
        let mut w = CompoundFileWriter::new(512).unwrap();
        let id = w.touch(ROOT_ID, "grows").unwrap();
        let mut expected = Vec::new();
        for i in 0..100u32 {
            let piece = vec![i as u8; 97];
            w.append_stream(id, &piece).unwrap();
            expected.extend(piece);
        }
        let mut file = read_back(w.finish().unwrap());
        let id = file.find("/grows").unwrap();
        assert_eq!(file.read_stream(id).unwrap(), expected);
    }

    #[test]
    fn many_children_keep_a_valid_tree() {
        let mut w = CompoundFileWriter::new(512).unwrap();
        let mut names = Vec::new();
        for i in 0..300 {
            let name = format!("entry{}", (i * 7919) % 1000);
            w.create_storage(ROOT_ID, &name, None).unwrap();
            names.push(name);
        }
        let file = read_back(w.finish().unwrap());
        let children = file.children(ROOT_ID).unwrap();
        assert_eq!(children.len(), names.len());
        for name in names {
            assert!(file.find(&format!("/{name}")).is_some(), "{name}");
        }
    }

    /// `CompoundFile::child` searches a storage's tree by `cmp_names`, so
    /// every tree the writer builds has to be a search tree in that order:
    /// everything left of an entry sorts before it, everything right after.
    #[test]
    fn trees_are_ordered_by_cmp_names() {
        fn check(file: &CompoundFile<IoCursor<Vec<u8>>>, id: Option<DirId>) -> Vec<String> {
            let Some(id) = id else { return Vec::new() };
            let entry = file.entry(id).unwrap();
            let mut names = check(file, entry.left);
            names.push(entry.name.clone());
            names.extend(check(file, entry.right));
            names
        }

        let mut w = CompoundFileWriter::new(512).unwrap();
        for i in 0..200u32 {
            // Mixed lengths and mixed case, where length order and
            // case-insensitive order both matter.
            let name = format!(
                "{}{}",
                if i % 3 == 0 { "N" } else { "n" },
                (i * 7919) % 1009
            );
            let dir = w.create_storage(ROOT_ID, &name, None).unwrap();
            for j in 0..(i % 5) {
                w.touch(dir, &format!("s{}", (j * 31 + i) % 97)).unwrap();
            }
        }
        let file = read_back(w.finish().unwrap());
        for entry in file.entries() {
            if !entry.entry_type.is_storage() {
                continue;
            }
            let in_order = check(&file, entry.child);
            assert!(
                in_order
                    .windows(2)
                    .all(|w| cmp_names(&w[0], &w[1]) == std::cmp::Ordering::Less),
                "{} is not ordered: {in_order:?}",
                entry.name
            );
        }
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let mut w = CompoundFileWriter::new(4096).unwrap();
        w.create_storage(ROOT_ID, "a", None).unwrap();
        assert!(matches!(
            w.create_storage(ROOT_ID, "a", None),
            Err(Error::EntryExists { .. })
        ));
    }
}

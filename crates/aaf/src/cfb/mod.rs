//! Reading Microsoft Compound File Binary files, the container AAF is stored in.
//!
//! An AAF file is a compound file: a filesystem inside a single file, with
//! *storages* for directories and *streams* for files. Microsoft's Office 97
//! formats, MSI packages and AAF all use it. This module implements the
//! container alone and knows nothing about AAF's object model; that sits on
//! top, in the rest of this crate.
//!
//! # Layout
//!
//! The file is cut into equal sectors, 512 bytes in version 3 or 4096 in
//! version 4, numbered from the first sector after the 512-byte header. A
//! *file allocation table* (FAT) gives, for each sector, the next sector of
//! whatever chain it belongs to, so a stream is a linked list of sectors.
//!
//! Streams shorter than 4096 bytes would waste most of a sector each, so they
//! go in the *mini stream* instead: one ordinary stream, owned by the root
//! storage, subdivided into 64-byte mini sectors and chained by a second
//! table, the mini FAT.
//!
//! The FAT is itself stored in sectors, and the list of those sectors — the
//! DIFAT — begins in the header and continues in sectors of its own when it
//! outgrows the 109 slots the header has.
//!
//! # Tolerance
//!
//! Real AAF files, written by many applications over thirty years, disagree
//! with their own headers in small ways. Where a count in the header conflicts
//! with what the sector chains actually hold, this reader trusts the chains
//! and records a [`Warning`], rather than refusing the file. Upstream
//! `pyaaf2` does the same, and those files open in editing applications today.
//! Anything that would make a read ambiguous or unbounded is still an error.
//!
//! # Example
//!
//! ```no_run
//! use std::fs::File;
//! use aaf::cfb::CompoundFile;
//!
//! let mut file = CompoundFile::open(File::open("example.aaf")?)?;
//! let id = file.find("/Header-2/properties").expect("the header has properties");
//! let bytes = file.read_stream(id)?;
//! println!("{} bytes", bytes.len());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod dir_entry;
mod error;
mod header;
pub mod sector;
mod stream;

use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

pub use dir_entry::{Color, DirEntry, DirId, EntryType, ROOT_ID, cmp_names};
pub use error::{Error, Result};
pub use header::Header;
pub use stream::Stream;

use dir_entry::DIR_ENTRY_LEN;
use header::{HEADER_DIFAT_LEN, HEADER_LEN};
use sector::SectorId;

/// Something the file got wrong that this reader worked around.
///
/// None of these stop a file being read. They are kept so a caller that cares
/// about strictness — a validator, or a test comparing against another
/// implementation — can see what was tolerated.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Warning {
    /// The header's FAT sector count disagrees with the DIFAT.
    FatSectorCountMismatch {
        /// The count the header declares.
        declared: u32,
        /// The number of FAT sectors the DIFAT actually lists.
        found: u32,
    },

    /// The header's directory sector count disagrees with the chain.
    DirSectorCountMismatch {
        /// The count the header declares.
        declared: u32,
        /// The number of sectors the directory chain actually has.
        found: u32,
    },

    /// The root entry's stream length disagrees with the mini FAT.
    MiniStreamSizeMismatch {
        /// The length the root directory entry declares.
        declared: u64,
        /// The length implied by the last allocated mini sector.
        found: u64,
    },

    /// A sector reserved for a Windows byte-range lock holds data.
    ///
    /// Version 4 files must leave the sector covering byte `0x7FFFFF00`
    /// unallocated, so that an application can lock it without locking real
    /// data. Some writers allocate it anyway; nothing here depends on it.
    RangeLockSectorInUse {
        /// The sector in question.
        sector: SectorId,
    },
}

/// A compound file, opened for reading.
///
/// Open one with [`CompoundFile::open`], find entries by path with
/// [`find`](Self::find) or by walking from [`ROOT_ID`] with
/// [`children`](Self::children), and read bytes with
/// [`read_stream`](Self::read_stream) or [`open_stream`](Self::open_stream).
#[derive(Debug)]
pub struct CompoundFile<R> {
    reader: R,
    header: Header,
    fat: Vec<SectorId>,
    mini_fat: Vec<SectorId>,
    mini_stream_chain: Vec<SectorId>,
    entries: Vec<DirEntry>,
    /// The storage each entry sits in, by entry number. `None` for the root
    /// and for slots nothing points at.
    parents: Vec<Option<DirId>>,
    warnings: Vec<Warning>,
    cached_sector: Option<SectorId>,
    cache: Vec<u8>,
}

impl<R: Read + Seek> CompoundFile<R> {
    /// Opens a compound file, reading its header, allocation tables and
    /// directory.
    ///
    /// Stream contents are read on demand, so this is proportional to the
    /// file's structure rather than its size. Opening a multi-gigabyte AAF
    /// with embedded media costs the same as opening an empty one.
    ///
    /// # Errors
    ///
    /// Returns an error if the reader fails, if the file is not a compound
    /// file, if it uses a sector size other than 512 or 4096, or if its sector
    /// chains are cyclic or point outside the file's allocation tables.
    pub fn open(mut reader: R) -> Result<Self> {
        let mut raw = [0u8; HEADER_LEN];
        reader.seek(SeekFrom::Start(0))?;
        reader.read_exact(&mut raw)?;
        let header = Header::parse(&raw)?;

        let mut file = Self {
            reader,
            fat: Vec::new(),
            mini_fat: Vec::new(),
            mini_stream_chain: Vec::new(),
            entries: Vec::new(),
            parents: Vec::new(),
            warnings: Vec::new(),
            cached_sector: None,
            cache: vec![0u8; header.sector_size as usize],
            header,
        };

        file.read_fat()?;
        file.read_mini_fat()?;
        file.read_directory()?;
        file.read_parents()?;
        file.read_mini_stream_chain()?;
        Ok(file)
    }

    /// The file's header, as it was written.
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// The sector size this file uses, in bytes.
    #[must_use]
    pub const fn sector_size(&self) -> u32 {
        self.header.sector_size
    }

    /// The file allocation table: for each sector, the next one in its chain.
    ///
    /// Entries may be the markers in [`sector`] rather than sector numbers.
    #[must_use]
    pub fn fat(&self) -> &[SectorId] {
        &self.fat
    }

    /// The mini allocation table, which chains the 64-byte mini sectors.
    ///
    /// Mini sector numbers are offsets into the mini stream, not into the
    /// file. The mini stream itself is the root storage's own stream.
    #[must_use]
    pub fn mini_fat(&self) -> &[SectorId] {
        &self.mini_fat
    }

    /// Everything about the file this reader had to work around.
    ///
    /// Empty for a well-formed file.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Every directory entry in the file, including unused slots.
    ///
    /// Slots are addressed by [`DirId`], so the position in this slice is the
    /// entry's number.
    #[must_use]
    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    /// The root storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the file has no directory entries at all, which
    /// [`open`](Self::open) rejects, so this cannot fail on an opened file.
    pub fn root(&self) -> Result<&DirEntry> {
        self.entry(ROOT_ID)
    }

    /// One directory entry, by number.
    ///
    /// # Errors
    ///
    /// Returns an error if the file has no entry with that number.
    pub fn entry(&self, id: DirId) -> Result<&DirEntry> {
        self.entries
            .get(id.0 as usize)
            .ok_or(Error::DirEntryOutOfRange {
                id: id.0,
                count: self.entries.len() as u32,
            })
    }

    /// The entries directly inside a storage, ordered by name.
    ///
    /// The order is the format's own: shorter names first, then
    /// case-insensitively. See [`cmp_names`].
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not a storage, or if the storage's
    /// red-black tree of children is corrupt.
    pub fn children(&self, id: DirId) -> Result<Vec<&DirEntry>> {
        let mut found = self.unordered_children(id)?;
        found.sort_by(|a, b| cmp_names(&a.name, &b.name));
        Ok(found)
    }

    /// The entry of this name directly inside a storage, if there is one.
    ///
    /// A storage's children are a search tree ordered by [`cmp_names`], so
    /// this walks down the tree rather than listing every child: a storage
    /// can hold thousands, and looking each of them up by listing all of
    /// them made reading a file quadratic. The format requires the order,
    /// but a writer that got it wrong would hide a name from the walk, so a
    /// miss is checked against every child before it is believed.
    ///
    /// # Errors
    ///
    /// As [`children`](Self::children).
    pub fn child(&self, parent: DirId, name: &str) -> Result<Option<&DirEntry>> {
        let storage = self.storage(parent)?;
        let mut next = storage.child;
        // A walk longer than the directory has gone round a cycle, which the
        // listing below reports.
        for _ in 0..self.entries.len() {
            let Some(id) = next else { break };
            let entry = self.entry(id)?;
            next = match cmp_names(name, &entry.name) {
                std::cmp::Ordering::Equal => return Ok(Some(entry)),
                std::cmp::Ordering::Less => entry.left,
                std::cmp::Ordering::Greater => entry.right,
            };
        }
        Ok(self
            .unordered_children(parent)?
            .into_iter()
            .find(|entry| cmp_names(&entry.name, name).is_eq()))
    }

    /// A storage's entry, failing if the entry is not a storage.
    fn storage(&self, id: DirId) -> Result<&DirEntry> {
        let entry = self.entry(id)?;
        if !entry.is_storage() {
            return Err(Error::WrongEntryType {
                id: id.0,
                expected: "storage",
            });
        }
        Ok(entry)
    }

    /// The entries directly inside a storage, in the order the tree gives.
    fn unordered_children(&self, id: DirId) -> Result<Vec<&DirEntry>> {
        let parent = self.storage(id)?;
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = Vec::from_iter(parent.child);

        // The children are a red-black tree of siblings, not a list, so this
        // is a traversal rather than a walk down a chain. Each entry is
        // visited once; a repeat means the links form a cycle or a diamond.
        while let Some(child) = stack.pop() {
            if !seen.insert(child) {
                return Err(Error::CorruptDirectoryTree { id: child.0 });
            }
            let entry = self.entry(child)?;
            found.push(entry);
            stack.extend(entry.left);
            stack.extend(entry.right);
        }
        Ok(found)
    }

    /// Finds an entry by path, or `None` if there is nothing there.
    ///
    /// Paths are `/`-separated and may start with `/`; `/` alone is the root
    /// storage. Components are matched the way the format compares names,
    /// which is case-insensitively.
    #[must_use]
    pub fn find(&self, path: &str) -> Option<DirId> {
        let mut current = ROOT_ID;
        for component in path.split('/').filter(|c| !c.is_empty()) {
            current = self.child(current, component).ok()??.id;
        }
        Some(current)
    }

    /// The storage an entry sits in, or `None` for the root.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not an entry of this file.
    pub fn parent(&self, id: DirId) -> Result<Option<DirId>> {
        self.parents
            .get(id.0 as usize)
            .copied()
            .ok_or(Error::DirEntryOutOfRange {
                id: id.0,
                count: self.entries.len() as u32,
            })
    }

    /// The path of an entry, as [`find`](Self::find) would take it.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not an entry of this file, or if it is a
    /// slot no storage points at, which has no path.
    pub fn path(&self, id: DirId) -> Result<String> {
        let mut names = Vec::new();
        let mut current = id;
        while let Some(parent) = self.parent(current)? {
            names.push(self.entry(current)?.name.as_str());
            current = parent;
        }
        if current != ROOT_ID {
            return Err(Error::DirEntryOutOfRange {
                id: id.0,
                count: self.entries.len() as u32,
            });
        }
        if names.is_empty() {
            return Ok("/".to_owned());
        }
        names.reverse();
        Ok(format!("/{}", names.join("/")))
    }

    /// Every entry in the file, depth first from the root, with its path.
    ///
    /// Storages come before their contents, and siblings are in the order
    /// [`children`](Self::children) gives.
    ///
    /// # Errors
    ///
    /// Returns an error if any storage's tree of children is corrupt.
    pub fn walk(&self) -> Result<Vec<(String, DirId)>> {
        let mut out = Vec::new();
        self.walk_from(ROOT_ID, String::new(), &mut out)?;
        Ok(out)
    }

    /// Opens a stream for reading.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not an entry of this file, or if its sector
    /// chain is cyclic or points outside the allocation table.
    pub fn open_stream(&mut self, id: DirId) -> Result<Stream<'_, R>> {
        Stream::new(self, id)
    }

    /// Reads a whole stream into memory.
    ///
    /// # Errors
    ///
    /// Returns an error if `id` is not an entry of this file, if its sector
    /// chain is malformed, or if the underlying reader fails.
    pub fn read_stream(&mut self, id: DirId) -> Result<Vec<u8>> {
        self.open_stream(id)?.read_to_end_vec()
    }

    // --- opening ------------------------------------------------------------

    /// Reads the FAT, following the DIFAT to find the sectors holding it.
    fn read_fat(&mut self) -> Result<()> {
        let fat_sectors = self.read_difat()?;

        if fat_sectors.len() as u32 != self.header.fat_sector_count {
            self.warnings.push(Warning::FatSectorCountMismatch {
                declared: self.header.fat_sector_count,
                found: fat_sectors.len() as u32,
            });
        }

        let per_sector = self.header.sector_size as usize / 4;
        self.fat = Vec::with_capacity(fat_sectors.len() * per_sector);
        for sid in fat_sectors {
            let sector = self.sector(sid)?.to_vec();
            self.fat.extend(read_sids(&sector));
        }

        // Version 4 files must leave the range-lock sector free. Some writers
        // do not; nothing here depends on it, so just note it.
        if self.header.sector_size == 4096 {
            let range_lock = (0x7fff_ff00u32 / 4096) - 1;
            if self
                .fat
                .get(range_lock as usize)
                .is_some_and(|sid| *sid != sector::END_OF_CHAIN && *sid != sector::FREE)
            {
                self.warnings
                    .push(Warning::RangeLockSectorInUse { sector: range_lock });
            }
        }

        Ok(())
    }

    /// Collects the sectors holding the FAT, from the header and the DIFAT chain.
    fn read_difat(&mut self) -> Result<Vec<SectorId>> {
        let mut fat_sectors: Vec<SectorId> = self.header.difat_head[..HEADER_DIFAT_LEN]
            .iter()
            .copied()
            .filter(|sid| sector::is_regular(*sid))
            .collect();

        // The DIFAT continues in sectors of its own, each ending with a
        // pointer to the next. Walk it with a visited set: unlike the FAT
        // chains there is no table to check a sector number against here.
        let mut seen = HashSet::new();
        let mut sid = self.header.difat_sector_start;
        let mut left = self.header.difat_sector_count;
        while left > 0 && sector::is_regular(sid) {
            if !seen.insert(sid) {
                return Err(Error::CyclicChain {
                    mini: false,
                    start: self.header.difat_sector_start,
                });
            }
            let sector = self.sector(sid)?.to_vec();
            let entries: Vec<SectorId> = read_sids(&sector).collect();
            let (body, next) = entries.split_at(entries.len() - 1);
            fat_sectors.extend(body.iter().copied().filter(|s| sector::is_regular(*s)));
            sid = next[0];
            left -= 1;
        }

        Ok(fat_sectors)
    }

    fn read_mini_fat(&mut self) -> Result<()> {
        let chain = self.chain(self.header.mini_fat_sector_start, false)?;
        let per_sector = self.header.sector_size as usize / 4;
        self.mini_fat = Vec::with_capacity(chain.len() * per_sector);
        for sid in chain {
            let sector = self.sector(sid)?.to_vec();
            self.mini_fat.extend(read_sids(&sector));
        }
        Ok(())
    }

    fn read_directory(&mut self) -> Result<()> {
        let chain = self.chain(self.header.dir_sector_start, false)?;
        if !chain.is_empty()
            && self.header.dir_sector_count != 0
            && chain.len() as u32 != self.header.dir_sector_count
        {
            self.warnings.push(Warning::DirSectorCountMismatch {
                declared: self.header.dir_sector_count,
                found: chain.len() as u32,
            });
        }

        let per_sector = self.header.sector_size as usize / DIR_ENTRY_LEN;
        self.entries = Vec::with_capacity(chain.len() * per_sector);
        for sid in chain {
            let sector = self.sector(sid)?.to_vec();
            for raw in sector.chunks_exact(DIR_ENTRY_LEN) {
                let id = DirId(self.entries.len() as u32);
                self.entries.push(DirEntry::parse(id, raw)?);
            }
        }

        match self.entries.first() {
            Some(root) if root.is_root() => Ok(()),
            _ => Err(Error::MissingRootEntry),
        }
    }

    /// Works out which storage each entry sits in, once, at open time.
    ///
    /// Every storage's children are checked here too, so a file with a corrupt
    /// directory tree fails on open rather than on whichever listing happens
    /// to touch it first.
    fn read_parents(&mut self) -> Result<()> {
        let mut parents = vec![None; self.entries.len()];
        let mut stack = vec![ROOT_ID];
        let mut seen = HashSet::from([ROOT_ID]);

        while let Some(id) = stack.pop() {
            for child in self.children(id)? {
                if !seen.insert(child.id) {
                    return Err(Error::CorruptDirectoryTree { id: child.id.0 });
                }
                parents[child.id.0 as usize] = Some(id);
                if child.is_storage() {
                    stack.push(child.id);
                }
            }
        }

        self.parents = parents;
        Ok(())
    }

    /// Reads the chain of full sectors holding the mini stream.
    fn read_mini_stream_chain(&mut self) -> Result<()> {
        let root = self.root()?;
        let declared = root.stream_len;
        let Some(start) = root.start_sector else {
            return Ok(());
        };
        self.mini_stream_chain = self.chain(start, false)?;

        // The mini FAT says how many mini sectors are allocated, which gives a
        // second reading of the mini stream's length. They should agree.
        let allocated = self
            .mini_fat
            .iter()
            .rposition(|sid| *sid != sector::FREE)
            .map_or(0, |last| {
                (last as u64 + 1) * u64::from(sector::MINI_SECTOR_SIZE)
            });
        if allocated != declared {
            self.warnings.push(Warning::MiniStreamSizeMismatch {
                declared,
                found: allocated,
            });
        }
        Ok(())
    }

    // --- sectors ------------------------------------------------------------

    /// Follows a chain from `start` to its end, in order.
    ///
    /// A chain cannot be longer than the table that describes it without
    /// visiting some sector twice, so the length bound below is what stops a
    /// malformed file from looping forever.
    fn chain(&self, start: SectorId, mini: bool) -> Result<Vec<SectorId>> {
        let fat = if mini { &self.mini_fat } else { &self.fat };

        let mut chain = Vec::new();
        let mut sid = start;
        while sector::is_regular(sid) {
            if chain.len() >= fat.len() {
                return Err(Error::CyclicChain { mini, start });
            }
            chain.push(sid);
            sid = fat
                .get(sid as usize)
                .copied()
                .ok_or(Error::SectorOutOfRange {
                    sector: sid,
                    table_len: fat.len() as u32,
                })?;
        }
        Ok(chain)
    }

    /// Reads one sector, keeping the last one read to hand.
    ///
    /// A sector that runs past the end of the file reads as zeros. Files
    /// truncated at the last sector are common enough that refusing them would
    /// cost more than it gains, and the trailing bytes are padding anyway.
    fn sector(&mut self, sid: SectorId) -> Result<&[u8]> {
        if self.cached_sector != Some(sid) {
            self.cached_sector = None;
            self.cache.clear();
            self.cache.resize(self.header.sector_size as usize, 0);
            self.reader.seek(SeekFrom::Start(sector::offset(
                sid,
                self.header.sector_size,
            )))?;

            let mut filled = 0;
            while filled < self.cache.len() {
                match self.reader.read(&mut self.cache[filled..])? {
                    0 => break,
                    n => filled += n,
                }
            }
            self.cached_sector = Some(sid);
        }
        Ok(&self.cache)
    }

    // --- paths --------------------------------------------------------------

    fn walk_from(&self, id: DirId, prefix: String, out: &mut Vec<(String, DirId)>) -> Result<()> {
        let entry = self.entry(id)?;
        let path = if entry.is_root() {
            "/".to_owned()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        out.push((path.clone(), id));

        if entry.is_storage() {
            let prefix = if entry.is_root() { String::new() } else { path };
            for child in self.children(id)? {
                self.walk_from(child.id, prefix.clone(), out)?;
            }
        }
        Ok(())
    }
}

/// Reads a sector full of little-endian sector numbers.
fn read_sids(sector: &[u8]) -> impl Iterator<Item = SectorId> + '_ {
    sector
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().expect("slice is four bytes")))
}

/// Looks up the `index`th sector of a chain.
fn chain_entry(chain: &[SectorId], index: u64) -> Result<SectorId> {
    usize::try_from(index)
        .ok()
        .and_then(|i| chain.get(i))
        .copied()
        .ok_or(Error::SectorOutOfRange {
            sector: u32::try_from(index).unwrap_or(u32::MAX),
            table_len: chain.len() as u32,
        })
}

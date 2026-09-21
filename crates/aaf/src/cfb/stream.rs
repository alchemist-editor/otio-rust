//! Reading the bytes of a stream inside a compound file.

use std::io::{self, Read, Seek, SeekFrom};

use super::error::Result;
use super::sector::{self, SectorId};
use super::{CompoundFile, DirId};

/// A reader over one stream inside a compound file.
///
/// Obtained from [`CompoundFile::open_stream`]. It implements [`Read`] and
/// [`Seek`], so it can be handed to anything that takes an ordinary file.
/// Seeking past the end is allowed and reads nothing, matching a file opened
/// for reading.
///
/// The stream borrows the compound file mutably, because reading it moves the
/// underlying reader. To work with two streams at once, read one into memory
/// with [`CompoundFile::read_stream`] first.
#[derive(Debug)]
pub struct Stream<'a, R> {
    file: &'a mut CompoundFile<R>,
    /// The sectors of this stream, in order. Mini sectors when `is_mini`.
    chain: Vec<SectorId>,
    is_mini: bool,
    len: u64,
    pos: u64,
}

impl<'a, R: Read + Seek> Stream<'a, R> {
    pub(crate) fn new(file: &'a mut CompoundFile<R>, id: DirId) -> Result<Self> {
        let entry = file.entry(id)?;
        let len = entry.stream_len;
        // The root storage's own "stream" is the mini stream container, which
        // is always made of full sectors however short it is.
        let is_mini = !entry.is_root() && len < u64::from(file.header.mini_stream_cutoff);
        let start = entry.start_sector;

        let chain = match start {
            Some(start) => file.chain(start, is_mini)?,
            None => Vec::new(),
        };

        // The length on the entry and the chain in the allocation table say
        // the same thing twice. If they disagree the entry is corrupt, and
        // trusting it would mean reading sectors nothing allocated.
        let sector_size = if is_mini {
            u64::from(sector::MINI_SECTOR_SIZE)
        } else {
            u64::from(file.header.sector_size)
        };
        let allocated = chain.len() as u64 * sector_size;
        if len > allocated {
            return Err(super::Error::StreamLongerThanChain {
                id: id.get(),
                declared: len,
                allocated,
            });
        }

        Ok(Self {
            file,
            chain,
            is_mini,
            len,
            pos: 0,
        })
    }

    /// The length of this stream, in bytes.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether this stream is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether this stream lives in the mini stream rather than in full sectors.
    #[must_use]
    pub const fn is_mini(&self) -> bool {
        self.is_mini
    }

    /// Reads the whole stream from the current position.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying reader fails.
    pub fn read_to_end_vec(&mut self) -> Result<Vec<u8>> {
        // The stream's length was checked against its sector chain when it was
        // opened, so this allocation is bounded by what the file really holds.
        let remaining = usize::try_from(self.len.saturating_sub(self.pos)).map_err(|_| {
            io::Error::new(io::ErrorKind::OutOfMemory, "stream is too large to buffer")
        })?;
        let mut out = vec![0u8; remaining];

        let mut filled = 0;
        while filled < out.len() {
            match self.read_chunk(&mut out[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        out.truncate(filled);
        Ok(out)
    }

    /// Copies the next run of contiguous bytes into `buf`, returning how many.
    ///
    /// One call never crosses a sector boundary, because the next sector is
    /// somewhere else in the file.
    fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize> {
        let sector_size = u64::from(self.file.header.sector_size);
        let remaining = self.len.saturating_sub(self.pos);
        let wanted = remaining.min(buf.len() as u64);
        if wanted == 0 {
            return Ok(0);
        }

        let (sid, offset_in_sector, run) = if self.is_mini {
            let mini_size = u64::from(sector::MINI_SECTOR_SIZE);
            let mini_index = self.pos / mini_size;
            let offset_in_mini = self.pos % mini_size;

            // Mini sectors are numbered within the mini stream, which is
            // itself an ordinary stream of full sectors. Map through both.
            let mini_sid = u64::from(super::chain_entry(&self.chain, mini_index)?);
            let pos_in_mini_stream = mini_sid * mini_size + offset_in_mini;

            let full_index = pos_in_mini_stream / sector_size;
            let sid = super::chain_entry(&self.file.mini_stream_chain, full_index)?;

            // A mini sector never straddles a full sector: 512 and 4096 are
            // both multiples of 64, so this run stops at the mini sector's end.
            (
                sid,
                pos_in_mini_stream % sector_size,
                mini_size - offset_in_mini,
            )
        } else {
            let index = self.pos / sector_size;
            let offset = self.pos % sector_size;
            let sid = super::chain_entry(&self.chain, index)?;
            (sid, offset, sector_size - offset)
        };

        let count = usize::try_from(wanted.min(run)).expect("run is at most one sector");
        let start = usize::try_from(offset_in_sector).expect("offset is within a sector");
        let sector = self.file.sector(sid)?;
        buf[..count].copy_from_slice(&sector[start..start + count]);
        self.pos += count as u64;
        Ok(count)
    }
}

impl<R: Read + Seek> Read for Stream<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buf.len() {
            let count = self.read_chunk(&mut buf[written..])?;
            if count == 0 {
                break;
            }
            written += count;
        }
        Ok(written)
    }
}

impl<R: Read + Seek> Seek for Stream<'_, R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.pos) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.len) + i128::from(offset),
        };
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek to a negative position in a compound file stream",
            ));
        }
        self.pos = u64::try_from(target).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek past the largest representable position",
            )
        })?;
        Ok(self.pos)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.pos)
    }
}

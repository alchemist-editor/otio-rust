//! The 512-byte header at the start of every compound file.

use super::error::{Error, Result};
use super::sector::{self, SectorId};
use crate::Auid;

/// The eight bytes every compound file starts with.
pub(crate) const SIGNATURE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// The header is always 512 bytes, whatever the sector size is.
pub(crate) const HEADER_LEN: usize = 512;

/// The header holds the first 109 DIFAT entries inline.
pub(crate) const HEADER_DIFAT_LEN: usize = 109;

/// The parsed contents of a compound file header.
///
/// This is the on-disk header, not an interpretation of it: the sector counts
/// here are what the file *claims*, which is not always what it has. Where the
/// two disagree, [`CompoundFile::open`] trusts the sector chains and records the
/// discrepancy as a [`Warning`], because that is what makes real AAF files
/// written by real applications readable.
///
/// [`CompoundFile::open`]: super::CompoundFile::open
/// [`Warning`]: super::Warning
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Header {
    /// The class of the whole file, which AAF uses to identify itself.
    pub class_id: Auid,
    /// The format minor version. AAF writes 0x3E.
    pub minor_version: u16,
    /// The format major version: 3 for 512-byte sectors, 4 for 4096.
    pub major_version: u16,
    /// The sector size, in bytes: 512 or 4096.
    pub sector_size: u32,
    /// The mini sector size, in bytes. Always 64.
    pub mini_sector_size: u32,
    /// The number of directory sectors the header claims. Zero in version 3.
    pub dir_sector_count: u32,
    /// The number of FAT sectors the header claims.
    pub fat_sector_count: u32,
    /// The first sector of the directory chain.
    pub dir_sector_start: SectorId,
    /// The transaction signature, which AAF leaves at zero.
    pub transaction_signature: u32,
    /// Streams shorter than this live in the mini stream. Always 4096.
    pub mini_stream_cutoff: u32,
    /// The first sector of the mini FAT chain.
    pub mini_fat_sector_start: SectorId,
    /// The number of mini FAT sectors the header claims.
    pub mini_fat_sector_count: u32,
    /// The first sector of the DIFAT chain.
    pub difat_sector_start: SectorId,
    /// The number of DIFAT sectors the header claims.
    pub difat_sector_count: u32,
    /// The first 109 DIFAT entries, which live in the header itself.
    pub difat_head: [SectorId; HEADER_DIFAT_LEN],
}

impl Header {
    /// Parses a header out of the first 512 bytes of a file.
    pub(crate) fn parse(data: &[u8; HEADER_LEN]) -> Result<Self> {
        let signature: [u8; 8] = data[0..8].try_into().expect("slice is eight bytes");
        if signature != SIGNATURE {
            return Err(Error::BadSignature { found: signature });
        }

        let class_id = Auid::from_bytes_le(data[8..24].try_into().expect("slice is sixteen bytes"));
        let minor_version = u16(data, 24);
        let major_version = u16(data, 26);

        let byte_order = u16(data, 28);
        if byte_order != 0xfffe {
            return Err(Error::UnsupportedByteOrder { mark: byte_order });
        }

        // Both sizes are stored as the base-2 logarithm of the size.
        let sector_size = 1u32
            .checked_shl(u32::from(u16(data, 30)))
            .ok_or(Error::UnsupportedSectorSize { size: 0 })?;
        let mini_sector_size = 1u32
            .checked_shl(u32::from(u16(data, 32)))
            .ok_or(Error::UnsupportedMiniSectorSize { size: 0 })?;

        if !matches!(sector_size, 512 | 4096) {
            return Err(Error::UnsupportedSectorSize { size: sector_size });
        }
        if mini_sector_size != sector::MINI_SECTOR_SIZE {
            return Err(Error::UnsupportedMiniSectorSize {
                size: mini_sector_size,
            });
        }

        // Bytes 34..40 are reserved and ignored.
        let mut difat_head = [sector::FREE; HEADER_DIFAT_LEN];
        for (i, slot) in difat_head.iter_mut().enumerate() {
            *slot = u32(data, 76 + i * 4);
        }

        Ok(Self {
            class_id,
            minor_version,
            major_version,
            sector_size,
            mini_sector_size,
            dir_sector_count: u32(data, 40),
            fat_sector_count: u32(data, 44),
            dir_sector_start: u32(data, 48),
            transaction_signature: u32(data, 52),
            mini_stream_cutoff: u32(data, 56),
            mini_fat_sector_start: u32(data, 60),
            mini_fat_sector_count: u32(data, 64),
            difat_sector_start: u32(data, 68),
            difat_sector_count: u32(data, 72),
            difat_head,
        })
    }
}

fn u16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().expect("slice is two bytes"))
}

fn u32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("slice is four bytes"))
}

//! Sector numbering and the reserved sector values.

/// A sector number, counting from the first sector after the header.
///
/// Four values are reserved as markers rather than sector numbers: [`DIFAT`],
/// [`FAT`], [`END_OF_CHAIN`] and [`FREE`]. Use [`is_regular`] before treating
/// one as a position in the file.
pub type SectorId = u32;

/// The size of a mini sector, in bytes. The format fixes this at 64.
pub const MINI_SECTOR_SIZE: u32 = 64;

/// Chain marker: this sector holds part of the DIFAT.
pub const DIFAT: SectorId = 0xffff_fffc;

/// Chain marker: this sector holds part of the FAT.
pub const FAT: SectorId = 0xffff_fffd;

/// Chain marker: the chain ends here.
pub const END_OF_CHAIN: SectorId = 0xffff_fffe;

/// Chain marker: this sector is unallocated.
pub const FREE: SectorId = 0xffff_ffff;

/// The largest value that is a sector number rather than a marker.
pub const MAX_REGULAR: SectorId = 0xffff_fffa;

/// Whether `sid` is an actual sector number rather than a reserved marker.
#[must_use]
pub const fn is_regular(sid: SectorId) -> bool {
    sid <= MAX_REGULAR
}

/// The byte offset of sector `sid` in a file with sectors of `sector_size`.
///
/// Sector 0 begins one sector into the file, not 512 bytes in: with 4096-byte
/// sectors the 512-byte header is followed by 3584 bytes of padding.
#[must_use]
pub const fn offset(sid: SectorId, sector_size: u32) -> u64 {
    (sid as u64 + 1) * sector_size as u64
}

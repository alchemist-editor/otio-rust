//! The CRC-32 a zip archive records for each entry (IEEE 802.3, reflected).

/// The lookup table for the reflected polynomial `0xEDB88320`.
const TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// A running CRC-32, fed a chunk at a time.
#[derive(Debug, Clone, Copy)]
pub struct Crc32(u32);

impl Crc32 {
    /// A checksum of nothing yet.
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    /// Adds `bytes` to the checksum.
    pub fn update(&mut self, bytes: &[u8]) {
        let mut c = self.0;
        for &byte in bytes {
            c = TABLE[((c ^ u32::from(byte)) & 0xFF) as usize] ^ (c >> 8);
        }
        self.0 = c;
    }

    /// The checksum of everything added so far.
    pub const fn finish(self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

/// The CRC-32 of `bytes`.
pub fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(bytes);
    crc.finish()
}

#[cfg(test)]
mod tests {
    use super::checksum;

    #[test]
    fn matches_the_published_check_value() {
        // The standard check value for CRC-32/ISO-HDLC.
        assert_eq!(checksum(b"123456789"), 0xCBF4_3926);
        assert_eq!(checksum(b""), 0);
    }
}

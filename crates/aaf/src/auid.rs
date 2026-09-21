//! The 16-byte identifier AAF uses for classes, types, properties and objects.

use std::fmt;

/// A 16-byte AAF identifier.
///
/// AAF calls these AUIDs. They are UUIDs (RFC 4122) as far as their value is
/// concerned, but AAF stores them in the little-endian "GUID" layout that
/// Microsoft's COM uses: `Data1` as a little-endian `u32`, `Data2` and `Data3`
/// as little-endian `u16`s, and `Data4` as eight bytes in order. The textual
/// form is the usual big-endian UUID rendering, so the bytes on disk are not
/// in the order the string suggests.
///
/// This type stores the on-disk little-endian bytes and converts on the way in
/// and out, which is what upstream `pyaaf2`'s `AUID` does.
///
/// AAF also uses SMPTE Universal Labels, which are 16 bytes with no byte
/// swapping at all. Those are carried by the same type here, and the caller
/// decides which reading applies, exactly as upstream does.
///
/// # Example
///
/// ```
/// use aaf::Auid;
///
/// let id: Auid = "0d010101-0101-2f00-060e-2b3402060101".parse().unwrap();
/// assert_eq!(id.to_string(), "0d010101-0101-2f00-060e-2b3402060101");
/// assert_eq!(id.data1(), 0x0d01_0101);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Auid {
    /// The 16 bytes in AAF's on-disk little-endian order.
    bytes_le: [u8; 16],
}

impl Auid {
    /// The all-zero AUID, which AAF uses to mean "no identifier".
    pub const NIL: Self = Self { bytes_le: [0; 16] };

    /// Builds an AUID from the 16 bytes as they appear on disk.
    #[must_use]
    pub const fn from_bytes_le(bytes_le: [u8; 16]) -> Self {
        Self { bytes_le }
    }

    /// Builds an AUID from the 16 bytes in textual (big-endian) order.
    #[must_use]
    pub const fn from_bytes_be(b: [u8; 16]) -> Self {
        Self {
            bytes_le: [
                b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12],
                b[13], b[14], b[15],
            ],
        }
    }

    /// The 16 bytes as they appear on disk.
    #[must_use]
    pub const fn to_bytes_le(self) -> [u8; 16] {
        self.bytes_le
    }

    /// The 16 bytes in textual (big-endian) order.
    #[must_use]
    pub const fn to_bytes_be(self) -> [u8; 16] {
        let b = self.bytes_le;
        [
            b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13],
            b[14], b[15],
        ]
    }

    /// Whether this is the all-zero AUID.
    #[must_use]
    pub fn is_nil(self) -> bool {
        self.bytes_le == [0; 16]
    }

    /// The `Data1` field, the first and most significant group in the text form.
    #[must_use]
    pub const fn data1(self) -> u32 {
        u32::from_le_bytes([
            self.bytes_le[0],
            self.bytes_le[1],
            self.bytes_le[2],
            self.bytes_le[3],
        ])
    }

    /// The `Data2` field, the second group in the text form.
    #[must_use]
    pub const fn data2(self) -> u16 {
        u16::from_le_bytes([self.bytes_le[4], self.bytes_le[5]])
    }

    /// The `Data3` field, the third group in the text form.
    #[must_use]
    pub const fn data3(self) -> u16 {
        u16::from_le_bytes([self.bytes_le[6], self.bytes_le[7]])
    }

    /// The `Data4` field, the last eight bytes, stored in textual order.
    #[must_use]
    pub const fn data4(self) -> [u8; 8] {
        let b = self.bytes_le;
        [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]
    }
}

impl fmt::Display for Auid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.to_bytes_be();
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9]
        )?;
        for byte in &b[10..] {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The error returned when a string is not a well-formed AUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseAuidError {
    /// The rejected string.
    pub input: String,
}

impl fmt::Display for ParseAuidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "badly formed AUID string: '{}'", self.input)
    }
}

impl std::error::Error for ParseAuidError {}

impl std::str::FromStr for Auid {
    type Err = ParseAuidError;

    /// Parses the usual big-endian UUID text form.
    ///
    /// A `urn:` or `uuid:` prefix, surrounding braces and the hyphens are all
    /// optional, matching what upstream `pyaaf2` accepts.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseAuidError {
            input: s.to_owned(),
        };

        let trimmed = s
            .trim()
            .trim_start_matches("urn:")
            .trim_start_matches("uuid:")
            .trim_start_matches('{')
            .trim_end_matches('}');

        let mut bytes_be = [0u8; 16];
        let mut nibbles = trimmed.chars().filter(|c| *c != '-');
        for byte in &mut bytes_be {
            let hi = nibbles
                .next()
                .ok_or_else(err)?
                .to_digit(16)
                .ok_or_else(err)?;
            let lo = nibbles
                .next()
                .ok_or_else(err)?
                .to_digit(16)
                .ok_or_else(err)?;
            // Both digits are below 16, so the combination fits in a byte.
            *byte = ((hi << 4) | lo) as u8;
        }
        if nibbles.next().is_some() {
            return Err(err());
        }

        Ok(Self::from_bytes_be(bytes_be))
    }
}

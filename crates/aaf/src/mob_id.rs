//! The 32-byte identifier AAF uses for mobs.

use std::fmt;

use crate::Auid;

/// A MobID: the identifier that names a mob across files.
///
/// Where an [`Auid`] identifies a *kind* of thing — a class, a type, a
/// property — a MobID identifies one particular piece of material, and stays
/// with it as it moves between applications and files. It is a SMPTE ST 330
/// basic UMID: a 12-byte universal label, a length byte, a three-byte instance
/// number, and a 16-byte material number that is itself an AUID.
///
/// # Example
///
/// ```
/// use aaf::MobId;
///
/// let urn = "urn:smpte:umid:060a2b34.01010101.01010f00.13000000.\
///            060e2b34.7f7f2a80.4fa5c20f.4e301e50";
/// let id: MobId = urn.parse().unwrap();
/// assert_eq!(id.length(), 0x13);
/// assert_eq!(id.to_string(), urn);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MobId {
    bytes: [u8; 32],
}

impl MobId {
    /// The all-zero MobID, which AAF uses to mean "no mob".
    pub const NIL: Self = Self { bytes: [0; 32] };

    /// Builds a MobID from its 32 bytes as they appear on disk.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// The 32 bytes as they appear on disk.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.bytes
    }

    /// Whether this is the all-zero MobID.
    #[must_use]
    pub fn is_nil(self) -> bool {
        self.bytes == [0; 32]
    }

    /// The 12-byte SMPTE universal label.
    #[must_use]
    pub fn smpte_label(self) -> [u8; 12] {
        self.bytes[..12].try_into().expect("slice is twelve bytes")
    }

    /// The length byte: `0x13` for a basic UMID.
    #[must_use]
    pub const fn length(self) -> u8 {
        self.bytes[12]
    }

    /// The three-byte instance number, most significant byte first.
    #[must_use]
    pub const fn instance(self) -> [u8; 3] {
        [self.bytes[13], self.bytes[14], self.bytes[15]]
    }

    /// The 16-byte material number, which is an AUID.
    #[must_use]
    pub fn material(self) -> Auid {
        Auid::from_bytes_le(self.bytes[16..].try_into().expect("slice is sixteen bytes"))
    }
}

impl fmt::Debug for MobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MobId({self})")
    }
}

impl fmt::Display for MobId {
    /// Renders the URN form, `urn:smpte:umid:` followed by eight dotted groups.
    ///
    /// Some applications write the material number with its two halves
    /// swapped. Those are recognised by their universal label and rendered the
    /// way they were written, so that a MobID printed here matches the one the
    /// application that made it would print. Upstream `pyaaf2` does the same.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.bytes;
        let material = self.material();
        let data4 = material.data4();

        f.write_str("urn:smpte:umid:")?;
        for group in b[..12].chunks_exact(4) {
            for byte in group {
                write!(f, "{byte:02x}")?;
            }
            f.write_str(".")?;
        }
        write!(f, "{:02x}{:02x}{:02x}{:02x}.", b[12], b[13], b[14], b[15])?;

        let half_swapped = b[11] == 0x00 && data4[..6] == [0x06, 0x0e, 0x2b, 0x34, 0x7f, 0x7f];
        let (first, second) = if half_swapped {
            // The two halves of the material number are the other way round.
            (
                format!(
                    "{:02x}{:02x}{:02x}{:02x}.{:02x}{:02x}{:02x}{:02x}",
                    data4[0], data4[1], data4[2], data4[3], data4[4], data4[5], data4[6], data4[7]
                ),
                format!(
                    "{:08x}.{:04x}{:04x}",
                    material.data1(),
                    material.data2(),
                    material.data3()
                ),
            )
        } else {
            (
                format!(
                    "{:08x}.{:04x}{:04x}",
                    material.data1(),
                    material.data2(),
                    material.data3()
                ),
                format!(
                    "{:02x}{:02x}{:02x}{:02x}.{:02x}{:02x}{:02x}{:02x}",
                    data4[0], data4[1], data4[2], data4[3], data4[4], data4[5], data4[6], data4[7]
                ),
            )
        };
        write!(f, "{first}.{second}")
    }
}

/// The error returned when a string is not a well-formed MobID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseMobIdError {
    /// The rejected string.
    pub input: String,
}

impl fmt::Display for ParseMobIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "badly formed MobID string: '{}'", self.input)
    }
}

impl std::error::Error for ParseMobIdError {}

impl std::str::FromStr for MobId {
    type Err = ParseMobIdError;

    /// Parses the URN form, with the `urn:smpte:umid:` prefix and the dots
    /// between groups both optional.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseMobIdError {
            input: s.to_owned(),
        };

        let hex: String = s
            .trim()
            .to_ascii_lowercase()
            .replace("urn:smpte:umid:", "")
            .replace("0x", "")
            .chars()
            .filter(|c| !matches!(c, '.' | '-'))
            .collect();
        if hex.len() != 64 {
            return Err(err());
        }

        // The first 16 bytes are stored as written; the material number that
        // follows is written big-endian and stored little-endian.
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| err())?;
        }
        let material: [u8; 16] = bytes[16..].try_into().expect("slice is sixteen bytes");
        bytes[16..].copy_from_slice(&Auid::from_bytes_be(material).to_bytes_le());

        Ok(Self { bytes })
    }
}

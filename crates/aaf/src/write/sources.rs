//! Where a new file's times and identifiers come from.
//!
//! pyaaf2 reads the clock and asks for a random UUID at a handful of points
//! while it builds a file: the time the file was made, a random
//! `GenerationAUID` for the file, and for every new mob a random `MobID` and
//! the time the mob was made. Everything else it writes follows from what it
//! was given. So the same content written twice differs in those values and
//! nowhere else, and a writer that takes them from a source it is handed can
//! write a file that is identical, byte for byte, to one pyaaf2 wrote.
//!
//! [`Clock`] and [`IdSource`] are those two sources. A writer made with
//! [`AafWriter::new`](super::AafWriter::new) uses [`SystemClock`] and
//! [`RandomIds`], which behave as pyaaf2's do. [`SteppingClock`] and
//! [`SequentialIds`] hand out a fixed sequence instead, for tests and for
//! output that must not change from one run to the next.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use super::value::Timestamp;
use crate::Auid;

/// Tells a writer the time.
///
/// Asked once for the file itself, which records it as when the file was
/// made and last changed, and once for each new mob.
pub trait Clock {
    /// The time now.
    fn now(&mut self) -> Timestamp;
}

/// Hands a writer fresh identifiers.
///
/// Asked once for the file's `GenerationAUID`, and once for each new mob,
/// whose `MobID` carries the identifier as its material number.
pub trait IdSource {
    /// A new identifier, as random as the source can make it. pyaaf2 uses a
    /// version 4 UUID.
    fn uuid4(&mut self) -> Auid;
}

/// The system clock, read as UTC.
///
/// pyaaf2 records local time. The standard library has no time zones, so
/// this records UTC; a caller that wants local time can supply a [`Clock`]
/// of its own.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&mut self) -> Timestamp {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        Timestamp::from_unix(seconds)
    }
}

/// Random version 4 UUIDs.
///
/// Seeded from the standard library's per-process hash keys and the time,
/// which is as much randomness as the standard library offers. The values
/// are unique rather than secret: nothing in AAF depends on them being
/// unpredictable.
#[derive(Debug, Clone)]
pub struct RandomIds {
    state: u64,
}

impl RandomIds {
    /// A new, freshly seeded source.
    #[must_use]
    pub fn new() -> Self {
        let mut hasher = RandomState::new().build_hasher();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        hasher.write_u128(nanos);
        hasher.write_usize(std::process::id() as usize);
        Self {
            state: hasher.finish(),
        }
    }

    /// splitmix64: a small generator with no bad seeds.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

impl Default for RandomIds {
    fn default() -> Self {
        Self::new()
    }
}

impl IdSource for RandomIds {
    fn uuid4(&mut self) -> Auid {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&self.next_u64().to_be_bytes());
        bytes[8..].copy_from_slice(&self.next_u64().to_be_bytes());
        bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
        bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
        Auid::from_bytes_be(bytes)
    }
}

/// A clock that starts at a fixed time and moves on one second each time it
/// is read.
///
/// # Example
///
/// ```
/// use aaf::write::{Clock, SteppingClock, Timestamp};
///
/// let mut clock = SteppingClock::new(Timestamp::parse_iso("2024-05-06T07:08:09").unwrap());
/// assert_eq!(clock.now().to_string(), "2024-05-06T07:08:09");
/// assert_eq!(clock.now().to_string(), "2024-05-06T07:08:10");
/// ```
#[derive(Debug, Clone)]
pub struct SteppingClock {
    next: i64,
}

impl SteppingClock {
    /// A clock whose first reading is `start`.
    #[must_use]
    pub fn new(start: Timestamp) -> Self {
        Self {
            next: start.to_unix(),
        }
    }
}

impl Clock for SteppingClock {
    fn now(&mut self) -> Timestamp {
        let now = Timestamp::from_unix(self.next);
        self.next += 1;
        now
    }
}

/// Identifiers that count up from one, in a fixed pattern.
///
/// The `n`th identifier is `{prefix + n:08x}-0000-4000-8000-{n:012x}`: a
/// valid version 4 UUID in form, and plainly not a random one.
///
/// # Example
///
/// ```
/// use aaf::write::{IdSource, SequentialIds};
///
/// let mut ids = SequentialIds::new(0x5eed_0000);
/// assert_eq!(ids.uuid4().to_string(), "5eed0001-0000-4000-8000-000000000001");
/// assert_eq!(ids.uuid4().to_string(), "5eed0002-0000-4000-8000-000000000002");
/// ```
#[derive(Debug, Clone)]
pub struct SequentialIds {
    prefix: u32,
    count: u32,
}

impl SequentialIds {
    /// A source whose identifiers start with `prefix + n`.
    #[must_use]
    pub const fn new(prefix: u32) -> Self {
        Self { prefix, count: 0 }
    }
}

impl IdSource for SequentialIds {
    fn uuid4(&mut self) -> Auid {
        self.count += 1;
        let n = self.count;
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&self.prefix.wrapping_add(n).to_be_bytes());
        bytes[6] = 0x40;
        bytes[8] = 0x80;
        bytes[12..].copy_from_slice(&n.to_be_bytes());
        Auid::from_bytes_be(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_ids_are_version_4_and_differ() {
        let mut ids = RandomIds::new();
        let a = ids.uuid4().to_bytes_be();
        let b = ids.uuid4().to_bytes_be();
        assert_ne!(a, b);
        assert_eq!(a[6] >> 4, 4);
        assert_eq!(a[8] >> 6, 2);
    }
}

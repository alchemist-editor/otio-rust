//! Time math for OpenTimelineIO.
//!
//! This is a Rust port of upstream OpenTimelineIO's `opentime` C++ library. It
//! provides three value types and the conversions between them and the string
//! forms the industry uses:
//!
//! - [`RationalTime`] — a value at a rate, such as frame 24 at 24fps.
//! - [`TimeRange`] — a start time and a duration, with the full set of Allen
//!   interval relations.
//! - [`TimeTransform`] — an offset, a scale and a rate.
//!
//! All three are `Copy` plain data. There is no allocation, no interior
//! mutability and no shared ownership anywhere in this crate.
//!
//! # Compatibility
//!
//! The arithmetic, rounding and timecode behaviour here is intended to match
//! upstream exactly, so that a file written by either library reads the same in
//! the other. Where this port knowingly differs, the difference is documented
//! on the item and is always in the direction of accepting more input, never
//! of producing different output for input upstream accepts. There are two:
//! the SMPTE rate table drops upstream's trailing zero entry, and
//! [`RationalTime::from_time_string`] accepts the leading `-` that upstream
//! documents but rejects.
//!
//! # Example
//!
//! ```
//! use opentime::{RationalTime, TimeRange};
//!
//! let start = RationalTime::from_timecode("01:00:00:00", 24.0)?;
//! let duration = RationalTime::new(48.0, 24.0);
//! let shot = TimeRange::new(start, duration);
//!
//! assert_eq!(shot.end_time_exclusive().to_timecode()?, "01:00:02:00");
//! assert_eq!(shot.duration().to_seconds(), 2.0);
//! # Ok::<(), opentime::TimeError>(())
//! ```

mod cfmt;
mod error;
mod rational_time;
mod time_range;
mod time_transform;

pub use error::{Result, TimeError};
pub use rational_time::{DropFrame, RationalTime, max, min};
pub use time_range::{DEFAULT_EPSILON_S, TimeRange};
pub use time_transform::TimeTransform;

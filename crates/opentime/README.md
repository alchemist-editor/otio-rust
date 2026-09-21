# opentime

Time math for OpenTimelineIO: rational time, time ranges, and SMPTE timecode.

A Rust port of upstream OpenTimelineIO's `opentime` C++ library, matching its
arithmetic and timecode behaviour so that values round-trip between the two.

```rust
use opentime::{RationalTime, TimeRange};

let start = RationalTime::from_timecode("01:00:00:00", 24.0).unwrap();
let shot = TimeRange::new(start, RationalTime::new(48.0, 24.0));

assert_eq!(shot.end_time_exclusive().to_timecode().unwrap(), "01:00:02:00");
assert_eq!(shot.duration().to_seconds(), 2.0);
```

## What it covers

- `RationalTime` — a value at a rate, with rescaling, rounding, and conversion
  to and from SMPTE timecode (including drop-frame) and `HH:MM:SS.sss` strings.
- `TimeRange` — a start and a duration, with the full set of Allen interval
  relations: `contains`, `overlaps`, `before`, `meets`, `begins`, `finishes`,
  `intersects`.
- `TimeTransform` — an offset, a scale and a rate.

All three are `Copy` plain data. The crate allocates only when formatting a
string, uses no interior mutability, and forbids `unsafe`.

## License

Apache-2.0.

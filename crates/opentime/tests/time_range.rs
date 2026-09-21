//! Conformance tests for `TimeRange`.
//!
//! Ported from upstream OpenTimelineIO's `tests/test_opentime.py` at version
//! 0.19.0.

use opentime::{DEFAULT_EPSILON_S, RationalTime, TimeRange};

fn rt(value: f64, rate: f64) -> RationalTime {
    RationalTime::new(value, rate)
}

fn tr(start: f64, duration: f64, rate: f64) -> TimeRange {
    TimeRange::from_values(start, duration, rate)
}

const EPS: f64 = DEFAULT_EPSILON_S;

#[test]
fn create() {
    let range = TimeRange::default();
    assert_eq!(range.start_time(), RationalTime::default());
    assert_eq!(range.duration(), RationalTime::default());

    // A range built from a start time alone takes that time's rate for its
    // zero duration.
    let range = TimeRange::at(rt(10.0, 48.0));
    assert_eq!(range.start_time().rate(), range.duration().rate());

    let range = tr(0.0, 48.0, 24.0);
    assert_eq!(range.start_time(), rt(0.0, 24.0));
    assert_eq!(range.duration(), rt(48.0, 24.0));
}

#[test]
fn valid() {
    let range = tr(0.0, 0.0, 0.0);
    assert!(range.is_invalid_range());
    assert!(!range.is_valid_range());

    let range = tr(0.0, 48.0, 24.0);
    assert!(range.is_valid_range());
    assert!(!range.is_invalid_range());

    // A negative duration is not a valid range.
    let range = tr(0.0, -48.0, 24.0);
    assert!(!range.is_valid_range());
    assert!(range.is_invalid_range());
}

#[test]
fn end_time_with_whole_number_duration() {
    let start = rt(1.0, 24.0);
    let duration = rt(5.0, 24.0);
    let range = TimeRange::new(start, duration);

    assert_eq!(range.duration(), duration);
    assert_eq!(range.end_time_exclusive(), start + duration);
    assert_eq!(range.end_time_inclusive(), start + duration - rt(1.0, 24.0));
}

#[test]
fn end_time_with_fractional_duration() {
    let start = rt(1.0, 24.0);
    let duration = rt(5.5, 24.0);
    let range = TimeRange::new(start, duration);

    assert_eq!(range.end_time_exclusive(), start + duration);
    // A fractional duration ends partway through a frame, so the last whole
    // instant is the floor of the exclusive end.
    assert_eq!(range.end_time_inclusive(), rt(6.0, 24.0));
}

#[test]
fn compare() {
    let tr1 = TimeRange::new(rt(18.0, 24.0), rt(7.0, 24.0));
    // Same span, expressed at a different rate.
    let tr2 = TimeRange::new(rt(18.0, 24.0), rt(14.0, 48.0));
    assert_eq!(tr1, tr2);

    let tr3 = TimeRange::new(rt(20.0, 24.0), rt(3.0, 24.0));
    assert_ne!(tr1, tr3);
}

#[test]
fn clamped() {
    let range = TimeRange::new(rt(-1.0, 24.0), rt(6.0, 24.0));
    let other = TimeRange::new(rt(-2.0, 24.0), rt(7.0, 24.0));

    assert_eq!(range.clamped_time(rt(-2.0, 24.0)), range.start_time());
    assert_eq!(
        range.clamped_time(rt(6.0, 24.0)),
        range.end_time_inclusive()
    );
    assert_eq!(range.clamped_range(other), range);
}

#[test]
fn contains_time() {
    let start = rt(12.0, 25.0);
    let duration = rt(3.3, 25.0);
    let range = TimeRange::new(start, duration);

    assert!(range.contains_time(start));
    assert!(!range.contains_time(start + duration));
    assert!(!range.contains_time(start - duration));
}

#[test]
fn contains_range_is_strict() {
    let start = rt(12.0, 25.0);
    let duration = rt(3.3, 25.0);
    let range = TimeRange::new(start, duration);

    // A range does not strictly contain itself.
    assert!(!range.contains_range(range, EPS));

    let earlier = TimeRange::new(start - duration, duration);
    assert!(!range.contains_range(earlier, EPS));
    assert!(!earlier.contains_range(range, EPS));
}

#[test]
fn overlaps_time() {
    let range = TimeRange::new(rt(12.0, 25.0), rt(3.0, 25.0));
    assert!(range.overlaps_time(rt(13.0, 25.0)));
    assert!(!range.overlaps_time(rt(1.0, 25.0)));
}

#[test]
fn overlaps_range() {
    let range = TimeRange::new(rt(12.0, 25.0), rt(3.0, 25.0));

    // `overlaps` is directional: it is true only when this range starts
    // before the other and ends inside it.
    let cases: &[(f64, f64, f64, bool)] = &[
        (0.0, 3.0, 25.0, false),
        (10.0, 3.0, 25.0, false),
        (13.0, 1.0, 25.0, false),
        (2.0, 30.0, 25.0, false),
        (2.0, 60.0, 50.0, false),
        (2.0, 14.0, 50.0, false),
        (-100.0, 400.0, 50.0, false),
        (100.0, 400.0, 50.0, false),
    ];
    for &(start, duration, rate, expected) in cases {
        let other = tr(start, duration, rate);
        assert_eq!(
            range.overlaps_range(other, EPS),
            expected,
            "overlaps({start}, {duration}, {rate})"
        );
    }
}

#[test]
fn intersects_range() {
    let range = TimeRange::new(rt(12.0, 25.0), rt(3.0, 25.0));

    let cases: &[(f64, f64, f64, bool)] = &[
        (0.0, 3.0, 25.0, false),
        (10.0, 3.0, 25.0, true),
        (10.0, 2.0, 25.0, false),
        (14.0, 2.0, 25.0, true),
        (15.0, 2.0, 25.0, false),
        (13.0, 1.0, 25.0, true),
        (2.0, 30.0, 25.0, true),
        (2.0, 60.0, 50.0, true),
        (2.0, 14.0, 50.0, false),
        (-100.0, 400.0, 50.0, true),
        (100.0, 400.0, 50.0, false),
    ];
    for &(start, duration, rate, expected) in cases {
        let other = tr(start, duration, rate);
        assert_eq!(
            range.intersects(other, EPS),
            expected,
            "intersects({start}, {duration}, {rate})"
        );
    }
}

#[test]
fn before_range() {
    let range = TimeRange::new(rt(12.0, 25.0), rt(3.0, 25.0));

    let earlier = TimeRange::new(rt(10.0, 25.0), rt(1.5, 25.0));
    assert!(earlier.before_range(range, EPS));
    assert!(!range.before_range(earlier, EPS));

    // An overlapping range is not before.
    let overlapping = TimeRange::new(rt(10.0, 25.0), rt(12.0, 25.0));
    assert!(!overlapping.before_range(range, EPS));

    // Nothing is before itself.
    assert!(!range.before_range(range, EPS));
}

#[test]
fn before_time() {
    let start = rt(12.0, 25.0);
    let after = rt(15.0, 25.0);

    let range = TimeRange::new(start, rt(3.0, 25.0));
    assert!(!range.before_time(after, EPS));
    assert!(!range.before_time(start, EPS));

    // Ending just short of `after` puts the range before it.
    let shorter = TimeRange::new(start, rt(1.99, 25.0));
    assert!(shorter.before_time(after, EPS));
}

#[test]
fn meets() {
    let duration = rt(3.0, 25.0);
    let range = TimeRange::new(rt(12.0, 25.0), duration);
    let adjacent = TimeRange::new(rt(15.0, 25.0), duration);

    assert!(range.meets(adjacent, EPS));
    assert!(!adjacent.meets(range, EPS));

    // A zero-length range meets itself.
    let instant = TimeRange::new(rt(14.99, 25.0), rt(0.0, 25.0));
    assert!(instant.meets(instant, EPS));
}

#[test]
fn begins_range() {
    let start = rt(12.0, 25.0);
    let range = TimeRange::new(start, rt(3.0, 25.0));
    let longer = TimeRange::new(start, rt(5.0, 25.0));

    assert!(range.begins_range(longer, EPS));
    assert!(!longer.begins_range(range, EPS));
    // A range does not begin itself: the end must strictly precede.
    assert!(!range.begins_range(range, EPS));

    let instant = TimeRange::new(start, rt(0.0, 25.0));
    assert!(instant.begins_range(longer, EPS));
    assert!(!instant.begins_range(instant, EPS));

    // A different start time is not a shared beginning.
    let elsewhere = TimeRange::new(rt(30.0, 25.0), rt(0.0, 25.0));
    assert!(!instant.begins_range(elsewhere, EPS));

    let later = TimeRange::new(rt(13.0, 25.0), rt(0.0, 25.0));
    assert!(!later.begins_range(range, EPS));
}

#[test]
fn begins_time() {
    let start = rt(12.0, 25.0);
    let range = TimeRange::new(start, rt(3.0, 25.0));

    assert!(range.begins_time(start, EPS));
    assert!(!range.begins_time(rt(15.0, 25.0), EPS));
    assert!(!range.begins_time(rt(11.9, 25.0), EPS));
}

#[test]
fn finishes_range() {
    let range = TimeRange::new(rt(12.0, 25.0), rt(3.0, 25.0));
    let inner = TimeRange::new(rt(13.0, 25.0), rt(2.0, 25.0));

    assert!(inner.finishes_range(range, EPS));
    assert!(!range.finishes_range(inner, EPS));
    // A range does not finish itself: the start must strictly follow.
    assert!(!range.finishes_range(range, EPS));

    // Ends short of the outer range's end.
    let short = TimeRange::new(rt(13.0, 25.0), rt(1.0, 25.0));
    assert!(!short.finishes_range(range, EPS));

    // Starts after the outer range ends.
    let elsewhere = TimeRange::new(rt(30.0, 25.0), rt(1.0, 25.0));
    assert!(!elsewhere.finishes_range(range, EPS));

    // A zero-length range at the end finishes it.
    let instant = TimeRange::new(rt(15.0, 25.0), rt(0.0, 25.0));
    assert!(instant.finishes_range(range, EPS));
}

#[test]
fn finishes_time() {
    let start = rt(12.0, 25.0);
    let range = TimeRange::new(start, rt(3.0, 25.0));

    assert!(range.finishes_time(rt(15.0, 25.0), EPS));
    assert!(!range.finishes_time(start, EPS));
    assert!(!range.finishes_time(rt(16.0, 25.0), EPS));
}

#[test]
fn range_from_start_end_time() {
    let start = rt(0.0, 25.0);
    let end = rt(12.0, 25.0);
    let range = TimeRange::range_from_start_end_time(start, end);

    assert_eq!(range.start_time(), start);
    assert_eq!(range.duration(), end);
    assert_eq!(range.end_time_exclusive(), end);
    assert_eq!(range.end_time_inclusive(), end - rt(1.0, 25.0));

    assert_eq!(
        range,
        TimeRange::range_from_start_end_time(range.start_time(), range.end_time_exclusive())
    );
}

#[test]
fn range_from_start_end_time_inclusive() {
    let start = rt(0.0, 25.0);
    let end = rt(12.0, 25.0);
    let range = TimeRange::range_from_start_end_time_inclusive(start, end);

    assert_eq!(range.start_time(), start);
    assert_eq!(range.duration(), rt(13.0, 25.0));
    assert_eq!(range.end_time_inclusive(), end);

    assert_eq!(
        range,
        TimeRange::range_from_start_end_time_inclusive(
            range.start_time(),
            range.end_time_inclusive()
        )
    );
}

#[test]
fn extended_by_adjacent_ranges() {
    let d1 = 0.3;
    let d2 = 0.4;
    let r1 = TimeRange::new(rt(0.0, 1.0), rt(d1, 1.0));
    let r2 = TimeRange::new(r1.end_time_exclusive(), rt(d2, 1.0));
    let full = TimeRange::new(rt(0.0, 1.0), rt(d1 + d2, 1.0));

    assert!(!r1.overlaps_range(r2, EPS));
    assert_eq!(r1.extended_by(r2), full);
}

#[test]
fn extended_by_distant_ranges() {
    let start = 0.1;
    let d1 = 0.3;
    let gap = 1.7;
    let d2 = 0.4;

    let r1 = TimeRange::new(rt(start, 1.0), rt(d1, 1.0));
    let r2 = TimeRange::new(rt(start + gap + d1, 1.0), rt(d2, 1.0));
    let full = TimeRange::new(rt(start, 1.0), rt(d1 + gap + d2, 1.0));

    assert!(!r1.overlaps_range(r2, EPS));
    assert_eq!(r1.extended_by(r2), full);
    assert_eq!(r2.extended_by(r1), full);
}

#[test]
fn duration_extended_by() {
    let range = TimeRange::new(rt(12.0, 25.0), rt(3.0, 25.0));
    let extended = range.duration_extended_by(rt(2.0, 25.0));
    assert_eq!(extended.start_time(), range.start_time());
    assert_eq!(extended.duration(), rt(5.0, 25.0));
}

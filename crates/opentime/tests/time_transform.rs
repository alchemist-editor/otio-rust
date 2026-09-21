//! Conformance tests for `TimeTransform`.
//!
//! Ported from upstream OpenTimelineIO's `tests/test_opentime.py` at version
//! 0.19.0.

use opentime::{RationalTime, TimeRange, TimeTransform};

fn rt(value: f64, rate: f64) -> RationalTime {
    RationalTime::new(value, rate)
}

/// A transform with only an offset, leaving scale and rate alone.
fn offset_by(offset: RationalTime) -> TimeTransform {
    TimeTransform::new(offset, 1.0, -1.0)
}

/// A transform with only a scale.
fn scale_by(scale: f64) -> TimeTransform {
    TimeTransform::new(RationalTime::new(0.0, 1.0), scale, -1.0)
}

#[test]
fn identity_transform() {
    let start = rt(12.0, 25.0);
    assert_eq!(TimeTransform::default().applied_to_time(start), start);

    // A transform carrying only a rate rescales.
    let to_50 = TimeTransform::new(RationalTime::new(0.0, 1.0), 1.0, 50.0);
    assert_eq!(to_50.applied_to_time(start).value(), 24.0);
}

#[test]
fn offset() {
    let start = rt(12.0, 25.0);
    let offset = rt(10.0, 25.0);
    let transform = offset_by(offset);

    assert_eq!(transform.applied_to_time(start), start + offset);

    let range = TimeRange::new(start, start);
    assert_eq!(
        transform.applied_to_range(range),
        TimeRange::new(start + offset, start)
    );
}

#[test]
fn scale() {
    let start = rt(12.0, 25.0);
    let transform = scale_by(2.0);

    assert_eq!(transform.applied_to_time(start), rt(24.0, 25.0));

    let range = TimeRange::new(start, start);
    let scaled = rt(24.0, 25.0);
    assert_eq!(
        transform.applied_to_range(range),
        TimeRange::new(scaled, scaled)
    );
}

#[test]
fn rate_falls_through_when_unset() {
    let identity = TimeTransform::default();
    let to_50 = TimeTransform::new(RationalTime::new(0.0, 1.0), 1.0, 50.0);
    // The identity transform has no rate of its own, so composing keeps the
    // other transform's rate.
    assert_eq!(identity.applied_to_transform(to_50).rate(), to_50.rate());
}

#[test]
fn composition_adds_offsets_and_multiplies_scales() {
    let a = TimeTransform::new(rt(10.0, 25.0), 2.0, -1.0);
    let b = TimeTransform::new(rt(5.0, 25.0), 3.0, 30.0);

    let composed = a.applied_to_transform(b);
    assert_eq!(composed.offset(), rt(15.0, 25.0));
    assert_eq!(composed.scale(), 6.0);
    // `a` has no rate, so `b`'s wins.
    assert_eq!(composed.rate(), 30.0);

    // A transform with a rate keeps its own.
    let c = TimeTransform::new(rt(0.0, 25.0), 1.0, 48.0);
    assert_eq!(c.applied_to_transform(b).rate(), 48.0);
}

#[test]
fn comparison() {
    let start = rt(12.0, 25.0);
    let a = TimeTransform::new(start, 2.0, -1.0);
    let b = TimeTransform::new(rt(12.0, 25.0), 2.0, -1.0);
    assert_eq!(a, b);

    assert_ne!(a, TimeTransform::new(start, 3.0, -1.0));
    assert_ne!(a, TimeTransform::new(start, 2.0, 25.0));
}

#[test]
fn display_matches_upstream_str() {
    let transform = TimeTransform::new(rt(12.0, 25.0), 2.0, -1.0);
    assert_eq!(
        transform.to_string(),
        "TimeTransform(RationalTime(12, 25), 2, -1)"
    );
}

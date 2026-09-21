//! Conformance tests for `RationalTime`.
//!
//! Ported from upstream OpenTimelineIO's `tests/test_opentime.py` at version
//! 0.19.0. Tests that only exercise Python-language behaviour (copy semantics,
//! attribute immutability, `TypeError` on bad argument types, `repr`
//! formatting) are not ported, since they test the binding rather than the
//! time math.

use opentime::{DropFrame, RationalTime, TimeError};

/// 23.976, as the exact NTSC rate.
const NTSC_23976: f64 = 24000.0 / 1001.0;
/// 29.97, as the exact NTSC rate.
const NTSC_2997: f64 = 30000.0 / 1001.0;
/// 59.94, as the exact NTSC rate.
const NTSC_5994: f64 = 60000.0 / 1001.0;

fn rt(value: f64, rate: f64) -> RationalTime {
    RationalTime::new(value, rate)
}

#[test]
fn create() {
    assert_eq!(rt(30.2, 1.0).value(), 30.2);
    assert_eq!(rt(-30.2, 1.0).value(), -30.2);

    let default = RationalTime::default();
    assert_eq!(default.value(), 0.0);
    assert_eq!(default.rate(), 1.0);
}

#[test]
fn valid() {
    let invalid = rt(0.0, 0.0);
    assert!(invalid.is_invalid_time());
    assert!(!invalid.is_valid_time());

    let valid = rt(24.0, 1.0);
    assert!(valid.is_valid_time());
    assert!(!valid.is_invalid_time());
}

#[test]
fn equality_rescales_before_comparing() {
    let t1 = rt(30.2, 1.0);
    assert_eq!(t1, t1);
    assert_eq!(t1, rt(30.2, 1.0));
    // Same instant, different rate.
    assert_eq!(t1, rt(60.4, 2.0));
}

#[test]
fn inequality() {
    let t1 = rt(30.2, 1.0);
    assert_ne!(t1, rt(33.2, 1.0));
    assert!(t1 == rt(30.2, 1.0));
}

#[test]
fn strict_equality_does_not_rescale() {
    let t1 = rt(30.2, 1.0);
    assert!(t1.strictly_equal(t1));
    assert!(t1.strictly_equal(rt(30.2, 1.0)));
    // Equal as instants, but not strictly equal.
    assert!(!t1.strictly_equal(rt(60.4, 2.0)));
    assert_eq!(t1, rt(60.4, 2.0));
}

#[test]
fn rounding() {
    let t1 = rt(30.2, 1.0);
    assert_eq!(t1.floor(), rt(30.0, 1.0));
    assert_eq!(t1.ceil(), rt(31.0, 1.0));
    assert_eq!(t1.round(), rt(30.0, 1.0));

    let t2 = rt(30.8, 1.0);
    assert_eq!(t2.floor(), rt(30.0, 1.0));
    assert_eq!(t2.ceil(), rt(31.0, 1.0));
    assert_eq!(t2.round(), rt(31.0, 1.0));
}

#[test]
// The negated forms are the point: upstream defines `<` as `!(>=)` and `<=`
// as `!(>)`, and these assertions pin that down.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
fn comparison() {
    let t1 = rt(15.2, 1.0);
    let t2 = rt(15.6, 1.0);
    assert!(t1 < t2);
    assert!(t1 <= t2);
    assert!(!(t1 > t2));
    assert!(!(t1 >= t2));

    // The equality case of each comparison.
    let t3 = rt(30.4, 2.0);
    assert!(t1 <= t3);
    assert!(t1 >= t3);
    assert!(t3 <= t1);
    assert!(t3 >= t1);

    // Comparison converts between rates.
    let t4 = rt(15.6, 48.0);
    assert!(t1 > t4);
    assert!(t1 >= t4);
    assert!(!(t1 < t4));
    assert!(!(t1 <= t4));
}

#[test]
fn base_conversion() {
    let t = rt(10.0, 24.0);
    assert_eq!(t.rate(), 24.0);
    assert_eq!(t.rescaled_to(48.0).rate(), 48.0);

    let other = rt(20.0, 48.0);
    assert_eq!(t.rescaled_to_time(other).rate(), other.rate());
}

#[test]
fn timecode_round_trips() {
    let timecode = "00:06:56:17";
    let t = RationalTime::from_timecode(timecode, 24.0).unwrap();
    assert_eq!(t.to_timecode().unwrap(), timecode);
}

#[test]
fn negative_timecode_is_rejected() {
    // Upstream slices fixed-width fields, so the leading sign shifts every
    // field and the second one fails to parse.
    assert!(matches!(
        RationalTime::from_timecode("-01:00:13:13", 24.0),
        Err(TimeError::InvalidTimecodeString { .. })
    ));
}

#[test]
fn bogus_timecode_is_rejected() {
    // Rate 13 is not a SMPTE rate, which is caught before the string is read.
    assert!(matches!(
        RationalTime::from_timecode("pink elephants", 13.0),
        Err(TimeError::InvalidTimecodeRate { .. })
    ));
    assert!(matches!(
        RationalTime::from_timecode("pink elephants", 24.0),
        Err(TimeError::InvalidTimecodeString { .. })
    ));
}

#[test]
fn timecode_frame_beyond_rate_is_rejected() {
    let error = RationalTime::from_timecode("01:00:13:24", 24.0).unwrap_err();
    assert_eq!(
        error,
        TimeError::TimecodeRateMismatch {
            timecode: "01:00:13:24".to_string(),
            max_frame: 23,
        }
    );
}

#[test]
fn timecode_24() {
    let cases = [
        ("00:00:01:00", 24.0),
        ("00:01:00:00", 24.0 * 60.0),
        ("01:00:00:00", 24.0 * 60.0 * 60.0),
        ("24:00:00:00", 24.0 * 60.0 * 60.0 * 24.0),
        ("23:59:59:23", 24.0 * 60.0 * 60.0 * 24.0 - 1.0),
    ];
    for (timecode, value) in cases {
        assert_eq!(
            RationalTime::from_timecode(timecode, 24.0).unwrap(),
            rt(value, 24.0),
            "parsing {timecode}"
        );
    }
}

#[test]
fn timecode_23976_behaves_like_24() {
    let cases = [
        ("00:00:01:00", 24.0),
        ("00:01:00:00", 24.0 * 60.0),
        ("01:00:00:00", 24.0 * 60.0 * 60.0),
        ("24:00:00:00", 24.0 * 60.0 * 60.0 * 24.0),
        ("23:59:59:23", 24.0 * 60.0 * 60.0 * 24.0 - 1.0),
    ];
    for (timecode, value) in cases {
        assert_eq!(
            RationalTime::from_timecode(timecode, NTSC_23976).unwrap(),
            rt(value, NTSC_23976),
            "parsing {timecode}"
        );
    }
}

#[test]
// The explicit `sum2 = sum2 + increment` is what this test compares against.
#[allow(clippy::assign_op_pattern)]
fn add_assign_matches_add() {
    let mut sum1 = RationalTime::default();
    let mut sum2 = RationalTime::default();
    for i in 0..10 {
        let increment = rt(f64::from(i) + 1.0, 24.0);
        sum1 += increment;
        sum2 = sum2 + increment;
    }
    assert_eq!(sum1, sum2);
}

#[test]
fn timecode_zero() {
    let t = RationalTime::default();
    assert_eq!(
        t.to_timecode_at(24.0, DropFrame::InferFromRate).unwrap(),
        "00:00:00:00"
    );
    assert_eq!(RationalTime::from_timecode("00:00:00:00", 24.0).unwrap(), t);
}

#[test]
fn long_running_timecode_24_round_trips() {
    let final_frame_number = 24 * 60 * 60 * 24 - 1;
    let final_time = RationalTime::from_frames(f64::from(final_frame_number), 24.0);
    assert_eq!(final_time.to_timecode().unwrap(), "23:59:59:23");

    // Accumulating one frame at a time must land on the same instant.
    let step = rt(1.0, 24.0);
    let mut cumulative = RationalTime::default();
    for _ in 0..final_frame_number {
        cumulative += step;
    }
    assert_eq!(cumulative, final_time);

    // Every 1113th frame, which is not a multiple of the rate, round-trips.
    for frame in (1113..final_frame_number).step_by(1113) {
        let t = RationalTime::from_frames(f64::from(frame), 24.0);
        let timecode = t.to_timecode().unwrap();
        let parsed = RationalTime::from_timecode(&timecode, 24.0).unwrap();
        assert_eq!(t, parsed, "frame {frame}");
        assert_eq!(timecode, parsed.to_timecode().unwrap(), "frame {frame}");
    }
}

#[test]
fn negative_values_have_no_timecode() {
    let t = rt(-1.0, 25.0);
    assert_eq!(
        t.to_timecode_at(25.0, DropFrame::InferFromRate),
        Err(TimeError::NegativeValue)
    );
}

#[test]
fn drop_frame_timecode_2997_across_minute_rollovers() {
    assert_eq!(RationalTime::nearest_smpte_timecode_rate(29.97), NTSC_2997);

    // (frame number, expected drop-frame timecode). Drawn from upstream's
    // table, which walks each boundary where the drop-frame compensation
    // changes: every minute except every tenth.
    let cases: &[(i32, &str)] = &[
        // First four frames.
        (0, "00:00:00;00"),
        (1, "00:00:00;01"),
        (2, "00:00:00;02"),
        (3, "00:00:00;03"),
        // First minute rollover.
        (30 * 59 + 29, "00:00:59;29"),
        (30 * 59 + 30, "00:01:00;02"),
        (30 * 59 + 31, "00:01:00;03"),
        (30 * 59 + 32, "00:01:00;04"),
        (30 * 59 + 33, "00:01:00;05"),
        // Fifth minute.
        (30 * 299 + 29 - 2 * 4, "00:04:59;29"),
        (30 * 299 + 30 - 2 * 4, "00:05:00;02"),
        (30 * 299 + 31 - 2 * 4, "00:05:00;03"),
        (30 * 299 + 32 - 2 * 4, "00:05:00;04"),
        (30 * 299 + 33 - 2 * 4, "00:05:00;05"),
        // Seventh minute.
        (30 * 419 + 29 - 2 * 6, "00:06:59;29"),
        (30 * 419 + 30 - 2 * 6, "00:07:00;02"),
        (30 * 419 + 31 - 2 * 6, "00:07:00;03"),
        (30 * 419 + 32 - 2 * 6, "00:07:00;04"),
        (30 * 419 + 33 - 2 * 6, "00:07:00;05"),
        // Tenth minute: no frames are dropped here.
        (30 * 599 + 29 - 2 * (10 - 10 / 10), "00:09:59;29"),
        (30 * 599 + 30 - 2 * (10 - 10 / 10), "00:10:00;00"),
        (30 * 599 + 31 - 2 * (10 - 10 / 10), "00:10:00;01"),
        (30 * 599 + 32 - 2 * (10 - 10 / 10), "00:10:00;02"),
        (30 * 599 + 33 - 2 * (10 - 10 / 10), "00:10:00;03"),
        // Second hour.
        (30 * 7199 + 29 - 2 * (120 - 120 / 10), "01:59:59;29"),
        (30 * 7199 + 30 - 2 * (120 - 120 / 10), "02:00:00;00"),
        (30 * 7199 + 31 - 2 * (120 - 120 / 10), "02:00:00;01"),
        (30 * 7199 + 32 - 2 * (120 - 120 / 10), "02:00:00;02"),
        (30 * 7199 + 33 - 2 * (120 - 120 / 10), "02:00:00;03"),
        // Two and a half hours.
        (30 * 8999 + 29 - 2 * (150 - 150 / 10), "02:29:59;29"),
        (30 * 8999 + 30 - 2 * (150 - 150 / 10), "02:30:00;00"),
        (30 * 8999 + 31 - 2 * (150 - 150 / 10), "02:30:00;01"),
        (30 * 8999 + 32 - 2 * (150 - 150 / 10), "02:30:00;02"),
        (30 * 8999 + 33 - 2 * (150 - 150 / 10), "02:30:00;03"),
        // Tenth hour.
        (30 * 35999 + 29 - 2 * (600 - 600 / 10), "09:59:59;29"),
        (30 * 35999 + 30 - 2 * (600 - 600 / 10), "10:00:00;00"),
        (30 * 35999 + 31 - 2 * (600 - 600 / 10), "10:00:00;01"),
        (30 * 35999 + 32 - 2 * (600 - 600 / 10), "10:00:00;02"),
        (30 * 35999 + 33 - 2 * (600 - 600 / 10), "10:00:00;03"),
        // Third minute of the tenth hour.
        (30 * 36179 + 29 - 2 * (602 - 602 / 10), "10:02:59;29"),
        (30 * 36179 + 30 - 2 * (602 - 602 / 10), "10:03:00;02"),
        (30 * 36179 + 31 - 2 * (602 - 602 / 10), "10:03:00;03"),
        (30 * 36179 + 32 - 2 * (602 - 602 / 10), "10:03:00;04"),
        (30 * 36179 + 33 - 2 * (602 - 602 / 10), "10:03:00;05"),
    ];

    for &(value, expected) in cases {
        let t = rt(f64::from(value), NTSC_2997);
        assert_eq!(
            t.to_timecode_at(NTSC_2997, DropFrame::ForceYes).unwrap(),
            expected,
            "rendering frame {value}"
        );
        assert_eq!(
            RationalTime::from_timecode(expected, NTSC_2997).unwrap(),
            t,
            "parsing {expected}"
        );
    }
}

#[test]
fn timecode_ntsc_2997() {
    let frames = 1_084_319.0;
    let t = rt(frames, NTSC_2997);

    assert_eq!(
        t.to_timecode_at(NTSC_2997, DropFrame::ForceYes).unwrap(),
        "10:03:00;05"
    );
    assert_eq!(
        t.to_timecode_at(NTSC_2997, DropFrame::ForceNo).unwrap(),
        "10:02:23:29"
    );
    // Drop-frame is inferred from the rate when not specified.
    assert_eq!(
        t.to_timecode_at(NTSC_2997, DropFrame::InferFromRate)
            .unwrap(),
        "10:03:00;05"
    );

    // 24000/1001 has no drop-frame form.
    let invalid = rt(30.0, NTSC_23976);
    assert!(matches!(
        invalid.to_timecode_at(NTSC_23976, DropFrame::ForceYes),
        Err(TimeError::InvalidRateForDropFrameTimecode { .. })
    ));
}

#[test]
fn timecode_infers_drop_frame_from_rate() {
    let frames = 1_084_319.0;
    let cases = [
        (29.97, "10:03:00;05"),
        (NTSC_2997, "10:03:00;05"),
        (59.94, "05:01:30;03"),
        (NTSC_5994, "05:01:30;03"),
    ];
    for (rate, expected) in cases {
        let t = rt(frames, rate);
        assert_eq!(
            t.to_timecode_at(rate, DropFrame::InferFromRate).unwrap(),
            expected,
            "at rate {rate}"
        );
        assert_eq!(t.to_timecode().unwrap(), expected, "at rate {rate}");
    }
}

#[test]
fn timecode_2997_drop_and_non_drop_both_round_trip() {
    // (frame, non-drop timecode, drop-frame timecode)
    let cases = [
        (10789.0, "00:05:59:19", "00:05:59;29"),
        (10790.0, "00:05:59:20", "00:06:00;02"),
        (17981.0, "00:09:59:11", "00:09:59;29"),
        (17982.0, "00:09:59:12", "00:10:00;00"),
        (17983.0, "00:09:59:13", "00:10:00;01"),
        (17984.0, "00:09:59:14", "00:10:00;02"),
    ];

    for (value, non_drop, drop) in cases {
        let t = rt(value, NTSC_2997);
        let rendered_drop = t.to_timecode_at(NTSC_2997, DropFrame::ForceYes).unwrap();
        let rendered_non_drop = t.to_timecode_at(NTSC_2997, DropFrame::ForceNo).unwrap();
        let rendered_auto = t
            .to_timecode_at(NTSC_2997, DropFrame::InferFromRate)
            .unwrap();

        assert_eq!(rendered_drop, rendered_auto, "frame {value}");
        assert_eq!(rendered_drop, drop, "frame {value}");
        assert_eq!(rendered_non_drop, non_drop, "frame {value}");

        assert_eq!(
            RationalTime::from_timecode(&rendered_drop, NTSC_2997).unwrap(),
            t
        );
        assert_eq!(
            RationalTime::from_timecode(&rendered_non_drop, NTSC_2997).unwrap(),
            t
        );
    }
}

#[test]
fn non_drop_frame_timecode_at_a_drop_frame_rate() {
    let drop = "01:00:02;05";
    let non_drop = "00:59:58:17";
    let frames = 107_957.0;

    let t = rt(frames, NTSC_2997);
    assert_eq!(t.to_timecode().unwrap(), drop);
    assert_eq!(
        t.to_timecode_at(NTSC_2997, DropFrame::ForceNo).unwrap(),
        non_drop
    );

    // Both spellings name the same frame.
    assert_eq!(
        RationalTime::from_timecode(drop, NTSC_2997)
            .unwrap()
            .value(),
        frames
    );
    assert_eq!(
        RationalTime::from_timecode(non_drop, NTSC_2997)
            .unwrap()
            .value(),
        frames
    );
}

#[test]
fn drop_frame_timecode_at_a_non_drop_frame_rate_is_rejected() {
    assert!(matches!(
        RationalTime::from_timecode("01:00:13;23", 24.0),
        Err(TimeError::InvalidRateForDropFrameTimecode { .. })
    ));
}

#[test]
fn faulty_time_string_is_rejected() {
    assert!(matches!(
        RationalTime::from_time_string("bogus", 24.0),
        Err(TimeError::InvalidTimeString { .. })
    ));
}

#[test]
fn invalid_rate_for_timecode_is_rejected() {
    let t = rt(100.0, 999.0);
    assert!(matches!(
        t.to_timecode_at(777.0, DropFrame::InferFromRate),
        Err(TimeError::InvalidTimecodeRate { .. })
    ));
    assert!(matches!(
        t.to_timecode(),
        Err(TimeError::InvalidTimecodeRate { .. })
    ));
}

#[test]
fn time_string_24() {
    let cases = [
        ("00:00:00.041667", 1.0),
        ("00:00:01", 24.0),
        ("00:01:00", 24.0 * 60.0),
        ("01:00:00", 24.0 * 60.0 * 60.0),
        ("24:00:00", 24.0 * 60.0 * 60.0 * 24.0),
        ("23:59:59.958333", 24.0 * 60.0 * 60.0 * 24.0 - 1.0),
    ];
    for (time_string, value) in cases {
        let parsed = RationalTime::from_time_string(time_string, 24.0).unwrap();
        assert!(
            rt(value, 24.0).almost_equal(parsed, 0.001),
            "parsing {time_string}: got {parsed}"
        );
        assert_eq!(parsed.rate(), 24.0);
    }
}

#[test]
fn time_string_25() {
    let cases = [
        ("00:00:01", 25.0),
        ("00:01:00", 25.0 * 60.0),
        ("01:00:00", 25.0 * 60.0 * 60.0),
        ("24:00:00", 25.0 * 60.0 * 60.0 * 24.0),
        ("23:59:59.92", 25.0 * 60.0 * 60.0 * 24.0 - 2.0),
    ];
    for (time_string, value) in cases {
        let parsed = RationalTime::from_time_string(time_string, 25.0).unwrap();
        assert!(
            rt(value, 25.0).almost_equal(parsed, 0.001),
            "parsing {time_string}: got {parsed}"
        );
    }
}

#[test]
fn negative_time_renders_with_a_leading_sign() {
    // ffmpeg compatibility.
    assert_eq!(rt(-24.0, 24.0).to_time_string(), "-00:00:01.0");
}

#[test]
fn zero_time_string() {
    let t = RationalTime::default();
    assert_eq!(t.to_time_string(), "00:00:00.0");
    let parsed = RationalTime::from_time_string("00:00:00.0", 24.0).unwrap();
    assert!(t.almost_equal(parsed, 0.001));
}

#[test]
fn long_running_time_string_24() {
    let final_frame_number = 24 * 60 * 60 * 24 - 1;
    let final_time = RationalTime::from_frames(f64::from(final_frame_number), 24.0);
    assert_eq!(final_time.to_time_string(), "23:59:59.958333");

    let step = rt(1.0, 24.0);
    let mut cumulative = RationalTime::default();
    for _ in 0..final_frame_number {
        cumulative += step;
    }
    assert!(cumulative.almost_equal(final_time, 0.001));
}

#[test]
fn time_string_at_600_ticks() {
    // Rewritten by upstream from the 23.976 timecode table into seconds.
    let cases = [
        (1025.0, "00:00:01.708333"),
        (179_900.0, "00:04:59.833333"),
        (180_000.0, "00:05:00.0"),
        (360_000.0, "00:10:00.0"),
        (720_000.0, "00:20:00.0"),
        (1_079_300.0, "00:29:58.833333"),
        (1_080_000.0, "00:30:00.0"),
        (1_080_150.0, "00:30:00.25"),
        (1_440_000.0, "00:40:00.0"),
        (1_800_000.0, "00:50:00.0"),
        (1_978_750.0, "00:54:57.916666"),
        (1_980_000.0, "00:55:00.0"),
        (46700.0, "00:01:17.833333"),
        (225_950.0, "00:06:16.583333"),
        (436_400.0, "00:12:07.333333"),
        (703_350.0, "00:19:32.25"),
    ];
    for (value, expected) in cases {
        assert_eq!(rt(value, 600.0).to_time_string(), expected, "value {value}");
    }
}

#[test]
fn display_matches_upstream_str() {
    assert_eq!(rt(1.0, 2.0).to_string(), "RationalTime(1, 2)");
}

#[test]
fn from_frames_with_integer_rates() {
    for rate in [24.0, 30.0, 48.0, 60.0] {
        assert_eq!(RationalTime::from_frames(101.0, rate), rt(101.0, rate));
    }
}

#[test]
fn from_frames_with_non_integer_rates() {
    for rate in [23.98, 29.97, 59.94] {
        assert_eq!(RationalTime::from_frames(101.0, rate), rt(101.0, rate));
    }
}

#[test]
fn seconds() {
    let t1 = RationalTime::from_seconds(1834.0);
    assert_eq!(t1.value(), 1834.0);
    assert_eq!(t1.rate(), 1.0);
    assert_eq!(t1.to_seconds(), 1834.0);

    let t2 = RationalTime::from_seconds(248_474.345);
    assert!((t2.value() - 248_474.345).abs() < 1e-9);
    assert_eq!(t2.rate(), 1.0);
    assert!((t2.to_seconds() - 248_474.345).abs() < 1e-9);

    let seconds = 3459.0 / 24.0;
    assert!((rt(3459.0, 24.0).to_seconds() - seconds).abs() < 1e-9);
    assert!((RationalTime::from_seconds(seconds).to_seconds() - seconds).abs() < 1e-9);

    let t5 = RationalTime::from_seconds(seconds).rescaled_to(24.0);
    let t6 = RationalTime::from_seconds_at_rate(seconds, 24.0);
    assert_eq!(t5, t6);
    assert_eq!(t6.rate(), 24.0);
}

#[test]
fn duration_from_start_end_time() {
    let duration = RationalTime::duration_from_start_end_time(
        RationalTime::from_frames(100.0, 24.0),
        RationalTime::from_frames(200.0, 24.0),
    );
    assert_eq!(duration, RationalTime::from_frames(100.0, 24.0));

    // The result takes the rate of the start time.
    let duration = RationalTime::duration_from_start_end_time(
        RationalTime::from_frames(0.0, 1.0),
        RationalTime::from_frames(200.0, 24.0),
    );
    assert_eq!(duration, RationalTime::from_frames(200.0, 24.0));

    let end = rt(12.0, 25.0);
    assert_eq!(
        RationalTime::duration_from_start_end_time(rt(0.0, 25.0), end),
        end
    );
}

#[test]
fn duration_from_start_end_time_inclusive() {
    let duration = RationalTime::duration_from_start_end_time_inclusive(
        RationalTime::from_frames(100.0, 24.0),
        RationalTime::from_frames(200.0, 24.0),
    );
    assert_eq!(duration, RationalTime::from_frames(101.0, 24.0));

    let duration = RationalTime::duration_from_start_end_time_inclusive(
        RationalTime::from_frames(0.0, 30.0),
        RationalTime::from_frames(200.0, 24.0),
    );
    assert_eq!(duration, RationalTime::from_frames(251.0, 30.0));
}

#[test]
fn arithmetic() {
    let a = RationalTime::from_frames(100.0, 24.0);
    let gap = RationalTime::from_frames(50.0, 24.0);
    let b = RationalTime::from_frames(150.0, 24.0);

    assert_eq!(b - a, gap);
    assert_eq!(a + gap, b);
    assert_eq!(b - gap, a);

    let mut c = a;
    c += gap;
    assert_eq!(c, b);

    let mut accumulated = RationalTime::from_frames(100.0, 24.0);
    let step = RationalTime::from_frames(1.0, 24.0);
    for _ in 0..50 {
        accumulated += step;
    }
    assert_eq!(accumulated, RationalTime::from_frames(150.0, 24.0));
}

#[test]
fn arithmetic_resolves_to_the_higher_rate() {
    let a = RationalTime::from_frames(100.0, 24.0);
    let gap = RationalTime::from_frames(100.0, 48.0);
    let b = RationalTime::from_frames(75.0, 12.0);

    assert_eq!(b - a, gap.rescaled_to(24.0));
    assert_eq!(a + gap, b.rescaled_to(48.0));
    assert_eq!(b - gap, a.rescaled_to(48.0));

    let mut gap2 = gap;
    gap2 += a;
    assert_eq!(gap2, a + gap);
}

#[test]
fn subtract_with_different_rates() {
    assert_eq!((rt(12.0, 10.0) - rt(12.0, 5.0)).value(), -12.0);
}

#[test]
fn negation() {
    assert!((-rt(12.0, 24.0)).strictly_equal(rt(-12.0, 24.0)));
}

#[test]
fn nearest_smpte_timecode_rate() {
    let cases = [
        (23.976_023_976_023_97, NTSC_23976),
        (23.97, NTSC_23976),
        (23.976, NTSC_23976),
        (23.98, NTSC_23976),
        (29.97, NTSC_2997),
        (59.94, NTSC_5994),
        (24.0, 24.0),
        (23.999_999, 24.0),
        (29.999_999, 30.0),
        (30.01, 30.0),
        (60.01, 60.0),
    ];
    for (wonky_rate, smpte_rate) in cases {
        assert!(RationalTime::is_smpte_timecode_rate(smpte_rate));
        assert_eq!(
            RationalTime::nearest_smpte_timecode_rate(wonky_rate),
            smpte_rate,
            "snapping {wonky_rate}"
        );
    }
}

#[test]
fn to_timecode_at_mixed_rates() {
    let timecode = "00:06:56:17";
    let t = RationalTime::from_timecode(timecode, 24.0).unwrap();
    assert_eq!(t.to_timecode().unwrap(), timecode);
    assert_eq!(
        t.to_timecode_at(24.0, DropFrame::InferFromRate).unwrap(),
        timecode
    );
    assert_ne!(
        t.to_timecode_at(48.0, DropFrame::InferFromRate).unwrap(),
        timecode
    );

    // The same instant at different rates renders identically.
    assert_eq!(
        rt(24.0, 24.0)
            .to_timecode_at(24.0, DropFrame::InferFromRate)
            .unwrap(),
        rt(1.0, 1.0)
            .to_timecode_at(24.0, DropFrame::InferFromRate)
            .unwrap()
    );
}

#[test]
fn to_frames_at_mixed_rates() {
    let t = RationalTime::from_frames(100.0, 24.0);
    assert_eq!(t.to_frames(), 100);
    assert_eq!(t.to_frames_at_rate(24.0), 100);
    assert_ne!(t.to_frames_at_rate(12.0), 100);
}

#[test]
fn smpte_rate_table_excludes_upstreams_phantom_zero() {
    // Upstream's array is sized 11 with 10 initializers, leaving a trailing
    // 0.0 that makes zero report as a valid SMPTE rate and lets low rates
    // snap to zero. This port carries only the ten real rates.
    assert!(!RationalTime::is_smpte_timecode_rate(0.0));
    assert_eq!(RationalTime::nearest_smpte_timecode_rate(1.0), NTSC_23976);
}

#[test]
fn negative_time_string_round_trips() {
    // Upstream documents a leading '-' but its parser rejects one. This port
    // accepts it, so to_time_string output round-trips for negative times.
    let t = rt(-24.0, 24.0);
    let rendered = t.to_time_string();
    let parsed = RationalTime::from_time_string(&rendered, 24.0).unwrap();
    assert!(t.almost_equal(parsed, 0.001), "got {parsed}");
}

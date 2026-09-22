use opentime::RationalTime;

fn main() -> Result<(), opentime::TimeError> {
    // A time is a value and a rate, not a number of seconds. Four seconds at
    // 24 is 96 units; the rate travels with it so nothing has to guess later.
    let start = RationalTime::from_timecode("01:00:00:00", 24.0)?;
    let duration = RationalTime::from_frames(96.0, 24.0);

    let end = start + duration;
    println!("{} for {} seconds", end.to_timecode()?, duration.to_seconds());

    // Comparison rescales first, so the same instant at two rates is equal.
    assert_eq!(RationalTime::new(24.0, 24.0), RationalTime::new(48.0, 48.0));

    Ok(())
}

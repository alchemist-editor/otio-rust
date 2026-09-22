import OpenTimelineIO

// A time is a value and a rate, not a number of seconds. Four seconds at 24
// is 96 units; the rate travels with it so nothing has to guess later.
let start = try RationalTime.fromTimecode("01:00:00:00", rate: 24)
let duration = RationalTime.fromFrames(96, rate: 24)

let end = start.add(duration)
print(try end.toTimecode(), "for", duration.toSeconds, "seconds")

// Comparison rescales first, so the same instant at two rates is equal.
assert(RationalTime(value: 24, rate: 24).equals(RationalTime(value: 48, rate: 48)))

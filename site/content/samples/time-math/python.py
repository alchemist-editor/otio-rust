import opentimelineio as otio

# A time is a value and a rate, not a number of seconds. Four seconds at 24 is
# 96 units; the rate travels with it so nothing has to guess later.
start = otio.opentime.RationalTime.from_timecode("01:00:00:00", 24)
duration = otio.opentime.RationalTime.from_frames(96, 24)

end = start + duration
print(end.to_timecode(), "for", duration.to_seconds(), "seconds")

# Comparison rescales first, so the same instant at two rates is equal.
assert otio.opentime.RationalTime(24, 24) == otio.opentime.RationalTime(48, 48)

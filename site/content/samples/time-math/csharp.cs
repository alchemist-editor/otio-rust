using System;
using OpenTimelineIO;

// A time is a value and a rate, not a number of seconds. Four seconds at
// 24 is 96 units; the rate travels with it so nothing has to guess later.
var start = RationalTime.FromTimecode("01:00:00:00", 24);
var duration = RationalTime.FromFrames(96, 24);

var end = start.Add(duration);
Console.WriteLine($"{end.ToTimecode()} for {duration.ToSeconds()} seconds");

// Comparison rescales first, so the same instant at two rates is equal.
return new RationalTime(24, 24).Equals(new RationalTime(48, 48)) ? 0 : 1;

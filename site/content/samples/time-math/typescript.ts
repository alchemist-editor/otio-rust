import { init, RationalTime } from "@alchemist-edit/otio";

await init();

// A time is a value and a rate, not a number of seconds. Four seconds at 24
// is 96 units; the rate travels with it so nothing has to guess later.
const start = RationalTime.fromTimecode("01:00:00:00", 24);
const duration = RationalTime.fromFrames(96, 24);

const end = start.add(duration);
console.log(`${end.toTimecode()} for ${duration.toSeconds()} seconds`);

// Comparison rescales first, so the same instant at two rates is equal.
console.assert(new RationalTime(24, 24).equal(new RationalTime(48, 48)));

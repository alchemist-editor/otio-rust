#include <iostream>

#include <opentimelineio/otio.hpp>

int main() {
    // A time is a value and a rate, not a number of seconds. Four seconds at
    // 24 is 96 units; the rate travels with it so nothing has to guess later.
    const otio::RationalTime start = otio::RationalTime::from_timecode("01:00:00:00", 24);
    const otio::RationalTime duration = otio::RationalTime::from_frames(96, 24);

    const otio::RationalTime end = start.add(duration);
    std::cout << end.to_timecode() << " for " << duration.to_seconds() << " seconds\n";

    // Comparison rescales first, so the same instant at two rates is equal.
    return otio::RationalTime{24, 24}.equals(otio::RationalTime{48, 48}) ? 0 : 1;
}

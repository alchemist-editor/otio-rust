#include <iostream>

#include <opentimelineio/otio.hpp>

int main() {
    // An object is built on its own and put together with the others
    // afterwards. Nothing has to exist before the thing it goes into.
    otio::Timeline timeline = otio::Timeline::create("Cut");

    otio::Stack stack = otio::Stack::create("tracks");
    otio::Track track = otio::Track::create("V1", "Video");
    timeline.set_tracks(stack);
    stack.append_child(track);

    const std::vector<std::string> names = {"A", "B", "C"};
    for (std::size_t index = 0; index < names.size(); ++index) {
        otio::Clip clip = otio::Clip::create(names[index]);
        const otio::RationalTime start(static_cast<double>(index) * 24, 24);
        clip.set_source_range(otio::TimeRange(start, otio::RationalTime(24, 24)));
        // Appending moves the clip into the timeline's arena. That is
        // bookkeeping this header does for you, not something to hold.
        track.append_child(clip);
    }

    // Three seconds of picture, written as canonical OpenTimelineIO JSON.
    std::cout << track.duration().to_seconds() << "\n";
    otio::save(timeline, "cut.otio");
    return 0;
}

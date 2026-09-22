#include <iostream>

#include <opentimelineio/otio.hpp>

int main() {
    otio::Document document = otio::Document::create();

    otio::Timeline timeline = document.new_timeline("Cut");
    otio::Stack stack = document.new_stack("tracks");
    timeline.set_tracks(stack);
    otio::Track track = document.new_track("V1", "Video");
    stack.append_child(track);

    const std::vector<std::string> names = {"A", "B", "C"};
    for (std::size_t index = 0; index < names.size(); ++index) {
        otio::Clip clip = document.new_clip(names[index]);
        const otio::RationalTime start(static_cast<double>(index) * 24, 24);
        clip.set_source_range(otio::TimeRange(start, otio::RationalTime(24, 24)));
        track.append_child(clip);
    }

    document.set_root(timeline);

    // Three seconds of picture, written as canonical OpenTimelineIO JSON.
    std::cout << track.duration().to_seconds() << "\n";
    document.save("cut.otio");
    return 0;
}

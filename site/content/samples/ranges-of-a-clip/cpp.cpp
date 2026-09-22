#include <iostream>

#include <opentimelineio/otio.hpp>

static std::string frames(const otio::TimeRange &span) {
    return std::to_string(span.start_time.value) + " for " +
           std::to_string(span.duration.value);
}

int main() {
    // Ten seconds of rushes on disk. `available_range` belongs to the media,
    // not to the clip: it is what the file offers, whoever uses it.
    otio::ExternalReference media =
        otio::ExternalReference::create("A001", "file:///A001.mov");
    media.set_available_range(otio::TimeRange(otio::RationalTime(0, 24),
                                              otio::RationalTime(240, 24)));

    // Three seconds of it, starting two seconds in. A source range is in the
    // media's clock, which is why it starts at 48 rather than at 0.
    otio::Clip clip = otio::Clip::create("shot");
    clip.set_media_reference("DEFAULT_MEDIA", media);
    clip.set_source_range(
        otio::TimeRange(otio::RationalTime(48, 24), otio::RationalTime(72, 24)));

    // A second of black in front of it, so the clip does not start the track.
    otio::Gap head = otio::Gap::create("");
    head.set_source_range(
        otio::TimeRange(otio::RationalTime(0, 24), otio::RationalTime(24, 24)));

    otio::Track track = otio::Track::create("V1", "Video");
    track.append_child(head);
    track.append_child(clip);

    // The same clip, asked four questions. The first three answer in the
    // media's clock; the last answers in the track's.
    std::cout << "available: " << frames(clip.available_range()) << "\n";
    std::cout << "trimmed:   " << frames(clip.trimmed_range()) << "\n";
    std::cout << "visible:   " << frames(clip.visible_range()) << "\n";
    std::cout << "in parent: " << frames(clip.range_in_parent()) << "\n";

    return 0;
}

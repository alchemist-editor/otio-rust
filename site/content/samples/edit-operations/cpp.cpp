#include <iostream>
#include <string>

#include <opentimelineio/otio.hpp>

/// One second of picture, named.
static otio::Clip second(const std::string &name) {
    otio::Clip clip = otio::Clip::create(name);
    clip.set_source_range(
        otio::TimeRange(otio::RationalTime(0, 24), otio::RationalTime(24, 24)));
    return clip;
}

static void show(const otio::Track &track) {
    std::string names;
    for (const otio::SerializableObject &child : track.children()) {
        if (!names.empty()) names += " ";
        names += child.name();
    }
    std::cout << names << " - " << track.duration().value << " frames\n";
}

int main() {
    otio::Track track = otio::Track::create("V1", "Video");
    for (const char *name : {"A", "B", "C"}) {
        track.append_child(second(name));
    }
    show(track);

    // Insert makes room: everything from the insertion point onwards moves
    // later, and the track gets longer.
    otio::insert(second("D"), track, otio::RationalTime(24, 24), false);
    show(track);

    // Overwrite does not: it lays an item over a span and whatever was in
    // that span gives way. The track is the same length afterwards.
    otio::overwrite(
        second("E"), track,
        otio::TimeRange(otio::RationalTime(48, 24), otio::RationalTime(24, 24)),
        false);
    show(track);

    return 0;
}

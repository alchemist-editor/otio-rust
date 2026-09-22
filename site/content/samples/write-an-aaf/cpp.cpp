#include <iostream>

#include <opentimelineio/otio.hpp>

int main() {
    otio::Timeline timeline = otio::Timeline::create("Cut");
    otio::Stack stack = otio::Stack::create("tracks");
    otio::Track track = otio::Track::create("V1", "Video");

    timeline.set_tracks(stack);
    stack.append_child(track);

    const otio::TimeRange one_second(otio::RationalTime(0, 24), otio::RationalTime(24, 24));

    // An AAF clip is cut from media of a known length, so each clip's media
    // says how much of it there is. A new clip has no media at all, so its
    // reference goes in under upstream's key and is made the active one.
    const std::vector<std::string> names = {"A001C003", "A001C004"};
    for (const std::string &name : names) {
        otio::ExternalReference media =
            otio::ExternalReference::create(std::nullopt, "file:///media/" + name + ".mov");
        media.set_available_range(one_second);

        otio::Clip clip = otio::Clip::create(name);
        clip.set_media_reference("DEFAULT_MEDIA", media);
        clip.set_active_media_reference_key("DEFAULT_MEDIA");
        clip.set_source_range(one_second);
        track.append_child(clip);
    }

    // Every clip needs a MobID, from its metadata, its media's metadata or
    // the AAF its media names. A cut built from scratch has none, so let the
    // writer make them up rather than refuse the clip.
    otio::WriteOptions options = otio::write_options_default();
    options.aaf_use_empty_mob_ids = true;
    otio::write_to_file(otio::Format::AAF, timeline, "cut.aaf", options);

    const otio::SerializableObject root =
        otio::read_from_file(otio::Format::AAF, "cut.aaf", std::nullopt);

    for (const otio::SerializableObject &node : root.find_clips()) {
        if (std::optional<otio::Clip> clip = node.as<otio::Clip>()) {
            std::cout << clip->name() << "\n";
        }
    }

    return 0;
}

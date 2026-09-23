#include <iostream>
#include <utility>
#include <vector>

#include <opentimelineio/otio.hpp>

int main() {
    otio::Timeline timeline = otio::Timeline::create("Cut");
    otio::Track track = otio::Track::create("V1", "Video");
    timeline.tracks()->as<otio::Stack>()->append_child(track);

    // A cut of two clips: one whose media is a file beside the program, and
    // one whose media is on the web.
    const std::vector<std::pair<std::string, std::string>> sources = {
        {"A001C003", "shot.mov"},
        {"A001C004", "https://example.com/remote.mov"},
    };
    for (const auto &[name, url] : sources) {
        otio::Clip clip = otio::Clip::create(name);
        clip.set_media_reference("DEFAULT_MEDIA", otio::ExternalReference::create(std::nullopt, url));
        clip.set_active_media_reference_key("DEFAULT_MEDIA");
        track.append_child(clip);
    }

    // Every clip whose media is a file has the file copied into the bundle
    // and its reference pointed at the copy. Media that is not a file would
    // stop the write, so it is made missing instead. Format::OTIOD writes
    // the same layout as a directory.
    otio::WriteOptions options = otio::write_options_default();
    options.bundle_media_policy = otio::BundleMediaPolicy::MISSING_IF_NOT_FILE;
    otio::write_to_file(otio::Format::OTIOZ, timeline, "cut.otioz", options);

    // Unpacked, with each reference made absolute, the media is ready to use.
    otio::ReadOptions read = otio::read_options_default();
    read.bundle_extract_path = "cut";
    read.bundle_absolute_media_paths = true;
    const otio::SerializableObject root =
        otio::read_from_file(otio::Format::OTIOZ, "cut.otioz", read);

    for (const otio::SerializableObject &node : root.find_clips()) {
        const std::optional<otio::SerializableObject> media =
            node.as<otio::Clip>()->media_reference();
        if (auto external = media->as<otio::ExternalReference>()) {
            std::cout << external->target_url() << "\n";
        } else {
            std::cout << "missing\n";
        }
    }

    return 0;
}

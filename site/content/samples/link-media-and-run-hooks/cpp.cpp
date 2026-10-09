#include <cstdint>
#include <iostream>
#include <optional>
#include <string>
#include <vector>

#include <opentimelineio/otio.hpp>

// A cut of two clips, as an .otio file would hold it.
const std::string cut = R"({
  "OTIO_SCHEMA": "Track.1",
  "name": "V1",
  "kind": "Video",
  "children": [
    {"OTIO_SCHEMA": "Clip.2", "name": "A"},
    {"OTIO_SCHEMA": "Clip.2", "name": "B"}
  ]
})";

int main() {
    // A media linker is handed each clip as it is read, with the arguments
    // the read was given, and answers with the media the clip should use.
    // An empty optional leaves the clip as it was.
    otio::register_media_linker(
        "proxies",
        [](const otio::Clip &clip, const otio::Metadata &arguments)
            -> std::optional<otio::MediaReference> {
            const std::string root = arguments.get_string("root");
            return otio::ExternalReference::create(
                clip.name(), root + "/" + clip.name() + ".mov");
        });

    // A hook script is handed the whole result, and answers with what the
    // read goes on with: here the same object, stamped. An exception thrown
    // from either fails the read with Status::PLUGIN_ERROR and its message.
    otio::register_hook_script(
        "stamp", [](const otio::SerializableObject &target, const otio::Metadata &arguments) {
            otio::Metadata(target).set_string("read_by", arguments.get_string("who"));
            return target;
        });
    otio::attach_hook_script("post_adapter_read", "stamp");

    // The read names the linker, and carries both sets of arguments as JSON.
    otio::ReadOptions options = otio::read_options_default();
    options.media_linker = "proxies";
    options.media_linker_arguments = R"({"root": "/proxies"})";
    options.hook_arguments = R"({"who": "the conform"})";
    const otio::SerializableObject track = otio::read_from_bytes(
        otio::Format::OTIO_JSON, std::vector<std::uint8_t>(cut.begin(), cut.end()), options);

    std::cout << "read by " << otio::Metadata(track).get_string("read_by") << "\n";
    for (const otio::SerializableObject &node : track.find_clips()) {
        const otio::Clip clip = *node.as<otio::Clip>();
        const otio::ExternalReference media =
            *clip.media_reference()->as<otio::ExternalReference>();
        std::cout << clip.name() << " -> " << media.target_url() << "\n";
    }

    // The registry is the whole process's, so take them out again.
    otio::detach_hook_script("post_adapter_read", "stamp");
    otio::unregister_hook_script("stamp");
    otio::unregister_media_linker("proxies");
    return 0;
}

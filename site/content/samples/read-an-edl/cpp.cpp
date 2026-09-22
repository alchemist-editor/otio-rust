#include <iostream>

#include <opentimelineio/otio.hpp>

int main() {
    // An EDL never says what rate its timecode is at, so this has to be
    // right: a file read at the wrong rate puts every event in the wrong
    // place rather than failing.
    otio::ReadOptions options = otio::Document::read_options_default();
    options.rate = 24;

    otio::Document document =
        otio::Document::read_from_file(otio::Format::CMX_3600, "cut.edl", options);

    if (std::optional<otio::SerializableObject> root = document.root()) {
        for (const otio::SerializableObject &node : root->find_clips()) {
            if (std::optional<otio::Clip> clip = node.as<otio::Clip>()) {
                std::cout << clip->name() << "\n";
            }
        }
    }

    return 0;
}

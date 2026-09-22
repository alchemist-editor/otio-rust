#include <opentimelineio/otio.hpp>

int main() {
    // An EDL never says what rate its timecode is at, so this has to be
    // right: a file read at the wrong rate puts every event in the wrong
    // place rather than failing.
    otio::ReadOptions options = otio::read_options_default();
    options.rate = 24;

    const otio::SerializableObject root =
        otio::read_from_file(otio::Format::CMX_3600, "cut.edl", options);

    // Nothing happens in between. The timeline an EDL parses to is the same
    // timeline FCP X writes out, so converting is a read and a write: the
    // object model is the interchange, and the file formats are two ways of
    // spelling it.
    otio::write_to_file(otio::Format::FCPX_XML, root, "cut.fcpxml");

    return 0;
}

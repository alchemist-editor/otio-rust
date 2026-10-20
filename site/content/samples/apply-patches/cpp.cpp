#include <iostream>

#include <opentimelineio/ext/sync.hpp>
#include <opentimelineio/otio.hpp>

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
const char *const CUT = R"({
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
})";

int main() {
    namespace sync = otio::ext::sync;
    sync::enable();

    // Beginning a transaction gives every object a GUID, and returns the
    // timeline as it was as a snapshot: that snapshot is the transaction.
    // A second program starts from the same stamped timeline.
    otio::Timeline editor = otio::from_json(CUT).as<otio::Timeline>().value();
    const std::string before = sync::begin(editor, "main", 1);
    const otio::Timeline viewer = otio::from_json(editor.to_json(0)).as<otio::Timeline>().value();

    // Edit as usual, then commit for the patches that say what changed.
    editor.set_name("fine cut");
    const std::string patches = sync::commit(editor, before);
    std::cout << patches << "\n";

    // The other copy applies them in place: what it held still names the
    // same objects.
    sync::apply_to(viewer, patches);
    std::cout << "the viewer now shows " << viewer.name() << "\n";
    return 0;
}

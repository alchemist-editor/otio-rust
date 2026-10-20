#include <iostream>

#include <opentimelineio/ext/sync.hpp>

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// `metadata.sync.guid`, which is how a patch names it.
const char *const BEFORE = R"({"sequenceId": "main", "generation": 1, "otioJson": {
  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
     "children": [
       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}},
       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}}]}]}}})";

const char *const AFTER = R"({"sequenceId": "main", "generation": 2, "otioJson": {
  "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
  "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
     "children": [
       {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}},
       {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}}]}]}}})";

int main() {
    namespace sync = otio::ext::sync;
    sync::enable();

    // One move, of A to after B: a patch names objects by GUID. The stats
    // say whether the change was too large for patches and fell back to
    // the whole timeline.
    std::cout << sync::diff(BEFORE, AFTER, true) << "\n";
    std::cout << sync::diff_stats(BEFORE, AFTER) << "\n";
    return 0;
}

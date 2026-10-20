import Foundation
import OTIOExtSync

// Two versions of one sequence, as Furnace sends them: in the second, clip B
// has moved in front of clip A. Every object carries its GUID in
// `metadata.sync.guid`, which is how a patch names it.
let before = """
    {"sequenceId": "main", "generation": 1, "otioJson": {
      "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
      "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
         "children": [
           {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}},
           {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}}]}]}}}
    """

let after = """
    {"sequenceId": "main", "generation": 2, "otioJson": {
      "OTIO_SCHEMA": "Timeline.1", "name": "cut", "metadata": {"sync": {"guid": "cut"}},
      "tracks": {"OTIO_SCHEMA": "Stack.1", "metadata": {"sync": {"guid": "tracks"}}, "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "metadata": {"sync": {"guid": "v1"}},
         "children": [
           {"OTIO_SCHEMA": "Clip.2", "name": "B", "metadata": {"sync": {"guid": "b"}}},
           {"OTIO_SCHEMA": "Clip.2", "name": "A", "metadata": {"sync": {"guid": "a"}}}]}]}}}
    """

try Sync.enable()

let patches = try Sync.diff(before, newSnapshot: after, pretty: true)
let stats = try JSONSerialization.jsonObject(
    with: Data(try Sync.diffStats(before, newSnapshot: after).utf8)) as? [String: Any]

// One move, of A to after B: a patch names objects by GUID.
print(patches)
print("fell back to the whole timeline: \(stats?["fellBack"] as? Bool ?? false)")

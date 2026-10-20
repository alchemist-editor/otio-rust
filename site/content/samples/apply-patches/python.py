import opentimelineio as otio
from opentimelineio.ext import sync

# A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
CUT = """{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "cut",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "A"},
      {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
}"""

sync.enable()

# Beginning a transaction gives every object a GUID, and returns the
# timeline as it was as a snapshot: that snapshot is the transaction. A
# second program starts from the same stamped timeline.
timeline = otio.adapters.read_from_string(CUT, "otio_json")
before = sync.begin(timeline, "main", 1)
shown = otio.adapters.read_from_string(timeline.to_json_string(), "otio_json")

# Edit as usual, then commit for the patches that say what changed.
timeline.name = "fine cut"
patches = sync.commit(timeline, before)
print(patches)

# The other copy applies them in place: what it held is still the same
# object.
sync.apply_to(shown, patches)
print("the viewer now shows", shown.name)

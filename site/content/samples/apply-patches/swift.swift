import OTIOExtSync
import OpenTimelineIO

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
let cut = """
    {
      "OTIO_SCHEMA": "Timeline.1",
      "name": "cut",
      "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "A"},
          {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
    }
    """

try Sync.enable()

// Beginning a transaction gives every object a GUID, and remembers the
// timeline as it was: that snapshot is the transaction. A second program
// starts from the same stamped timeline.
let editor = try OTIO.fromJSON(cut)
let before = try Sync.begin(editor, sequenceID: "main", generation: 1)
let viewer = try OTIO.fromJSON(try editor.toJSON(0))

// Edit as usual, then commit for the patches that say what changed.
try editor.setName("fine cut")
let patches = try Sync.commit(editor, before: before)
print(patches)

// The other copy applies them in place: what it held still names the same
// objects.
try Sync.applyTo(viewer, patches: patches)
print("the viewer now shows \(try viewer.name())")

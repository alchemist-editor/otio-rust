using System;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Sync;

// A cut, as an .otio file would hold it. Nothing in it has a GUID yet.
const string Cut = """
    {
      "OTIO_SCHEMA": "Timeline.1",
      "name": "cut",
      "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "A"},
          {"OTIO_SCHEMA": "Clip.2", "name": "B"}]}]}
    }
    """;

SyncExtension.Enable();

// Beginning a transaction gives every object a GUID, and returns the
// timeline as it was as a snapshot: that snapshot is the transaction. A
// second program starts from the same stamped timeline.
var timeline = Otio.FromJson(Cut);
var before = SyncExtension.Begin(timeline, "main", 1);
var shown = Otio.FromJson(timeline.ToJson(0));

// Edit as usual, then commit for the patches that say what changed.
timeline.SetName("fine cut");
var patches = SyncExtension.Commit(timeline, before);
Console.WriteLine(patches);

// The other copy applies them in place: what it held still names the same
// objects.
SyncExtension.ApplyTo(shown, patches);
Console.WriteLine($"the viewer now shows {shown.Name()}");

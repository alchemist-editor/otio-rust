using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using OpenTimelineIO;
using OpenTimelineIO.Ext.Ori;

// A cut with one shot, which sync knows by the GUID `shot`.
const string Cut = """
    {
      "OTIO_SCHEMA": "Timeline.1",
      "name": "review",
      "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "shot",
           "metadata": {"sync": {"guid": "shot"}}}]}]}
    }
    """;

// ORI Annotations is an extension; enabling it enables sync too.
OriExtension.Enable();
var timeline = Otio.FromJson(Cut);

// The Annotations track, and the clip on it for frame 10 of the shot.
var track = OriExtension.EnsureTrack(timeline);
var frame = OriExtension.EnsureClip(track, "shot", 10, 24);

// ORI's coordinates are height-normalised with the origin at the centre,
// so convert from the pixels of the 1920x1080 picture.
var points = new[] { (200.0, 200.0), (960.0, 540.0), (1720.0, 880.0) }
    .Select(pixel => OriExtension.PixelsToOri(1920, 1080, pixel.Item1, pixel.Item2))
    .ToArray();

// A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
// Fields left out take the defaults ORI's classes give them.
var uuid = OriExtension.NewUuid();
var stroke = JsonSerializer.Serialize(new object[]
{
    new Dictionary<string, object>
    {
        ["OTIO_SCHEMA"] = "PaintStart.1", ["uuid"] = uuid,
        ["rgba"] = new[] { 1.0, 0.25, 0.25, 1.0 }, ["participant_hash"] = "reviewer-1",
    },
    new Dictionary<string, object>
    {
        ["OTIO_SCHEMA"] = "PaintPoint.1", ["uuid"] = uuid,
        ["points"] = new Dictionary<string, object>
        {
            ["OTIO_SCHEMA"] = "PaintVertices.1",
            ["x"] = points.Select(point => point.x).ToArray(),
            ["y"] = points.Select(point => point.y).ToArray(),
            ["size"] = points.Select(_ => 0.01).ToArray(),
            ["alpha"] = points.Select(_ => 1.0).ToArray(),
        },
    },
    new Dictionary<string, object> { ["OTIO_SCHEMA"] = "PaintEnd.1", ["uuid"] = uuid },
});
OriExtension.AddCommands(frame, stroke);

// Read them back: every annotation on the shot.
foreach (var holder in OriExtension.ListForClip(timeline, "shot"))
{
    Console.WriteLine(OriExtension.Describe(holder));
}

// The file is plain OpenTimelineIO with ORI's schemas in it.
Console.WriteLine(timeline.ToJson(4));

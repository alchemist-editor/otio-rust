import Foundation
import OTIOExtOri
import OpenTimelineIO

// A cut with one shot, which sync knows by the GUID `shot`.
let cut = """
    {"OTIO_SCHEMA": "Timeline.1", "name": "review",
     "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
       {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
         {"OTIO_SCHEMA": "Clip.2", "name": "shot",
          "metadata": {"sync": {"guid": "shot"}}}]}]}}
    """

// ORI Annotations is an extension; enabling it enables sync too.
try Ori.enable()
let timeline = try OTIO.fromJSON(cut)

// The Annotations track, and the clip on it for frame 10 of the shot.
let track = try Ori.ensureTrack(timeline)
let frame = try Ori.ensureClip(track, clipGuid: "shot", value: 10, rate: 24)

// ORI's coordinates are height-normalised with the origin at the centre,
// so convert from the pixels of the 1920x1080 picture.
let points = try [(200.0, 200.0), (960.0, 540.0), (1720.0, 880.0)].map {
    try Ori.pixelsToOri(1920, height: 1080, px: $0.0, py: $0.1)
}

// A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
// Fields left out take the defaults ORI's classes give them.
let uuid = try Ori.newUUID()
let stroke: [[String: Any]] = [
    ["OTIO_SCHEMA": "PaintStart.1", "uuid": uuid, "rgba": [1, 0.25, 0.25, 1],
     "participant_hash": "reviewer-1"],
    ["OTIO_SCHEMA": "PaintPoint.1", "uuid": uuid, "points": [
        "OTIO_SCHEMA": "PaintVertices.1",
        "x": points.map(\.x), "y": points.map(\.y),
        "size": points.map { _ in 0.01 }, "alpha": points.map { _ in 1.0 },
    ]],
    ["OTIO_SCHEMA": "PaintEnd.1", "uuid": uuid],
]
let json = String(decoding: try JSONSerialization.data(withJSONObject: stroke), as: UTF8.self)
try Ori.addCommands(frame, commandsJSON: json)

// Read them back: every annotation on the shot.
for holder in try Ori.listForClip(timeline, clipGuid: "shot") {
    print(try Ori.describe(holder))
}

// The file is plain OpenTimelineIO with ORI's schemas in it.
print(try timeline.toJSON(4))

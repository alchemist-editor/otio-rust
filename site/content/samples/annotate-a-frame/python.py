import json

import opentimelineio as otio
from opentimelineio.ext import ori

# A cut with one shot, which sync knows by the GUID `shot`.
CUT = """{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "review",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "shot",
       "metadata": {"sync": {"guid": "shot"}}}]}]}
}"""

# ORI Annotations is an extension; enabling it enables sync too.
ori.enable()
timeline = otio.adapters.read_from_string(CUT, "otio_json")

# The Annotations track, and the clip on it for frame 10 of the shot.
track = ori.ensure_track(timeline)
frame = ori.ensure_clip(track, "shot", otio.opentime.RationalTime(10, 24))

# ORI's coordinates are height-normalised with the origin at the centre,
# so convert from the pixels of the 1920x1080 picture.
points = [ori.pixels_to_ori(1920, 1080, px, py) for px, py in [(200, 200), (960, 540), (1720, 880)]]

# A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
# Fields left out take the defaults ORI's classes give them.
uuid = ori.new_uuid()
ori.add_commands(frame, [
    {"OTIO_SCHEMA": "PaintStart.1", "uuid": uuid, "rgba": [1.0, 0.25, 0.25, 1.0],
     "participant_hash": "reviewer-1", "timestamp": ori.timestamp_now()},
    {"OTIO_SCHEMA": "PaintPoint.1", "uuid": uuid, "points": {
        "OTIO_SCHEMA": "PaintVertices.1",
        "x": [x for x, _ in points], "y": [y for _, y in points],
        "size": [0.01] * len(points), "alpha": [1.0] * len(points)}},
    {"OTIO_SCHEMA": "PaintEnd.1", "uuid": uuid},
])

# Read them back: every annotation on the shot.
for holder in ori.list_for_clip(timeline, "shot"):
    about = ori.describe(holder)
    print(f"frame {about['frame']['value']}: {about['command_count']} commands")
    print(json.dumps(json.loads(ori.commands(holder))[0], indent=2))

# The file is plain OpenTimelineIO with ORI's schemas in it.
print(otio.adapters.write_to_string(timeline))

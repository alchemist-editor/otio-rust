import { init, Timeline, deserializeJsonFromString } from "@alchemist-edit/otio";
import * as ori from "@alchemist-edit/otio/ext/ori";

// A cut with one shot, which sync knows by the GUID `shot`.
const CUT = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "review",
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "Clip.2", "name": "shot",
       "metadata": {"sync": {"guid": "shot"}}}]}]}
}`;

await init();
// ORI Annotations is an extension; enabling it enables sync too.
ori.enable();
const timeline = deserializeJsonFromString(CUT) as Timeline;

// The Annotations track, and the clip on it for frame 10 of the shot.
const track = ori.ensureTrack(timeline);
const frame = ori.ensureClip(track, "shot", 10, 24);

// ORI's coordinates are height-normalised with the origin at the centre,
// so convert from the pixels of the 1920x1080 picture.
const points = [[200, 200], [960, 540], [1720, 880]].map(([px, py]) => ori.pixelsToOri(1920, 1080, px!, py!));

// A stroke is a PaintStart, its points and a PaintEnd sharing one uuid.
// Fields left out take the defaults ORI's classes give them.
const uuid = ori.newUuid();
ori.addCommands(frame, JSON.stringify([
  { OTIO_SCHEMA: "PaintStart.1", uuid, rgba: [1, 0.25, 0.25, 1], participant_hash: "reviewer-1" },
  {
    OTIO_SCHEMA: "PaintPoint.1", uuid, points: {
      OTIO_SCHEMA: "PaintVertices.1",
      x: points.map((point) => point.x), y: points.map((point) => point.y),
      size: points.map(() => 0.01), alpha: points.map(() => 1),
    },
  },
  { OTIO_SCHEMA: "PaintEnd.1", uuid },
]));

// Read them back: every annotation on the shot.
for (const holder of ori.listForClip(timeline, "shot")) {
  console.log(ori.describe(holder));
}

// The file is plain OpenTimelineIO with ORI's schemas in it.
console.log(timeline.toJson(4));

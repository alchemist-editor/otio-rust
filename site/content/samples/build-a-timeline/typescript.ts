import { writeFile } from "node:fs/promises";
import { init, Clip, RationalTime, Timeline, TimeRange, Track, serializeJsonToString } from "@alchemist-edit/otio";

await init();

// A fresh timeline arrives with an empty stack called `tracks`, as
// upstream's does, so there is nothing to build before appending to it.
const timeline = new Timeline({ name: "Cut" });
const track = new Track({ name: "V1", kind: "Video" });
timeline.tracks?.appendChild(track);

["A", "B", "C"].forEach((name, index) => {
  const clip = new Clip({ name });
  clip.sourceRange = new TimeRange(
    new RationalTime(index * 24, 24),
    new RationalTime(24, 24),
  );
  track.appendChild(clip);
});

// Three seconds of picture, written as canonical OpenTimelineIO JSON.
console.log(track.duration().toSeconds());
await writeFile("cut.otio", serializeJsonToString(timeline));

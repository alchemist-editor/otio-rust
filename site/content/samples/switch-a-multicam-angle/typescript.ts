import { writeFile } from "node:fs/promises";
import { init, multicam, deserializeJsonFromString, serializeJsonToString, Timeline } from "@alchemist-edit/otio";

// Two cameras on one interview, and an edit with two cuts on camera A.
const EDIT = `{
  "OTIO_SCHEMA": "Timeline.1",
  "name": "interview",
  "multicams": {
    "mc1": {"OTIO_SCHEMA": "Multicam.1", "name": "Interview", "children": [
      {"OTIO_SCHEMA": "Stack.1", "name": "A", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "A V", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "A_take",
           "source_range": {"OTIO_SCHEMA": "TimeRange.1",
             "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
             "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}}}]}]},
      {"OTIO_SCHEMA": "Stack.1", "name": "B", "children": [
        {"OTIO_SCHEMA": "Track.1", "name": "B V", "kind": "Video", "children": [
          {"OTIO_SCHEMA": "Clip.2", "name": "B_take",
           "source_range": {"OTIO_SCHEMA": "TimeRange.1",
             "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
             "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 240, "rate": 24}}}]}]}]}
  },
  "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
    {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
      {"OTIO_SCHEMA": "MulticamClip.1", "name": "cut 1", "multicam": "mc1", "angle": "A",
       "source_range": {"OTIO_SCHEMA": "TimeRange.1",
         "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 0, "rate": 24},
         "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}},
      {"OTIO_SCHEMA": "MulticamClip.1", "name": "cut 2", "multicam": "mc1", "angle": "A",
       "source_range": {"OTIO_SCHEMA": "TimeRange.1",
         "start_time": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24},
         "duration": {"OTIO_SCHEMA": "RationalTime.1", "value": 48, "rate": 24}}}]}]}
}`;

await init();
const timeline = deserializeJsonFromString(EDIT) as Timeline;
const cuts = timeline.tracks?.childAt(0).children() ?? [];

// Switch the second cut to camera B. It keeps its place and its range.
multicam.clipSetAngle(cuts[1]!, "B", 0);

// What each cut plays: a track of one angle.
for (const cut of cuts) {
  console.log(`${cut.name} plays ${multicam.resolve(timeline, cut).name}`);
}
if (multicam.validate(timeline) !== "") throw new Error("the multicams do not resolve");
await writeFile("edit.otio", serializeJsonToString(timeline));

// For a reader without multicam: each cut becomes the material it plays.
const lost = multicam.flatten(timeline);
console.log(`flattening lost: ${lost || "nothing"}`);
await writeFile("flat.otio", serializeJsonToString(timeline));

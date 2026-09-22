import { writeFile } from "node:fs/promises";
import {
  init,
  Clip,
  ExternalReference,
  RationalTime,
  Timeline,
  TimeRange,
  Track,
  readFromBytes,
  writeToBytes,
} from "@otio/otio";

await init();

const timeline = new Timeline({ name: "Cut" });
const track = new Track({ name: "V1", kind: "Video" });
timeline.tracks?.appendChild(track);

const oneSecond = new TimeRange(new RationalTime(0, 24), new RationalTime(24, 24));

// An AAF clip is cut from media of a known length, so each clip's media says
// how much of it there is. A new clip has no media at all, so its reference
// goes in under upstream's key and is made the active one.
for (const name of ["A001C003", "A001C004"]) {
  const media = new ExternalReference({ targetUrl: `file:///media/${name}.mov` });
  media.availableRange = oneSecond;

  const clip = new Clip({ name });
  clip.setMediaReference("DEFAULT_MEDIA", media);
  clip.activeMediaReferenceKey = "DEFAULT_MEDIA";
  clip.sourceRange = oneSecond;
  track.appendChild(clip);
}

// Every clip needs a MobID, from its metadata, its media's metadata or the
// AAF its media names. A cut built from scratch has none, so let the writer
// make them up rather than refuse the clip. The module has no clock or
// randomness of its own, so this side supplies the time and a seed.
const aaf = writeToBytes("aaf", timeline, { aafUseEmptyMobIds: true });
await writeFile("cut.aaf", aaf);

for (const clip of readFromBytes("aaf", aaf).findClips()) {
  console.log(clip.name);
}
